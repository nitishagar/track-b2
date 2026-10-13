//! Real `OpenSTA` `report_checks` text ingest.
//!
//! Grammar (verified against `OpenSTA` goldens + `ReportPath.cc`, read-only):
//! 4-line headers, a `Delay Time Description` table with a dash rule, arrival
//! rows, a blank line, required rows, a dash rule, and a 3-line footer whose
//! arrival value is negated. Stage increments come from Time-column deltas
//! (Delay is cross-checked, not trusted) so `-digits` rounding cannot corrupt
//! accounting; the inferred half-ULP rides on the path for validation.

use crate::parser::{err, ParseError};
use crate::schema::{
    Check, ClockSegment, Edge, Run, TimingPath, TimingPoint, TimingReport, Transition,
};

/// Parse options. Times in the report scale by `time_unit_ns` (reports do
/// not declare their unit; default assumes ns).
#[derive(Debug, Clone)]
pub struct ParseOpts {
    pub time_unit_ns: f64,
    pub strict: bool,
    pub max_errors: usize,
}

/// Streaming parse statistics.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IterStats {
    pub blocks: u64,
    pub skipped: u64,
    pub errors: u64,
}

impl Default for ParseOpts {
    fn default() -> Self {
        Self {
            time_unit_ns: 1.0,
            strict: false,
            max_errors: 20,
        }
    }
}

/// Recognized report layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    StaText,
    OtpsLegacy,
    StaJson,
    OtpsJson,
}

/// Stats accessor shared by the streaming path iterators (text + JSON).
pub trait StreamStats {
    fn stats(&self) -> IterStats;
}

impl<R: std::io::BufRead> StreamStats for PathIter<R> {
    fn stats(&self) -> IterStats {
        self.stats
    }
}

/// Sniff a plain-text report. v0.1 legacy layouts report as [`InputKind::OtpsLegacy`]
/// and fail later with a migration pointer; mixed layouts are rejected.
///
/// # Errors
///
/// Returns [`ParseError`] when the layout is unrecognized or mixed.
pub fn detect_text_kind(text: &str) -> Result<InputKind, ParseError> {
    let mut legacy = false;
    let mut sta = false;
    for line in text.lines().take(200) {
        let line = line.trim();
        if line.starts_with("Launch clock:") {
            legacy = true;
        } else if line.starts_with("Path Group:") {
            sta = true;
        }
    }
    match (sta, legacy) {
        (true, false) => Ok(InputKind::StaText),
        (false, true) => Ok(InputKind::OtpsLegacy),
        (true, true) => Err(err(1, "mixed report layouts in one file")),
        (false, false) => Err(err(1, "unrecognized report layout")),
    }
}

/// Table columns derived from the header row. Only Fanout/Delay/Time are
/// supported; Cap/Slew/Variation need positional parsing (future work) and
/// are rejected with guidance.
/// One byte; passed by value.
#[derive(Debug, Clone, Copy)]
struct Columns {
    fanout: bool,
}

fn parse_table_header(line: &str, lineno: usize) -> Result<Columns, ParseError> {
    let mut fanout = false;
    let mut delay = false;
    let mut time = false;
    let mut saw_description = false;
    for tok in line.split_whitespace() {
        if tok == "Description" {
            saw_description = true;
            break;
        }
        match tok {
            "Fanout" => fanout = true,
            "Delay" => delay = true,
            "Time" | "Total" => time = true,
            "edge" => {}
            _ => {
                return Err(err(
                    lineno,
                    format!("unsupported table column '{tok}' (only Fanout/Delay/Time)"),
                ));
            }
        }
    }
    if !saw_description {
        return Err(err(lineno, "expected table header with Description"));
    }
    if !delay || !time {
        return Err(err(lineno, "table header lacks Delay/Time columns"));
    }
    Ok(Columns { fanout })
}

fn is_num(tok: &str) -> bool {
    tok.bytes().any(|b| b.is_ascii_digit()) && tok.parse::<f64>().is_ok()
}

fn parse_num(tok: &str, lineno: usize) -> Result<f64, ParseError> {
    let value: f64 = tok
        .parse()
        .map_err(|_| err(lineno, format!("bad number '{tok}'")))?;
    if !value.is_finite() {
        return Err(err(lineno, format!("non-finite number '{tok}'")));
    }
    Ok(value)
}

/// Numeric cells of one table row plus the description text.
struct RowNums {
    fanout: Option<u32>,
    delay: Option<f64>,
    time: Option<f64>,
}

fn tokenize(
    line: &str,
    cols: Columns,
    lineno: usize,
    unit: f64,
) -> Result<(RowNums, String), ParseError> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let mut n = 0;
    while n < toks.len() && is_num(toks[n]) {
        n += 1;
    }
    let (mut run, desc_toks) = toks.split_at(n);
    let mut fanout = None;
    // Fanout prints bare (`2`); delays/times always carry `.`/exponent.
    if cols.fanout && !run.is_empty() && run[0].bytes().all(|b| b.is_ascii_digit()) {
        let v = parse_num(run[0], lineno)?;
        if v.fract() != 0.0 || v < 0.0 || v > f64::from(u32::MAX) {
            return Err(err(lineno, format!("bad fanout '{}'", run[0])));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            fanout = Some(v as u32);
        }
        run = &run[1..];
    }
    let (delay, time) = match run {
        [] => (None, None),
        [t] => (None, Some(parse_num(t, lineno)? * unit)),
        [d, t] => (
            Some(parse_num(d, lineno)? * unit),
            Some(parse_num(t, lineno)? * unit),
        ),
        _ => return Err(err(lineno, "too many numbers for table columns")),
    };
    Ok((
        RowNums {
            fanout,
            delay,
            time,
        },
        desc_toks.join(" "),
    ))
}

/// One pin row; `span` (Time delta vs `prev`) is filled during assembly.
struct PinRow {
    pin: String,
    cell: String,
    net: String,
    transition: Option<Transition>,
    input_hint: bool,
    delay: Option<f64>,
    time: f64,
    prev: Option<f64>,
    span: f64,
    fanout: Option<u32>,
    lineno: usize,
}

fn parse_clock_desc(desc: &str, lineno: usize) -> Result<(String, Edge), ParseError> {
    let toks: Vec<&str> = desc.split_whitespace().collect();
    if toks.len() != 4 || toks[0] != "clock" || toks[3] != "edge)" {
        return Err(err(lineno, format!("bad clock row '{desc}'")));
    }
    let edge = match toks[2] {
        "(rise" => Edge::Rise,
        "(fall" => Edge::Fall,
        _ => return Err(err(lineno, format!("bad clock edge '{desc}'"))),
    };
    Ok((toks[1].to_owned(), edge))
}

fn parse_pin_desc(
    desc: &str,
    lineno: usize,
    strict: bool,
) -> Result<(Option<Transition>, String, String, bool), ParseError> {
    let toks: Vec<&str> = desc.split_whitespace().collect();
    if toks.is_empty() {
        return Err(err(lineno, "empty pin row"));
    }
    let mut idx = 0;
    let mut transition = None;
    let mut input_hint = false;
    // `^`/`v` lead a pin row only when a pin follows (else a lone single-char
    // pin name would be eaten as a marker).
    if toks.len() > 1 && (toks[0] == "^" || toks[0] == "v") {
        transition = Some(if toks[0] == "^" {
            Transition::Rise
        } else {
            Transition::Fall
        });
        idx = 1;
    } else if toks[0].len() == 1 && !toks[0].bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(err(lineno, format!("bad transition marker '{}'", toks[0])));
    }
    let Some(pin) = toks.get(idx) else {
        return Err(err(lineno, "pin row without pin"));
    };
    idx += 1;
    let mut cell = String::new();
    if let Some(role) = toks.get(idx) {
        if role.starts_with('(') {
            let inner = role.trim_matches(|c| c == '(' || c == ')');
            match inner {
                "in" => input_hint = true,
                "out" => {}
                _ => inner.clone_into(&mut cell),
            }
            idx += 1;
        }
    }
    if idx < toks.len() && strict {
        return Err(err(lineno, format!("unexpected tokens in '{desc}'")));
    }
    Ok((transition, (*pin).to_owned(), cell, input_hint))
}

fn instance_of(pin: &str) -> &str {
    pin.rsplit_once('/').map_or(pin, |(parent, _)| parent)
}

fn leaf_of(pin: &str) -> &str {
    pin.rsplit_once('/').map_or(pin, |(_, leaf)| leaf)
}

fn is_clock_pin_leaf(pin: &str) -> bool {
    matches!(
        leaf_of(pin).to_ascii_lowercase().as_str(),
        "ck" | "ckn" | "g" | "gn"
    )
}

/// Raw rows of one arrival/required section (pass 1 output).
#[derive(Default)]
struct RawSection {
    clock_rows: Vec<(String, Edge, f64, usize)>,
    srclat: Vec<(f64, usize)>,
    net_scalar: Vec<(f64, usize)>,
    macro_delay: Vec<(f64, usize)>,
    unc: Vec<(f64, usize)>,
    crpr: Vec<(f64, usize)>,
    margin: Vec<(f64, usize)>,
    external: Vec<(f64, usize)>,
    pins: Vec<PinRow>,
    end_time: Option<f64>,
}

#[allow(clippy::too_many_arguments)]
fn classify_row(
    nums: &RowNums,
    desc: &str,
    lineno: usize,
    strict: bool,
    section: &mut RawSection,
    pending_net: &mut Option<String>,
    prev: &mut Option<f64>,
    is_arrival: bool,
) -> Result<(), ParseError> {
    if desc == "data arrival time" || desc == "data required time" {
        if nums.delay.is_some() {
            return Err(err(lineno, format!("unexpected Delay on '{desc}'")));
        }
        let Some(time) = nums.time else {
            return Err(err(lineno, format!("'{desc}' without Time")));
        };
        if section.end_time.is_some() {
            return Err(err(lineno, format!("duplicate '{desc}'")));
        }
        section.end_time = Some(time);
        return Ok(());
    }
    // NOTE: specific `clock …` vocabularies precede the generic clock row.
    if desc == "clock source latency" {
        let Some(delay) = nums.delay else {
            return Err(err(lineno, "source latency without Delay"));
        };
        section.srclat.push((delay, lineno));
        return Ok(());
    }
    if desc.starts_with("clock network delay") {
        let Some(delay) = nums.delay else {
            return Err(err(lineno, "network delay without Delay"));
        };
        section.net_scalar.push((delay, lineno));
        return Ok(());
    }
    if desc == "macro clock tree delay" {
        let Some(delay) = nums.delay else {
            return Err(err(lineno, "macro delay without Delay"));
        };
        section.macro_delay.push((delay, lineno));
        return Ok(());
    }
    if !is_arrival && classify_required_vocab(nums, desc, lineno, section)? {
        return Ok(());
    }
    if desc.starts_with("clock ") {
        let Some(time) = nums.time else {
            return Err(err(lineno, "clock row without Time"));
        };
        let (name, edge) = parse_clock_desc(desc, lineno)?;
        section.clock_rows.push((name, edge, time, lineno));
        return Ok(());
    }
    classify_pin_row(nums, desc, lineno, strict, section, pending_net, prev)
}

/// Required-side vocabulary. Returns whether the row was handled.
fn classify_required_vocab(
    nums: &RowNums,
    desc: &str,
    lineno: usize,
    section: &mut RawSection,
) -> Result<bool, ParseError> {
    if desc == "clock uncertainty" || desc == "inter-clock uncertainty" {
        let Some(delay) = nums.delay else {
            return Err(err(lineno, format!("'{desc}' without Delay")));
        };
        section.unc.push((delay, lineno));
        return Ok(true);
    }
    if desc == "clock reconvergence pessimism" {
        let Some(delay) = nums.delay else {
            return Err(err(lineno, format!("'{desc}' without Delay")));
        };
        section.crpr.push((delay, lineno));
        return Ok(true);
    }
    if matches!(
        desc,
        "library setup time"
            | "library hold time"
            | "data check setup time"
            | "data check hold time"
    ) {
        let Some(delay) = nums.delay else {
            return Err(err(lineno, format!("'{desc}' without Delay")));
        };
        section.margin.push((delay, lineno));
        return Ok(true);
    }
    if desc == "input external delay" || desc == "output external delay" {
        let Some(delay) = nums.delay else {
            return Err(err(lineno, format!("'{desc}' without Delay")));
        };
        section.external.push((delay, lineno));
        return Ok(true);
    }
    if desc.starts_with("time borrowed") || desc.starts_with("time given") {
        return Err(err(lineno, "latch borrowing rows unsupported"));
    }
    Ok(false)
}

/// Net-name rows and pin rows.
#[allow(clippy::too_many_arguments)]
fn classify_pin_row(
    nums: &RowNums,
    desc: &str,
    lineno: usize,
    strict: bool,
    section: &mut RawSection,
    pending_net: &mut Option<String>,
    prev: &mut Option<f64>,
) -> Result<(), ParseError> {
    if desc.ends_with("(net)") && nums.time.is_none() {
        let name = desc
            .strip_suffix("(net)")
            .map_or(desc, str::trim)
            .trim()
            .to_owned();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(err(lineno, format!("bad net row '{desc}'")));
        }
        *pending_net = Some(name);
        return Ok(());
    }
    let Some(time) = nums.time else {
        return Err(err(lineno, format!("unsupported row '{desc}'")));
    };
    let (transition, pin, cell, input_hint) = parse_pin_desc(desc, lineno, strict)?;
    section.pins.push(PinRow {
        pin,
        cell,
        net: pending_net.take().unwrap_or_default(),
        transition,
        input_hint,
        delay: nums.delay,
        time,
        prev: *prev,
        span: 0.0,
        fanout: nums.fanout,
        lineno,
    });
    Ok(())
}

fn is_dash_rule(line: &str) -> bool {
    line.len() >= 3 && line.bytes().all(|b| b == b'-')
}

fn is_banner(line: &str) -> bool {
    line.starts_with("---")
}

fn strip_name(raw: &str) -> &str {
    raw.split(" (").next().unwrap_or(raw).trim()
}

fn clocked_by(raw: &str) -> Option<String> {
    raw.find("clocked by ").map(|i| {
        raw[i + "clocked by ".len()..]
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(')')
            .to_owned()
    })
}

fn skip_blanks(lines: &[(usize, String)], idx: &mut usize) {
    while *idx < lines.len() && lines[*idx].1.is_empty() {
        *idx += 1;
    }
}

/// Half-ULP in ns inferred from a printed token's decimals.
fn half_ulp(token: &str, unit: f64) -> f64 {
    let (mantissa, exp) = match token.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (token, 0),
    };
    let decimals = mantissa.split('.').nth(1).map_or(0, |d| {
        i32::try_from(d.bytes().take_while(u8::is_ascii_digit).count().min(12)).unwrap_or(12)
    });
    0.5 * 10f64.powi(exp - decimals) * unit
}

fn slack_token(line: &str) -> &str {
    line.split_whitespace()
        .take_while(|t| is_num(t))
        .last()
        .unwrap_or("0")
}

/// How one pin row's span splits into cell vs net.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Input pin: span is the arriving net arc.
    Input,
    /// Output pin after its input: span is the cell arc.
    Cell,
    /// Folded span (net+cell combined, no input-pin rows to split by).
    Combined,
}

/// Assign roles over the section's full pin list: explicit `(in)` hint and
/// clock-leaf pins (always inputs), then same-instance follower (input),
/// then same-instance predecessor (cell), then trailing endpoint (input),
/// else folded.
fn assign_roles(pins: &[PinRow]) -> Vec<Role> {
    pins.iter()
        .enumerate()
        .map(|(i, pin)| {
            let same_next = pins
                .get(i + 1)
                .is_some_and(|n| instance_of(&n.pin) == instance_of(&pin.pin));
            let same_prev = i > 0 && instance_of(&pins[i - 1].pin) == instance_of(&pin.pin);
            if pin.input_hint || is_clock_pin_leaf(&pin.pin) || same_next {
                Role::Input
            } else if same_prev {
                Role::Cell
            } else if i + 1 == pins.len() {
                Role::Input
            } else {
                Role::Combined
            }
        })
        .collect()
}

fn build_point(pin: &PinRow, role: Role) -> TimingPoint {
    let (cell_delay, net_delay, split_available) = match role {
        Role::Input => (0.0, pin.span, true),
        Role::Cell => (pin.span, 0.0, true),
        Role::Combined => (pin.span, 0.0, false),
    };
    TimingPoint {
        pin: pin.pin.clone(),
        cell: pin.cell.clone(),
        net: pin.net.clone(),
        cell_delay,
        net_delay,
        slew: 0.0,
        cap: 0.0,
        fanout: pin.fanout.unwrap_or(0),
        derate: None,
        sigma: None,
        si_delta: None,
        transition: pin.transition,
        arrival: Some(pin.time),
        split_available,
    }
}

/// Assembled clock side plus its end time.
struct Side {
    segment: Option<ClockSegment>,
    end_time: f64,
}

struct SectionAsm {
    side: Side,
    data_points: Vec<TimingPoint>,
    unc_sum: f64,
    crpr: Option<f64>,
    margin: Option<f64>,
    external: Option<f64>,
}

#[allow(clippy::too_many_lines)]
fn assemble_section(
    section: &mut RawSection,
    clock_fallback: &str,
    half_ulp_ns: f64,
    ctx: usize,
    side_name: &str,
    is_arrival: bool,
    end_label: &str,
) -> Result<SectionAsm, ParseError> {
    if section.clock_rows.len() > 1 {
        return Err(err(
            section.clock_rows[1].3,
            "generated-clock source paths unsupported",
        ));
    }
    for (rows, what) in [
        (&section.srclat, "source latency"),
        (&section.net_scalar, "network delay"),
        (&section.crpr, "reconvergence pessimism"),
        (&section.margin, "margin"),
        (&section.external, "external delay"),
    ] {
        if rows.len() > 1 {
            return Err(err(rows[1].1, format!("duplicate {what}")));
        }
    }
    if !section.margin.is_empty() && !section.external.is_empty() {
        return Err(err(
            section.external[0].1,
            "both margin and external delay present",
        ));
    }
    // Spans from Time deltas; Delay cross-checks within rounding.
    let xcheck = 3.0 * half_ulp_ns + 1e-9;
    for pin in &mut section.pins {
        pin.span = pin.time - pin.prev.unwrap_or(0.0);
        if let Some(delay) = pin.delay {
            if (delay - pin.span).abs() > xcheck {
                return Err(err(
                    pin.lineno,
                    format!(
                        "row inconsistent: Delay {delay:.6} vs Time-delta {:.6}",
                        pin.span
                    ),
                ));
            }
        }
    }
    let roles = assign_roles(&section.pins);
    let points: Vec<TimingPoint> = section
        .pins
        .iter()
        .zip(roles.iter())
        .map(|(pin, role)| build_point(pin, *role))
        .collect();
    // Clock/data split (arrival): data starts at the first CK/G pin.
    let (clock_points, data_points) = if is_arrival {
        let at = section
            .pins
            .iter()
            .position(|p| is_clock_pin_leaf(&p.pin))
            .unwrap_or(points.len());
        (
            points[..at.min(points.len())].to_vec(),
            points[at.min(points.len())..].to_vec(),
        )
    } else {
        (points, Vec::new())
    };
    if !section.net_scalar.is_empty() && !clock_points.is_empty() {
        return Err(err(ctx, format!("mixed {side_name} clock network form")));
    }
    let network_delay = section.net_scalar.first().map_or_else(
        || clock_points.iter().map(TimingPoint::incr).sum(),
        |(v, _)| *v,
    );
    let has_clock = !section.clock_rows.is_empty()
        || !section.net_scalar.is_empty()
        || !section.srclat.is_empty()
        || !section.macro_delay.is_empty()
        || !clock_points.is_empty();
    let segment = if has_clock {
        let (name, edge, edge_time) = section.clock_rows.first().map_or_else(
            || (clock_fallback.to_owned(), Edge::Unknown, None),
            |(name, edge, time, _)| (name.clone(), *edge, Some(*time)),
        );
        Some(ClockSegment {
            clock: name,
            edge,
            edge_time,
            source_latency: section.srclat.first().map_or(0.0, |(v, _)| *v),
            network_delay,
            macro_delay: section.macro_delay.iter().map(|(v, _)| *v).sum(),
            condensed: clock_points.is_empty(),
            stages: clock_points.clone(),
        })
    } else {
        None
    };
    let Some(end_time) = section.end_time else {
        return Err(err(ctx, format!("missing {end_label}")));
    };
    Ok(SectionAsm {
        side: Side { segment, end_time },
        data_points,
        unc_sum: section.unc.iter().map(|(v, _)| *v).sum(),
        crpr: section.crpr.first().map(|(v, _)| *v),
        margin: section.margin.first().map(|(v, _)| *v),
        external: section.external.first().map(|(v, _)| *v),
    })
}

fn header_line(
    lines: &[(usize, String)],
    idx: &mut usize,
    prefix: &str,
) -> Result<(usize, String), ParseError> {
    let Some((n, line)) = lines.get(*idx) else {
        return Err(err(
            lines.last().map_or(1, |(n, _)| *n),
            format!("truncated block, expected '{prefix}'"),
        ));
    };
    if !line.starts_with(prefix) {
        return Err(err(*n, format!("expected '{prefix}'")));
    }
    let out = (line[prefix.len()..].trim().to_owned(), *n);
    *idx += 1;
    Ok((out.1, out.0))
}

#[allow(clippy::too_many_lines)]
fn parse_block(
    lines: &[(usize, String)],
    idx: &mut usize,
    opts: &ParseOpts,
) -> Result<Option<TimingPath>, ParseError> {
    // Block extent for the unconstrained scan: up to the next Startpoint.
    let mut end = *idx + 1;
    while end < lines.len() && !lines[end].1.starts_with("Startpoint:") {
        end += 1;
    }
    if lines[*idx..end]
        .iter()
        .any(|(_, l)| l.contains("(Path is unconstrained)"))
    {
        *idx = end;
        return Ok(None);
    }
    let ctx = lines[*idx].0;
    let (_, start_raw) = header_line(lines, idx, "Startpoint:")?;
    let (_, end_raw) = header_line(lines, idx, "Endpoint:")?;
    let (_, group) = header_line(lines, idx, "Path Group:")?;
    let (_, type_raw) = header_line(lines, idx, "Path Type:")?;
    let check = match type_raw.as_str() {
        "max" => Check::Setup,
        "min" => Check::Hold,
        other => return Err(err(ctx, format!("unsupported Path Type '{other}'"))),
    };
    skip_blanks(lines, idx);
    let Some((_, table)) = lines.get(*idx) else {
        return Err(err(ctx, "no timing table (short format?)"));
    };
    if !table.contains("Description") {
        return Err(err(ctx, "no timing table (short format?)"));
    }
    let table_lineno = lines[*idx].0;
    let cols = parse_table_header(table, table_lineno)?;
    *idx += 1;
    let Some((n, rule)) = lines.get(*idx) else {
        return Err(err(ctx, "truncated block, expected dash rule"));
    };
    if !is_dash_rule(rule) {
        return Err(err(*n, "expected dash rule under table header"));
    }
    *idx += 1;

    let mut pending_net = None;
    let mut prev = None;
    let mut arrival = RawSection::default();
    // Arrival rows run to the first blank line.
    loop {
        let Some((n, line)) = lines.get(*idx) else {
            return Err(err(ctx, "truncated arrival section"));
        };
        if line.is_empty() {
            break;
        }
        if is_dash_rule(line) {
            return Err(err(*n, "unexpected dash rule in arrival section"));
        }
        let (nums, desc) = tokenize(line, cols, *n, opts.time_unit_ns)?;
        classify_row(
            &nums,
            &desc,
            *n,
            opts.strict,
            &mut arrival,
            &mut pending_net,
            &mut prev,
            true,
        )?;
        if nums.time.is_some() {
            prev = nums.time;
        }
        *idx += 1;
    }
    skip_blanks(lines, idx);
    let mut required = RawSection::default();
    pending_net = None;
    prev = None;
    // Required rows run to the dash rule.
    loop {
        let Some((n, line)) = lines.get(*idx) else {
            return Err(err(ctx, "truncated required section"));
        };
        if is_dash_rule(line) {
            *idx += 1;
            break;
        }
        if line.is_empty() {
            *idx += 1;
            continue;
        }
        let (nums, desc) = tokenize(line, cols, *n, opts.time_unit_ns)?;
        classify_row(
            &nums,
            &desc,
            *n,
            opts.strict,
            &mut required,
            &mut pending_net,
            &mut prev,
            false,
        )?;
        if nums.time.is_some() {
            prev = nums.time;
        }
        *idx += 1;
    }
    // Footer: required, negated arrival, dash, slack.
    skip_blanks(lines, idx);
    let footer = |lines: &[(usize, String)],
                  idx: &mut usize|
     -> Result<(usize, RowNums, String), ParseError> {
        skip_blanks(lines, idx);
        let Some((n, line)) = lines.get(*idx) else {
            return Err(err(ctx, "truncated footer"));
        };
        // The footer shares the table columns.
        let (nums, desc) = tokenize(line, cols, *n, opts.time_unit_ns)?;
        *idx += 1;
        Ok((*n, nums, desc))
    };
    let (n, nums, desc) = footer(lines, idx)?;
    if desc != "data required time" {
        return Err(err(
            n,
            format!("expected footer required time, got '{desc}'"),
        ));
    }
    let footer_required = nums
        .time
        .ok_or_else(|| err(n, "footer required without Time"))?;
    let (n, nums, desc) = footer(lines, idx)?;
    if desc != "data arrival time" {
        return Err(err(
            n,
            format!("expected footer arrival time, got '{desc}'"),
        ));
    }
    let footer_arrival = nums
        .time
        .ok_or_else(|| err(n, "footer arrival without Time"))?;
    skip_blanks(lines, idx);
    let Some((n, line)) = lines.get(*idx) else {
        return Err(err(ctx, "truncated footer"));
    };
    if !is_dash_rule(line) {
        return Err(err(*n, "expected dash rule before slack"));
    }
    *idx += 1;
    skip_blanks(lines, idx);
    let Some((slack_n, slack_line)) = lines.get(*idx) else {
        return Err(err(ctx, "truncated footer, expected slack"));
    };
    let (nums, desc) = tokenize(slack_line, cols, *slack_n, opts.time_unit_ns)?;
    let met = if desc == "slack (MET)" {
        true
    } else if desc == "slack (VIOLATED)" {
        false
    } else {
        return Err(err(*slack_n, format!("expected slack line, got '{desc}'")));
    };
    let slack = nums
        .time
        .ok_or_else(|| err(*slack_n, "slack without Time"))?;
    let half_ulp_ns = half_ulp(slack_token(slack_line), opts.time_unit_ns);
    *idx += 1;

    if (slack >= 0.0) != met {
        return Err(err(*slack_n, "slack sign disagrees with MET/VIOLATED"));
    }

    let startpoint = strip_name(&start_raw).to_owned();
    let endpoint = strip_name(&end_raw).to_owned();
    let launch_clock = clocked_by(&start_raw).unwrap_or_default();
    let capture_clock = clocked_by(&end_raw).unwrap_or_default();
    let mut arrival_asm = assemble_section(
        &mut arrival,
        &launch_clock,
        half_ulp_ns,
        ctx,
        "launch",
        true,
        "data arrival time",
    )?;
    let mut required_asm = assemble_section(
        &mut required,
        &capture_clock,
        half_ulp_ns,
        ctx,
        "capture",
        false,
        "data required time",
    )?;
    let arrival_time = arrival_asm.side.end_time;
    let required_time = required_asm.side.end_time;
    let xtol = 2.0 * half_ulp_ns + 1e-9;
    if (footer_required - required_time).abs() > xtol {
        return Err(err(ctx, "footer required disagrees with table"));
    }
    if (footer_arrival + arrival_time).abs() > xtol {
        return Err(err(ctx, "footer arrival disagrees with table"));
    }
    let (margin, external) = match (required_asm.margin, required_asm.external) {
        (Some(m), None) => (Some(m), Some(0.0)),
        (None, Some(e)) => (Some(0.0), Some(e)),
        (None, None) => (None, None),
        (Some(_), Some(_)) => unreachable!("both-margin rejected in assembly"),
    };
    let launch_clock_name = arrival_asm
        .side
        .segment
        .as_ref()
        .map_or_else(|| launch_clock.clone(), |s| s.clock.clone());
    let capture_clock_name = required_asm
        .side
        .segment
        .as_ref()
        .map_or_else(|| capture_clock.clone(), |s| s.clock.clone());
    Ok(Some(TimingPath {
        startpoint,
        endpoint,
        launch_clock: launch_clock_name,
        capture_clock: capture_clock_name,
        launch_edge: arrival_asm
            .side
            .segment
            .as_ref()
            .map_or(Edge::Unknown, |s| s.edge),
        capture_edge: required_asm
            .side
            .segment
            .as_ref()
            .map_or(Edge::Unknown, |s| s.edge),
        group,
        flavor: Some(type_raw),
        check,
        launch: arrival_asm.side.segment.take(),
        capture: required_asm.side.segment.take(),
        slack,
        required: required_time,
        arrival: arrival_time,
        uncertainty: Some(required_asm.unc_sum),
        crpr: required_asm.crpr,
        margin,
        external_delay: external,
        rounding_ns: Some(half_ulp_ns),
        points: arrival_asm.data_points,
    }))
}

/// Parse plain-text report input, dispatching on sniffed layout (real STA
/// text or legacy OTPS).
///
/// # Errors
///
/// Returns [`ParseError`] on unrecognized/mixed layouts or malformed input.
pub fn parse_auto(input: &str, run: Run, opts: &ParseOpts) -> Result<TimingReport, ParseError> {
    match detect_text_kind(input)? {
        InputKind::StaText => parse_sta_text(input, run, opts),
        InputKind::OtpsLegacy => crate::parser::parse_text(input, run),
        InputKind::StaJson | InputKind::OtpsJson => Err(err(
            1,
            "internal error: JSON layout from text dispatch".to_owned(),
        )),
    }
}

/// Parse real `report_checks` text (possibly multi-block). Unconstrained
/// blocks are skipped and counted on the report.
///
/// # Errors
///
/// Returns [`ParseError`] with a 1-based line number on malformed input.
/// Cap on one buffered block: adversarial inputs cannot blow memory.
const BLOCK_CAP_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IterState {
    Outside,
    Inside,
    Discarding,
    Done,
}

/// Streaming per-path results over STA text. Memory is O(one block).
/// Malformed blocks yield `Err` and parsing resumes at the next `Startpoint:`;
/// unconstrained blocks are skipped and counted in [`PathIter::stats`].
pub struct PathIter<R> {
    reader: R,
    opts: ParseOpts,
    lineno: usize,
    buf: String,
    pending: Vec<(usize, String)>,
    pending_bytes: usize,
    prev_row: bool,
    state: IterState,
    stats: IterStats,
}

impl<R: std::io::BufRead> PathIter<R> {
    #[must_use]
    pub fn new(reader: R, opts: ParseOpts) -> Self {
        Self {
            reader,
            opts,
            lineno: 0,
            buf: String::new(),
            pending: Vec::new(),
            pending_bytes: 0,
            prev_row: false,
            state: IterState::Outside,
            stats: IterStats::default(),
        }
    }

    #[must_use]
    pub fn stats(&self) -> IterStats {
        self.stats
    }

    /// Push one raw line into the pending block, joining continuations.
    fn push_line(&mut self, line_no: usize, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            self.pending.push((line_no, String::new()));
            self.prev_row = false;
            return;
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        let net_row = trimmed.ends_with("(net)") && trimmed.split_whitespace().count() == 2;
        if self.prev_row && raw.starts_with(char::is_whitespace) && !is_num(first) && !net_row {
            if let Some((_, prev)) = self.pending.last_mut() {
                prev.push(' ');
                prev.push_str(trimmed);
                self.pending_bytes += trimmed.len() + 1;
            }
            return;
        }
        self.prev_row = is_num(first);
        self.pending_bytes += trimmed.len();
        self.pending.push((line_no, trimmed.to_owned()));
    }

    fn take_block(&mut self) -> Vec<(usize, String)> {
        self.pending_bytes = 0;
        self.prev_row = false;
        std::mem::take(&mut self.pending)
    }

    fn parse_pending(&mut self) -> Option<Result<TimingPath, ParseError>> {
        let block = self.take_block();
        self.stats.blocks += 1;
        let mut idx = 0;
        match parse_block(&block, &mut idx, &self.opts) {
            Ok(Some(path)) => Some(Ok(path)),
            Ok(None) => {
                self.stats.skipped += 1;
                None
            }
            Err(e) => {
                self.stats.errors += 1;
                Some(Err(e))
            }
        }
    }
}

impl<R: std::io::BufRead> Iterator for PathIter<R> {
    type Item = Result<TimingPath, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.state == IterState::Done {
            return None;
        }
        loop {
            self.buf.clear();
            let line_no = self.lineno + 1;
            match self.reader.read_line(&mut self.buf) {
                Err(e) => {
                    self.state = IterState::Done;
                    self.stats.errors += 1;
                    return Some(Err(err(line_no, format!("read error: {e}"))));
                }
                Ok(0) => {
                    let was_discarding = self.state == IterState::Discarding;
                    self.state = IterState::Done;
                    if was_discarding || self.pending.iter().all(|(_, l)| l.is_empty()) {
                        return None;
                    }
                    return self.parse_pending();
                }
                Ok(_) => {}
            }
            self.lineno = line_no;
            let trimmed = self.buf.trim().to_owned();
            if self.state == IterState::Discarding {
                if trimmed.starts_with("Startpoint:") {
                    self.state = IterState::Inside;
                    let raw = self.buf.clone();
                    self.push_line(line_no, &raw);
                }
                continue;
            }
            if trimmed.starts_with("Startpoint:") {
                if self.state == IterState::Inside {
                    let result = self.parse_pending();
                    let raw = self.buf.clone();
                    self.push_line(line_no, &raw);
                    if result.is_some() {
                        return result;
                    }
                    continue;
                }
                self.state = IterState::Inside;
                let raw = self.buf.clone();
                self.push_line(line_no, &raw);
                continue;
            }
            if self.state == IterState::Outside {
                if trimmed.is_empty() || is_banner(&trimmed) || trimmed.starts_with('#') {
                    continue;
                }
                self.state = IterState::Done;
                self.stats.errors += 1;
                return Some(Err(err(
                    line_no,
                    "expected Startpoint (aggregate formats like end/summary unsupported)",
                )));
            }
            if trimmed.starts_with('#') {
                self.prev_row = false;
                continue;
            }
            if self.pending_bytes > BLOCK_CAP_BYTES {
                self.take_block();
                self.state = IterState::Discarding;
                self.stats.errors += 1;
                return Some(Err(err(line_no, "block exceeds 1 MiB cap")));
            }
            let raw = self.buf.clone();
            self.push_line(line_no, &raw);
        }
    }
}

/// Peek at a text stream to dispatch on layout, returning the kind plus the
/// consumed bytes (chain them back before parsing).
///
/// # Errors
///
/// Returns an I/O error on read failure or unrecognized/mixed layout.
pub fn sniff_text_kind<R: std::io::BufRead>(
    reader: &mut R,
) -> Result<(InputKind, Vec<u8>), std::io::Error> {
    let mut buf = Vec::new();
    let mut lines = 0;
    while buf.len() < 16_384 && lines < 128 {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        lines += 1;
        buf.extend_from_slice(line.as_bytes());
    }
    let text = String::from_utf8_lossy(&buf);
    detect_text_kind(&text)
        .map(|kind| (kind, buf))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.message))
}

/// Parse real `report_checks` text (possibly multi-block). Unconstrained
/// blocks are skipped and counted on the report.
///
/// # Errors
///
/// Returns [`ParseError`] with a 1-based line number on malformed input.
pub fn parse_sta_text(input: &str, run: Run, opts: &ParseOpts) -> Result<TimingReport, ParseError> {
    let mut iter = PathIter::new(std::io::Cursor::new(input), opts.clone());
    let mut paths = Vec::new();
    for item in iter.by_ref() {
        paths.push(item?);
    }
    let mut report = TimingReport::new(run, paths);
    report.skipped_paths = iter.stats().skipped;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Severity;

    // All samples below are synthesized for these tests (own pin names and
    // values) in the real `report_checks` layout. No golden content.
    const MAX_BLOCK: &str =
        "Startpoint: u_core/ff1 (rising edge-triggered flip-flop clocked by clk)\n\
        Endpoint: u_core/ff2 (rising edge-triggered flip-flop clocked by clk)\n\
        Path Group: clk\n\
        Path Type: max\n\
        \n\
          Delay    Time   Description\n\
        ---------------------------------------------------\n\
           0.00    0.00   clock clk (rise edge)\n\
           0.00    0.00   clock source latency\n\
           0.01    0.01 ^ clkbuf0/Z (CLKBUF_X1)\n\
           0.00    0.01 ^ u_core/ff1/CK (DFF_X1)\n\
           0.64    0.65 ^ u_core/ff1/Q (DFF_X1)\n\
           0.20    0.85 ^ u_core/ff2/D (DFF_X1)\n\
                   0.85   data arrival time\n\
        \n\
           1.00    1.00   clock clk (rise edge)\n\
           0.00    1.00   clock network delay (ideal)\n\
          -0.05    0.95   clock uncertainty\n\
          -0.05    0.90   library setup time\n\
                   0.90   data required time\n\
        ---------------------------------------------------\n\
                   0.90   data required time\n\
                  -0.85   data arrival time\n\
        ---------------------------------------------------\n\
                   0.05   slack (MET)\n";

    const MIN_BLOCK: &str =
        "Startpoint: u_core/ff7 (rising edge-triggered flip-flop clocked by clk)\n\
        Endpoint: u_core/ff8 (rising edge-triggered flip-flop clocked by clk)\n\
        Path Group: clk\n\
        Path Type: min\n\
        \n\
          Delay    Time   Description\n\
        ---------------------------------------------------\n\
           0.00    0.00   clock clk (rise edge)\n\
           0.00    0.00   clock network delay (ideal)\n\
           0.00    0.00 v u_core/ff7/CK (DFF_X1)\n\
           0.05    0.05 v u_core/ff7/Q (DFF_X1)\n\
           0.02    0.07 v u_core/ff8/D (DFF_X1)\n\
                   0.07   data arrival time\n\
        \n\
           0.00    0.00   clock clk (rise edge)\n\
           0.00    0.00   clock network delay (ideal)\n\
           0.00    0.00   clock reconvergence pessimism\n\
           0.01    0.01   library hold time\n\
                   0.01   data required time\n\
        ---------------------------------------------------\n\
                   0.01   data required time\n\
                  -0.07   data arrival time\n\
        ---------------------------------------------------\n\
                   0.06   slack (MET)\n";

    fn parse(block: &str) -> TimingReport {
        parse_sta_text(block, Run::default(), &ParseOpts::default()).expect("parse")
    }

    #[test]
    fn parses_max_expanded_with_full_accounting() {
        let report = parse(MAX_BLOCK);
        assert_eq!(report.paths.len(), 1);
        let path = &report.paths[0];
        assert_eq!(path.check, Check::Setup);
        assert_eq!(path.startpoint, "u_core/ff1");
        assert_eq!(path.group, "clk");
        assert_eq!(path.flavor.as_deref(), Some("max"));
        let launch = path.launch.as_ref().expect("launch");
        assert!(!launch.condensed);
        assert_eq!(launch.stages.len(), 1);
        assert!((launch.network_delay - 0.01).abs() < 1e-12);
        assert_eq!(path.points.len(), 3, "CK/Q/D are the data stages");
        assert!((path.points[1].cell_delay - 0.64).abs() < 1e-12);
        assert!((path.points[2].net_delay - 0.20).abs() < 1e-12);
        assert!(path.points[2].split_available);
        let capture = path.capture.as_ref().expect("capture");
        assert!(capture.condensed);
        assert_eq!(path.uncertainty, Some(-0.05));
        assert_eq!(path.margin, Some(-0.05));
        assert_eq!(path.crpr, None);
        assert_eq!(path.validate(1e-9).severity, Severity::Clean);
    }

    #[test]
    fn parses_min_hold_with_fall_markers() {
        let path = &parse(MIN_BLOCK).paths[0];
        assert_eq!(path.check, Check::Hold);
        assert!((path.slack - 0.06).abs() < 1e-12);
        assert!(path
            .points
            .iter()
            .all(|p| p.transition == Some(Transition::Fall)));
        assert_eq!(path.validate(1e-9).severity, Severity::Clean);
    }

    #[test]
    fn folded_driver_span_marks_split_unavailable() {
        // Default text without input-pin rows: buf/Z span covers net+cell.
        // NOTE: `\`-continuations strip leading indentation, so patterns
        // below carry no leading spaces.
        let block = MAX_BLOCK.replace(
            "0.64    0.65 ^ u_core/ff1/Q (DFF_X1)\n",
            "0.64    0.65 ^ u_core/ff1/Q (DFF_X1)\n0.02    0.67 ^ ubuf/Z (BUF_X1)\n",
        );
        // Arrival chain no longer adds up; fix the downstream times.
        let block = block
            .replace(
                "0.20    0.85 ^ u_core/ff2/D (DFF_X1)\n",
                "0.20    0.87 ^ u_core/ff2/D (DFF_X1)\n",
            )
            .replace("0.85   data arrival time\n", "0.87   data arrival time\n")
            .replace("-0.85   data arrival time\n", "-0.87   data arrival time\n")
            .replace("0.05   slack (MET)\n", "0.03   slack (MET)\n");
        let path = &parse(&block).paths[0];
        let folded = path
            .points
            .iter()
            .find(|p| p.pin == "ubuf/Z")
            .expect("ubuf/Z");
        assert!(!folded.split_available);
        assert!((folded.cell_delay - 0.02).abs() < 1e-12);
        assert_eq!(path.validate(1e-9).severity, Severity::Clean);
    }

    #[test]
    fn input_pin_rows_split_net_and_cell() {
        // With input-pin rows, A carries the net arc and Z the cell arc.
        let block = MAX_BLOCK.replace(
            "0.64    0.65 ^ u_core/ff1/Q (DFF_X1)\n",
            "0.04    0.05 ^ ubuf/A (BUF_X1)\n0.60    0.65 ^ ubuf/Z (BUF_X1)\n",
        );
        let path = &parse(&block).paths[0];
        let a = path
            .points
            .iter()
            .find(|p| p.pin == "ubuf/A")
            .expect("ubuf/A");
        let z = path
            .points
            .iter()
            .find(|p| p.pin == "ubuf/Z")
            .expect("ubuf/Z");
        assert!((a.net_delay - 0.04).abs() < 1e-12 && a.cell_delay == 0.0);
        assert!((z.cell_delay - 0.60).abs() < 1e-12 && z.net_delay == 0.0);
    }

    #[test]
    fn net_rows_attach_and_fanout_parses() {
        let block = MAX_BLOCK
            .replace(
                "Delay    Time   Description\n",
                "Fanout   Delay    Time   Description\n",
            )
            .replace(
                "0.64    0.65 ^ u_core/ff1/Q (DFF_X1)\n",
                "2    0.64    0.65 ^ u_core/ff1/Q (DFF_X1)\nnn1 (net)\n",
            );
        let path = &parse(&block).paths[0];
        let q = path
            .points
            .iter()
            .find(|p| p.pin == "u_core/ff1/Q")
            .expect("Q");
        assert_eq!(q.fanout, 2);
        let d = path
            .points
            .iter()
            .find(|p| p.pin == "u_core/ff2/D")
            .expect("D");
        assert_eq!(d.net, "nn1");
    }

    #[test]
    fn unconstrained_blocks_skip_and_count() {
        let block = "Startpoint: u_core/ff1 (rising edge-triggered flip-flop clocked by clk)\n\
            Endpoint: u_core/ff2 (rising edge-triggered flip-flop clocked by clk)\n\
            Path Group: clk\n\
            Path Type: max\n\
            (Path is unconstrained)\n";
        let report = parse(block);
        assert_eq!(report.paths, Vec::new());
        assert_eq!(report.skipped_paths, 1);
    }

    #[test]
    fn rejects_malformed_blocks() {
        // Short format: headers but no table.
        let short = "Startpoint: a (x)\nEndpoint: b (y)\nPath Group: g\nPath Type: max\n";
        let err = parse_sta_text(short, Run::default(), &ParseOpts::default()).expect_err("short");
        assert!(err.message.contains("short format"), "{}", err.message);
        // Unsupported Path Type.
        let bad_type = MAX_BLOCK.replace("Path Type: max", "Path Type: pulse_width");
        let err =
            parse_sta_text(&bad_type, Run::default(), &ParseOpts::default()).expect_err("type");
        assert!(
            err.message.contains("unsupported Path Type"),
            "{}",
            err.message
        );
        // Latch borrowing rows.
        let borrow = MAX_BLOCK.replace(
            "-0.05    0.90   library setup time\n",
            "-0.05    0.90   library setup time\n0.02    0.92   time borrowed from endpoint\n",
        );
        assert!(parse_sta_text(&borrow, Run::default(), &ParseOpts::default()).is_err());
        // Footer mismatch.
        let bad_footer =
            MAX_BLOCK.replace("-0.85   data arrival time", "-0.50   data arrival time");
        assert!(parse_sta_text(&bad_footer, Run::default(), &ParseOpts::default()).is_err());
        // Mixed clock form: scalar plus expanded pins (chain kept consistent).
        let mixed = MAX_BLOCK
            .replace(
                "0.01    0.01 ^ clkbuf0/Z (CLKBUF_X1)\n",
                "0.05    0.05   clock network delay (propagated)\n0.01    0.06 ^ clkbuf0/Z (CLKBUF_X1)\n",
            )
            .replace("0.00    0.01 ^ u_core/ff1/CK (DFF_X1)\n", "0.00    0.06 ^ u_core/ff1/CK (DFF_X1)\n")
            .replace("0.64    0.65 ^ u_core/ff1/Q (DFF_X1)\n", "0.64    0.70 ^ u_core/ff1/Q (DFF_X1)\n")
            .replace("0.20    0.85 ^ u_core/ff2/D (DFF_X1)\n", "0.20    0.90 ^ u_core/ff2/D (DFF_X1)\n")
            .replace("0.85   data arrival time\n", "0.90   data arrival time\n")
            .replace("-0.85   data arrival time\n", "-0.90   data arrival time\n")
            .replace("0.05   slack (MET)\n", "0.00   slack (MET)\n");
        let err = parse_sta_text(&mixed, Run::default(), &ParseOpts::default()).expect_err("mixed");
        assert!(err.message.contains("mixed"), "{}", err.message);
        // Cap columns need positional parsing (future work).
        let cap = MAX_BLOCK.replace(
            "Delay    Time   Description\n",
            "Delay    Time   Cap   Description\n",
        );
        let err = parse_sta_text(&cap, Run::default(), &ParseOpts::default()).expect_err("cap");
        assert!(
            err.message.contains("unsupported table column"),
            "{}",
            err.message
        );
    }

    #[test]
    fn strict_rejects_extra_tokens() {
        let extra = MAX_BLOCK.replace(
            "u_core/ff1/Q (DFF_X1)\n",
            "u_core/ff1/Q (DFF_X1) somegarbage\n",
        );
        assert_eq!(parse(&extra).paths.len(), 1);
        let strict = ParseOpts {
            strict: true,
            ..ParseOpts::default()
        };
        assert!(parse_sta_text(&extra, Run::default(), &strict).is_err());
    }

    #[test]
    fn met_violated_mismatch_rejected() {
        let bad = MAX_BLOCK.replace("0.05   slack (MET)", "0.05   slack (VIOLATED)");
        let err = parse_sta_text(&bad, Run::default(), &ParseOpts::default()).expect_err("met");
        assert!(err.message.contains("MET/VIOLATED"), "{}", err.message);
    }

    #[test]
    fn detects_layouts() {
        assert_eq!(
            detect_text_kind("Startpoint: a\nLaunch clock: clk\n").expect("legacy"),
            InputKind::OtpsLegacy
        );
        assert_eq!(
            detect_text_kind(MAX_BLOCK).expect("sta"),
            InputKind::StaText
        );
        assert!(detect_text_kind("Startpoint: a\nLaunch clock: c\nPath Group: g\n").is_err());
        assert!(detect_text_kind("hello world\n").is_err());
    }

    #[test]
    fn iter_recovers_after_bad_block() {
        let bad = MAX_BLOCK.replace("0.05   slack (MET)", "oops not a footer");
        let two_good = format!("{MAX_BLOCK}\n{MAX_BLOCK}");
        let mixed = format!("{MAX_BLOCK}\n{bad}\n{two_good}");
        let items: Vec<_> =
            PathIter::new(std::io::Cursor::new(&mixed), ParseOpts::default()).collect();
        assert_eq!(items.len(), 4);
        assert!(items[0].is_ok());
        assert!(items[1].is_err());
        assert!(items[2].is_ok());
        assert!(items[3].is_ok());
    }

    #[test]
    fn iter_stats_count_blocks_and_skips() {
        let unconstrained =
            "Startpoint: u_core/ff9 (rising edge-triggered flip-flop clocked by clk)\n\
            Endpoint: u_core/ff10 (rising edge-triggered flip-flop clocked by clk)\n\
            Path Group: clk\n\
            Path Type: max\n\
            (Path is unconstrained)\n";
        let mixed = format!("{MAX_BLOCK}\n{unconstrained}");
        let mut iter = PathIter::new(std::io::Cursor::new(&mixed), ParseOpts::default());
        let items: Vec<_> = iter.by_ref().collect();
        assert_eq!(items.len(), 1);
        assert_eq!(
            iter.stats(),
            IterStats {
                blocks: 2,
                skipped: 1,
                errors: 0
            }
        );
    }

    #[test]
    fn iter_rejects_oversize_block() {
        let big_line = "x".repeat(1 << 20);
        let input = format!("Startpoint: a\n{big_line}\njunk tail\n{MAX_BLOCK}");
        let items: Vec<_> =
            PathIter::new(std::io::Cursor::new(&input), ParseOpts::default()).collect();
        assert_eq!(items.len(), 2);
        assert!(items[0]
            .as_ref()
            .expect_err("cap")
            .message
            .contains("1 MiB"));
        assert!(items[1].is_ok());
    }

    #[test]
    fn sniff_dispatches_and_roundtrips() {
        use std::io::Read as _;
        let mut cursor = std::io::Cursor::new(MAX_BLOCK.as_bytes().to_vec());
        let (kind, peek) = sniff_text_kind(&mut cursor).expect("sniff");
        assert_eq!(kind, InputKind::StaText);
        let mut chained = std::io::BufReader::new(std::io::Cursor::new(peek).chain(cursor));
        let mut rest = String::new();
        chained.read_to_string(&mut rest).expect("chain");
        assert_eq!(rest, MAX_BLOCK);
    }
}

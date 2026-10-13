//! Parsers: legacy OTPS text format v0.2 plus a JSON report importer.
//!
//! v0.2 text carries clock segments, per-check slack, and signed/optional
//! required-side terms. v0.1 layouts are rejected with a migration pointer.
//! The text parser is hand-rolled (no regex) line parsing over `&str`.

use std::fmt;

use serde::de::Error as _;

use crate::schema::{
    Check, ClockSegment, Edge, Run, TimingPath, TimingPoint, TimingReport, Transition,
    SCHEMA_VERSION,
};

/// A parse failure with its 1-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

type Result<T> = std::result::Result<T, ParseError>;

pub(crate) fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

fn field<'a>(line: &'a str, prefix: &str, lineno: usize) -> Result<&'a str> {
    line.strip_prefix(prefix)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| err(lineno, format!("expected '{prefix}' value")))
}

fn number(line: &str, prefix: &str, lineno: usize) -> Result<f64> {
    let raw = field(line, prefix, lineno)?;
    parse_f64(raw, lineno).map_err(|_| err(lineno, format!("bad number in '{prefix}'")))
}

/// A number header that also accepts `unknown`/`none` for `None` terms.
fn opt_number(line: &str, prefix: &str, lineno: usize) -> Result<Option<f64>> {
    let raw = field(line, prefix, lineno)?;
    if raw.eq_ignore_ascii_case("unknown") || raw.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    parse_f64(raw, lineno)
        .map(Some)
        .map_err(|_| err(lineno, format!("bad number in '{prefix}'")))
}

fn parse_check(raw: &str, lineno: usize) -> Result<Check> {
    match raw.to_ascii_lowercase().as_str() {
        "setup" | "max" => Ok(Check::Setup),
        "hold" | "min" => Ok(Check::Hold),
        other => Err(err(lineno, format!("bad check type '{other}'"))),
    }
}

fn parse_edge(raw: &str, lineno: usize) -> Result<Edge> {
    match raw.to_ascii_lowercase().as_str() {
        "rise" => Ok(Edge::Rise),
        "fall" => Ok(Edge::Fall),
        "unknown" => Ok(Edge::Unknown),
        other => Err(err(lineno, format!("bad edge '{other}'"))),
    }
}

fn parse_transition(raw: &str, lineno: usize) -> Result<Transition> {
    match raw.to_ascii_lowercase().as_str() {
        "rise" => Ok(Transition::Rise),
        "fall" => Ok(Transition::Fall),
        "unknown" => Ok(Transition::Unknown),
        other => Err(err(lineno, format!("bad transition '{other}'"))),
    }
}

fn parse_f64(raw: &str, lineno: usize) -> Result<f64> {
    let value: f64 = raw
        .parse::<f64>()
        .map_err(|_| err(lineno, format!("bad number '{raw}'")))?;
    if !value.is_finite() {
        return Err(err(lineno, format!("non-finite number '{raw}'")));
    }
    Ok(value)
}

fn parse_point(line: &str, lineno: usize) -> Result<TimingPoint> {
    let mut cols = line.split('|');
    let pin = cols
        .next()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| err(lineno, "point needs a pin name"))?;
    let cell = cols.next().map(str::trim).unwrap_or_default();
    let net = cols.next().map(str::trim).unwrap_or_default();
    let attrs = cols.next().unwrap_or_default();
    if cols.next().is_some() {
        return Err(err(lineno, "unexpected column in point line"));
    }

    let mut point = TimingPoint {
        pin: pin.to_owned(),
        cell: cell.to_owned(),
        net: net.to_owned(),
        cell_delay: 0.0,
        net_delay: 0.0,
        slew: 0.0,
        cap: 0.0,
        fanout: 0,
        derate: None,
        sigma: None,
        si_delta: None,
        transition: None,
        arrival: None,
        split_available: true,
    };
    for attr in attrs.split_whitespace() {
        let (key, value) = attr
            .split_once('=')
            .ok_or_else(|| err(lineno, format!("bad attribute '{attr}'")))?;
        match key {
            "cell" => point.cell_delay = parse_f64(value, lineno)?,
            "net" => point.net_delay = parse_f64(value, lineno)?,
            "slew" => point.slew = parse_f64(value, lineno)?,
            "cap" => point.cap = parse_f64(value, lineno)?,
            "fanout" => {
                point.fanout = value
                    .parse::<u32>()
                    .map_err(|_| err(lineno, format!("bad fanout '{value}'")))?;
            }
            "derate" => point.derate = Some(parse_f64(value, lineno)?),
            "sigma" => point.sigma = Some(parse_f64(value, lineno)?),
            "si" => point.si_delta = Some(parse_f64(value, lineno)?),
            "trans" => point.transition = Some(parse_transition(value, lineno)?),
            "arrival" => point.arrival = Some(parse_f64(value, lineno)?),
            "split" => {
                point.split_available = match value {
                    "yes" => true,
                    "no" => false,
                    _ => return Err(err(lineno, format!("bad split '{value}'"))),
                }
            }
            other => return Err(err(lineno, format!("unknown attribute '{other}'"))),
        }
    }
    Ok(point)
}

pub(crate) type Line<'a> = (usize, &'a str);

pub(crate) fn next_line<'a>(lines: &[Line<'a>], idx: &mut usize, ctx: usize) -> Result<Line<'a>> {
    if *idx >= lines.len() {
        return Err(err(ctx, "truncated path"));
    }
    let out = lines[*idx];
    *idx += 1;
    Ok(out)
}

fn header(
    lines: &[Line<'_>],
    idx: &mut usize,
    ctx: usize,
    prefix: &str,
) -> Result<(usize, String)> {
    let (n, v) = next_line(lines, idx, ctx)?;
    if !v.starts_with(prefix) {
        // Friendly pointer for pre-0.2 layouts: v0.1 has Group/Check/Slack
        // where v0.2 has Launch edge.
        if prefix == "Launch edge:" && (v.starts_with("Group:") || v.starts_with("Slack:")) {
            return Err(err(
                n,
                "v0.1 report layout; migrate to v0.2 (see docs/format.html)",
            ));
        }
        return Err(err(n, format!("expected '{prefix}'")));
    }
    Ok((n, v.to_owned()))
}

/// Reads the `"<side> edge time|srclat|net"` header triple.
fn clock_numbers(
    lines: &[Line<'_>],
    idx: &mut usize,
    ctx: usize,
    side: &str,
) -> Result<(Option<f64>, f64, f64)> {
    let edge_time_h = format!("{side} edge time:");
    let srclat_h = format!("{side} srclat:");
    let net_h = format!("{side} net:");
    let (n, v) = header(lines, idx, ctx, &edge_time_h)?;
    let edge_time = opt_number(&v, &edge_time_h, n)?;
    let (n, v) = header(lines, idx, ctx, &srclat_h)?;
    let srclat = number(&v, &srclat_h, n)?;
    let (n, v) = header(lines, idx, ctx, &net_h)?;
    let net = number(&v, &net_h, n)?;
    Ok((edge_time, srclat, net))
}

fn point_section(
    lines: &[Line<'_>],
    idx: &mut usize,
    ctx: usize,
    marker: &str,
) -> Result<Vec<TimingPoint>> {
    let (n, v) = next_line(lines, idx, ctx)?;
    if v != marker {
        return Err(err(n, format!("expected '{marker}'")));
    }
    let mut points = Vec::new();
    loop {
        let (n, v) = next_line(lines, idx, ctx)?;
        if v == "End" {
            break;
        }
        points.push(parse_point(v, n)?);
    }
    Ok(points)
}

struct ClockHeaders {
    clock: String,
    edge: Edge,
    edge_time: Option<f64>,
    source_latency: f64,
    network_delay: f64,
}

fn clock_segment(
    headers: ClockHeaders,
    stages: Vec<TimingPoint>,
    lineno: usize,
    side: &str,
) -> Result<ClockSegment> {
    let condensed = stages.is_empty();
    if !condensed {
        let sum: f64 = stages.iter().map(TimingPoint::incr).sum();
        if (sum - headers.network_delay).abs() > 1e-9 {
            return Err(err(
                lineno,
                format!(
                    "{side} network inconsistent: stages={sum:.6} scalar={:.6}",
                    headers.network_delay
                ),
            ));
        }
    }
    Ok(ClockSegment {
        clock: headers.clock,
        edge: headers.edge,
        edge_time: headers.edge_time,
        source_latency: headers.source_latency,
        network_delay: headers.network_delay,
        macro_delay: 0.0,
        condensed,
        stages,
    })
}

fn parse_path(lines: &[Line<'_>], idx: &mut usize) -> Result<TimingPath> {
    let (start_n, start_l) = next_line(lines, idx, 1)?;
    if !start_l.starts_with("Startpoint:") {
        return Err(err(start_n, "expected 'Startpoint:'"));
    }
    let startpoint = field(start_l, "Startpoint:", start_n)?.to_owned();
    let ctx = start_n;

    let (n, v) = header(lines, idx, ctx, "Endpoint:")?;
    let endpoint = field(&v, "Endpoint:", n)?.to_owned();
    let (n, v) = header(lines, idx, ctx, "Launch clock:")?;
    let launch_clock = field(&v, "Launch clock:", n)?.to_owned();
    let (n, v) = header(lines, idx, ctx, "Capture clock:")?;
    let capture_clock = field(&v, "Capture clock:", n)?.to_owned();
    let (n, v) = header(lines, idx, ctx, "Launch edge:")?;
    let launch_edge = parse_edge(field(&v, "Launch edge:", n)?, n)?;
    let (n, v) = header(lines, idx, ctx, "Capture edge:")?;
    let capture_edge = parse_edge(field(&v, "Capture edge:", n)?, n)?;
    let (_, v) = header(lines, idx, ctx, "Group:")?;
    let group = v
        .strip_prefix("Group:")
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let (n, v) = header(lines, idx, ctx, "Check:")?;
    let check = parse_check(field(&v, "Check:", n)?, n)?;
    let (n, v) = header(lines, idx, ctx, "Slack:")?;
    let slack = number(&v, "Slack:", n)?;
    let (n, v) = header(lines, idx, ctx, "Required:")?;
    let required = number(&v, "Required:", n)?;
    let (n, v) = header(lines, idx, ctx, "Arrival:")?;
    let arrival = number(&v, "Arrival:", n)?;
    let (launch_edge_time, launch_srclat, launch_net) = clock_numbers(lines, idx, ctx, "Launch")?;
    let (capture_edge_time, capture_srclat, capture_net) =
        clock_numbers(lines, idx, ctx, "Capture")?;
    let (n, v) = header(lines, idx, ctx, "Uncertainty:")?;
    let uncertainty = opt_number(&v, "Uncertainty:", n)?;
    let (n, v) = header(lines, idx, ctx, "Crpr:")?;
    let crpr = opt_number(&v, "Crpr:", n)?;
    let (n, v) = header(lines, idx, ctx, "Margin:")?;
    let margin = opt_number(&v, "Margin:", n)?;
    let (n, v) = header(lines, idx, ctx, "External:")?;
    let external_delay = opt_number(&v, "External:", n)?;

    // Optional clock sections precede the data section.
    let mut launch_stages = Vec::new();
    let mut capture_stages = Vec::new();
    if *idx < lines.len() && lines[*idx].1 == "Launch clock points:" {
        launch_stages = point_section(lines, idx, ctx, "Launch clock points:")?;
    }
    if *idx < lines.len() && lines[*idx].1 == "Capture clock points:" {
        capture_stages = point_section(lines, idx, ctx, "Capture clock points:")?;
    }
    let points = point_section(lines, idx, ctx, "Points:")?;

    let launch = clock_segment(
        ClockHeaders {
            clock: launch_clock.clone(),
            edge: launch_edge,
            edge_time: launch_edge_time,
            source_latency: launch_srclat,
            network_delay: launch_net,
        },
        launch_stages,
        n,
        "launch",
    )?;
    let capture = clock_segment(
        ClockHeaders {
            clock: capture_clock.clone(),
            edge: capture_edge,
            edge_time: capture_edge_time,
            source_latency: capture_srclat,
            network_delay: capture_net,
        },
        capture_stages,
        n,
        "capture",
    )?;

    Ok(TimingPath {
        startpoint,
        endpoint,
        launch_clock,
        capture_clock,
        launch_edge,
        capture_edge,
        group,
        flavor: None,
        check,
        launch: Some(launch),
        capture: Some(capture),
        slack,
        required,
        arrival,
        uncertainty,
        crpr,
        margin,
        external_delay,
        rounding_ns: None,
        points,
    })
}

/// Parse one legacy OTPS text report (v0.2 grammar).
/// `run` describes the run the text came from.
///
/// # Errors
///
/// Returns [`ParseError`] with a 1-based line number when the input is not a
/// well-formed v0.2 report. v0.1 layouts are rejected with a migration pointer.
pub fn parse_text(input: &str, run: Run) -> Result<TimingReport> {
    let lines: Vec<Line<'_>> = input
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let mut idx = 0;
    let mut paths = Vec::new();
    while idx < lines.len() {
        paths.push(parse_path(&lines, &mut idx)?);
    }
    Ok(TimingReport::new(run, paths))
}

/// Import a report previously exported as JSON (in-tool exporter dumps).
/// The document's `schema_version` must match this build's version.
///
/// # Errors
///
/// Returns the `serde_json` error when the input is not a valid report, or a
/// version-mismatch error when the document targets another schema version.
pub fn parse_json(input: &str) -> serde_json::Result<TimingReport> {
    let report: TimingReport = serde_json::from_str(input)?;
    if report.schema_version != SCHEMA_VERSION {
        return Err(serde_json::Error::custom(format!(
            "schema version {} != {}",
            report.schema_version, SCHEMA_VERSION
        )));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# sample\n\
        Startpoint: u_core/ff1\n\
        Endpoint: u_core/ff2\n\
        Launch clock: clk\n\
        Capture clock: clk\n\
        Launch edge: rise\n\
        Capture edge: rise\n\
        Group: clk\n\
        Check: setup\n\
        Slack: 0.100\n\
        Required: 1.000\n\
        Arrival: 0.900\n\
        Launch edge time: 0.000\n\
        Launch srclat: 0.000\n\
        Launch net: 0.000\n\
        Capture edge time: 1.000\n\
        Capture srclat: 0.000\n\
        Capture net: 0.000\n\
        Uncertainty: 0.000\n\
        Crpr: none\n\
        Margin: 0.000\n\
        External: none\n\
        Points:\n\
        u_core/ff1/CK | DFF | net_clk | cell=0.050 net=0.010 slew=0.020 cap=0.003 fanout=2 trans=rise\n\
        u_core/ff2/D | DFF | net_d | cell=0.640 net=0.200 slew=0.040 cap=0.005 fanout=1 arrival=0.900\n\
        End\n";

    const V01_SAMPLE: &str = "Startpoint: a\n\
        Endpoint: b\n\
        Launch clock: clk\n\
        Capture clock: clk\n\
        Group: g\n\
        Check: setup\n\
        Slack: 0.1\n";

    #[test]
    fn parses_sample_and_validates() {
        let report = parse_text(SAMPLE, Run::default()).expect("parse");
        assert_eq!(report.paths.len(), 1);
        let path = &report.paths[0];
        assert_eq!(path.check, Check::Setup);
        assert_eq!(path.points.len(), 2);
        assert_eq!(path.launch_edge, Edge::Rise);
        assert!(path.launch.as_ref().expect("launch").condensed);
        assert_eq!(
            path.points[0].transition,
            Some(Transition::Rise),
            "trans attr"
        );
        assert_eq!(path.validate(1e-9).severity, crate::schema::Severity::Clean);
    }

    #[test]
    fn rejects_v01_layout_with_pointer() {
        let err = parse_text(V01_SAMPLE, Run::default()).expect_err("v0.1");
        assert!(err.message.contains("v0.1"), "{}", err.message);
    }

    #[test]
    fn rejects_truncated_path() {
        let bad = "Startpoint: a\nEndpoint: b\n";
        assert!(parse_text(bad, Run::default()).is_err());
    }

    #[test]
    fn rejects_non_finite_numbers() {
        for bad in ["NaN", "inf", "-inf"] {
            let input = SAMPLE.replace("Slack: 0.100", &format!("Slack: {bad}"));
            assert!(parse_text(&input, Run::default()).is_err(), "{bad} slack");
            let input = SAMPLE.replace("cell=0.050", &format!("cell={bad}"));
            assert!(parse_text(&input, Run::default()).is_err(), "{bad} cell");
        }
    }

    #[test]
    fn rejects_extra_point_column() {
        let input = SAMPLE.replace("fanout=2 trans", "fanout=2 | garbage trans");
        assert!(parse_text(&input, Run::default()).is_err());
    }

    #[test]
    fn json_round_trip() {
        let report = parse_text(SAMPLE, Run::default()).expect("parse");
        let json = serde_json::to_string(&report).expect("json");
        let back: TimingReport = parse_json(&json).expect("reparse");
        assert_eq!(report, back);
    }

    #[test]
    fn json_rejects_other_schema_versions() {
        let report = parse_text(SAMPLE, Run::default()).expect("parse");
        let json = serde_json::to_string(&report).expect("json");
        let old = json.replace("\"0.2.0\"", "\"0.1.0\"");
        assert!(parse_json(&old).is_err());
    }
}

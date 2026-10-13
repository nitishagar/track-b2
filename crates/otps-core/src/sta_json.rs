//! Native `report_checks -format json` ingest (`{"checks":[...]}`).
//!
//! Grammar (verified against `OpenSTA` goldens + `ReportPath.cc`, read-only):
//! per-check `type`, `path_group`, `path_type`, `startpoint`/`endpoint`,
//! clock keys + pin arrays with cumulative `arrival`, scalars
//! `data_arrival_time`/`crpr`/`margin`/`required_time`/`slack`. All times in
//! SECONDS (`{:.3e}`); capacitance in farads; slew in seconds.
//!
//! Sign rules (B4 ground truth): text and JSON agree on CRPR and on hold
//! margin, but JSON `margin` is raw `margin()` (+setup) while text shows the
//! signed contribution (-setup, see `reportRequired`). The schema stores the
//! required-offset contribution, so setup checks negate the JSON key at
//! ingest. `output_delay` checks route it to `external_delay` instead.
//! Uncertainty has no JSON key and stays `None`.

use serde::Deserialize;

use crate::parser::{err, ParseError};
use crate::schema::{Check, ClockSegment, Edge, Run, TimingPath, TimingPoint, TimingReport};
use crate::sta::{InputKind, IterStats, StreamStats};

impl<R: std::io::BufRead> StreamStats for JsonPathIter<R> {
    fn stats(&self) -> IterStats {
        self.stats
    }
}

const SEC_TO_NS: f64 = 1e9;
const FARAD_TO_FF: f64 = 1e15;
/// `{:.3e}` relative half-ULP, scaled per path by max(|arrival|, |required|).
const JSON_HALF_ULP: f64 = 5e-4;
/// Cap on one framed check: adversarial inputs cannot blow memory.
const ELEMENT_CAP_BYTES: usize = 1 << 20;

#[derive(Debug, Deserialize)]
struct RawPin {
    pin: Option<String>,
    net: Option<String>,
    cell: Option<String>,
    instance: Option<String>,
    arrival: Option<f64>,
    slew: Option<f64>,
    capacitance: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawCheck {
    #[serde(rename = "type")]
    kind: Option<String>,
    path_group: Option<String>,
    path_type: Option<String>,
    startpoint: Option<String>,
    endpoint: Option<String>,
    source_clock: Option<String>,
    source_clock_edge: Option<String>,
    target_clock: Option<String>,
    target_clock_edge: Option<String>,
    source_clock_path: Option<Vec<RawPin>>,
    source_path: Option<Vec<RawPin>>,
    target_clock_path: Option<Vec<RawPin>>,
    data_arrival_time: Option<f64>,
    required_time: Option<f64>,
    slack: Option<f64>,
    crpr: Option<f64>,
    margin: Option<f64>,
    path_margin: Option<f64>,
    path_delay: Option<f64>,
}

fn parse_edge(raw: Option<&str>) -> Edge {
    if raw.is_some_and(|s| s.eq_ignore_ascii_case("rise")) {
        Edge::Rise
    } else if raw.is_some_and(|s| s.eq_ignore_ascii_case("fall")) {
        Edge::Fall
    } else {
        Edge::Unknown
    }
}

/// Stage-grouping key: the leaf instance when known, else the pin's parent
/// path. Consecutive pins with equal keys form a cell arc.
fn stage_key<'a>(pin: &'a RawPin, name: &'a str) -> &'a str {
    match pin.instance.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => name.rsplit_once('/').map_or("", |(parent, _)| parent),
    }
}

/// Difference cumulative arrivals into stage points. `reference_ns` is the
/// arrival the first pin hangs from (`None` = unknown: the first point
/// carries its total arrival with `split_available=false`).
fn difference_pins(
    pins: &[RawPin],
    reference_ns: Option<f64>,
    line: usize,
    what: &str,
) -> Result<Vec<TimingPoint>, ParseError> {
    let mut points = Vec::with_capacity(pins.len());
    let mut prev_arr = reference_ns.unwrap_or(0.0);
    let mut prev_key = String::new();
    for (i, pin) in pins.iter().enumerate() {
        let name = pin
            .pin
            .clone()
            .ok_or_else(|| err(line, format!("{what} pin without name")))?;
        let arrival = pin
            .arrival
            .map(|a| a * SEC_TO_NS)
            .ok_or_else(|| err(line, format!("pin {name} without arrival")))?;
        let key = stage_key(pin, &name);
        let (cell_delay, net_delay, split) = if i == 0 {
            match reference_ns {
                Some(_) => (arrival - prev_arr, 0.0, true),
                None => (arrival, 0.0, false),
            }
        } else if key == prev_key {
            (arrival - prev_arr, 0.0, true)
        } else {
            (0.0, arrival - prev_arr, true)
        };
        key.clone_into(&mut prev_key);
        points.push(TimingPoint {
            pin: name,
            cell: pin.cell.clone().unwrap_or_default(),
            net: pin.net.clone().unwrap_or_default(),
            cell_delay,
            net_delay,
            slew: pin.slew.map_or(0.0, |s| s * SEC_TO_NS),
            cap: pin.capacitance.map_or(0.0, |f| f * FARAD_TO_FF),
            fanout: 0,
            derate: None,
            sigma: None,
            si_delta: None,
            transition: None,
            arrival: Some(arrival),
            split_available: split,
        });
        prev_arr = arrival;
    }
    Ok(points)
}

fn convert_segment(
    clock: Option<&str>,
    edge: Edge,
    pins: Option<&[RawPin]>,
    line: usize,
    side: &str,
) -> Result<Option<ClockSegment>, ParseError> {
    if clock.is_none() && pins.is_none() {
        return Ok(None);
    }
    let empty: &[RawPin] = &[];
    let pins = pins.unwrap_or(empty);
    let edge_time = pins.first().and_then(|p| p.arrival).map(|a| a * SEC_TO_NS);
    let stages = difference_pins(pins, edge_time, line, side)?;
    let network_delay = stages.iter().map(TimingPoint::incr).sum();
    Ok(Some(ClockSegment {
        clock: clock.unwrap_or("unknown").to_owned(),
        edge,
        edge_time,
        source_latency: 0.0,
        network_delay,
        macro_delay: 0.0,
        condensed: stages.is_empty(),
        stages,
    }))
}

/// Check type + startpoint/endpoint/group: all required for parsed checks.
fn check_identity(
    raw: &RawCheck,
    line: usize,
) -> Result<(Check, String, String, String), ParseError> {
    let check = match raw.path_type.as_deref() {
        Some("max") => Check::Setup,
        Some("min") => Check::Hold,
        other => {
            return Err(err(
                line,
                format!("missing or unsupported path_type {other:?}"),
            ));
        }
    };
    let startpoint = raw
        .startpoint
        .clone()
        .ok_or_else(|| err(line, "check without startpoint"))?;
    let endpoint = raw
        .endpoint
        .clone()
        .ok_or_else(|| err(line, "check without endpoint"))?;
    let group = raw
        .path_group
        .clone()
        .ok_or_else(|| err(line, "check without path_group"))?;
    Ok((check, startpoint, endpoint, group))
}

/// Header scalars in ns. Missing keys mean malformed input (unconstrained
/// checks never reach here: they carry `type: "unconstrained"`).
fn check_scalars(raw: &RawCheck, line: usize) -> Result<(f64, f64, f64), ParseError> {
    match (raw.data_arrival_time, raw.required_time, raw.slack) {
        (Some(a), Some(r), Some(s)) => Ok((a * SEC_TO_NS, r * SEC_TO_NS, s * SEC_TO_NS)),
        _ => Err(err(
            line,
            "check without data_arrival_time/required_time/slack",
        )),
    }
}

/// Convert one raw check. `Ok(None)` = well-formed but unrepresentable
/// (unconstrained, path-delay, or nonzero path margin): skip and count.
fn convert_check(raw: &RawCheck, line: usize) -> Result<Option<TimingPath>, ParseError> {
    let kind = raw.kind.as_deref().unwrap_or("");
    if kind == "unconstrained" {
        return Ok(None);
    }
    if raw.path_delay.is_some() {
        return Ok(None);
    }
    if raw.path_margin.is_some_and(|m| m != 0.0) {
        return Ok(None);
    }
    let (check, startpoint, endpoint, group) = check_identity(raw, line)?;
    let (arrival, required, slack) = check_scalars(raw, line)?;
    let launch_edge = parse_edge(raw.source_clock_edge.as_deref());
    let capture_edge = parse_edge(raw.target_clock_edge.as_deref());
    let launch = convert_segment(
        raw.source_clock.as_deref(),
        launch_edge,
        raw.source_clock_path.as_deref(),
        line,
        "launch",
    )?;
    let capture = convert_segment(
        raw.target_clock.as_deref(),
        capture_edge,
        raw.target_clock_path.as_deref(),
        line,
        "capture",
    )?;
    let launch_ref = launch
        .as_ref()
        .and_then(|seg| seg.stages.last())
        .and_then(|p| p.arrival);
    let empty: &[RawPin] = &[];
    let points = difference_pins(
        raw.source_path.as_deref().unwrap_or(empty),
        launch_ref,
        line,
        "data",
    )?;
    // JSON margin is raw margin() (+setup); the schema stores the
    // required-offset contribution, so setup checks negate (reportRequired).
    let signed = |m: f64| {
        let v = if check == Check::Setup { -m } else { m } * SEC_TO_NS;
        v + 0.0 // normalize -0.0
    };
    let (margin, external_delay) = match (raw.margin, kind == "output_delay") {
        (None, _) => (None, None),
        (Some(m), false) => (Some(signed(m)), Some(0.0)),
        (Some(m), true) => (Some(0.0), Some(signed(m))),
    };
    let scale = arrival.abs().max(required.abs()).max(1e-12);
    Ok(Some(TimingPath {
        startpoint,
        endpoint,
        launch_clock: raw
            .source_clock
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        capture_clock: raw
            .target_clock
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        launch_edge,
        capture_edge,
        group,
        flavor: raw.kind.clone().filter(|k| !k.is_empty()),
        check,
        launch,
        capture,
        points,
        slack,
        required,
        arrival,
        uncertainty: None,
        crpr: raw.crpr.map(|c| c * SEC_TO_NS),
        margin,
        external_delay,
        rounding_ns: Some(JSON_HALF_ULP * scale),
    }))
}

fn convert_element(
    buf: &[u8],
    line: usize,
    stats: &mut IterStats,
) -> Option<Result<TimingPath, ParseError>> {
    let raw: RawCheck = match serde_json::from_slice(buf) {
        Ok(raw) => raw,
        Err(e) => {
            stats.errors += 1;
            return Some(Err(err(line, format!("invalid check JSON: {e}"))));
        }
    };
    match convert_check(&raw, line) {
        Ok(Some(path)) => Some(Ok(path)),
        Ok(None) => {
            stats.skipped += 1;
            None
        }
        Err(e) => {
            stats.errors += 1;
            Some(Err(e))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JsonState {
    SeekChecks,
    SeekBracket,
    ExpectElement,
    InElement,
    SkipElement,
    Done,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Streaming per-check results over native `-format json`. Memory is O(one
/// check): bytes are framed with brace matching (string/escape aware) and
/// each element converts independently. Malformed elements yield `Err` and
/// parsing resumes at the next element; unrepresentable checks
/// (unconstrained, path-delay, nonzero path margin) are skipped and counted
/// in [`JsonPathIter::stats`]. Commas between elements are decorative.
pub struct JsonPathIter<R> {
    reader: R,
    line: usize,
    state: JsonState,
    first: bool,
    depth: u32,
    in_string: bool,
    escape: bool,
    key_buf: Option<Vec<u8>>,
    pending_key: Option<Vec<u8>>,
    first_key: Option<String>,
    elem_buf: Vec<u8>,
    elem_depth: u32,
    elem_line: usize,
    stats: IterStats,
}

impl<R: std::io::BufRead> JsonPathIter<R> {
    #[must_use]
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            line: 1,
            state: JsonState::SeekChecks,
            first: true,
            depth: 0,
            in_string: false,
            escape: false,
            key_buf: None,
            pending_key: None,
            first_key: None,
            elem_buf: Vec::new(),
            elem_depth: 0,
            elem_line: 1,
            stats: IterStats::default(),
        }
    }

    #[must_use]
    pub fn stats(&self) -> IterStats {
        self.stats
    }

    fn fail(&mut self, message: String) -> Result<TimingPath, ParseError> {
        self.state = JsonState::Done;
        self.stats.errors += 1;
        Err(err(self.line, message))
    }

    fn expected_checks_msg(&self) -> String {
        match &self.first_key {
            Some(k) => format!("expected \"checks\" array (top-level key \"{k}\")"),
            None => "expected \"checks\" array".to_owned(),
        }
    }

    /// Track one element byte; true when the element just closed.
    fn track_byte(&mut self, b: u8) -> bool {
        if self.in_string {
            if self.escape {
                self.escape = false;
            } else if b == b'\\' {
                self.escape = true;
            } else if b == b'"' {
                self.in_string = false;
            }
            false
        } else {
            match b {
                b'{' | b'[' => {
                    self.elem_depth += 1;
                    false
                }
                b'"' => {
                    self.in_string = true;
                    false
                }
                b'}' | b']' => {
                    self.elem_depth = self.elem_depth.saturating_sub(1);
                    self.elem_depth == 0
                }
                _ => false,
            }
        }
    }

    fn seek_bracket_byte(&mut self, b: u8) -> Option<Result<TimingPath, ParseError>> {
        if is_ws(b) {
            return None;
        }
        if b == b'[' {
            self.state = JsonState::ExpectElement;
            self.first = true;
            return None;
        }
        Some(self.fail("expected '[' after \"checks\"".to_owned()))
    }

    fn seek_byte(&mut self, b: u8) -> Option<Result<TimingPath, ParseError>> {
        if let Some(key) = self.pending_key.take() {
            if is_ws(b) {
                self.pending_key = Some(key);
                return None;
            }
            if b == b':' {
                if key == b"checks" {
                    self.state = JsonState::SeekBracket;
                } else if self.first_key.is_none() {
                    self.first_key = Some(String::from_utf8_lossy(&key).into_owned());
                }
                return None;
            }
            // A value, not a key: fall through to normal handling.
        }
        if self.in_string {
            if self.escape {
                self.escape = false;
                if let Some(key) = self.key_buf.as_mut() {
                    key.push(b);
                }
            } else if b == b'\\' {
                self.escape = true;
            } else if b == b'"' {
                self.in_string = false;
                if let Some(key) = self.key_buf.take() {
                    self.pending_key = Some(key);
                }
            } else if let Some(key) = self.key_buf.as_mut() {
                key.push(b);
            }
            return None;
        }
        match b {
            b'"' => {
                self.in_string = true;
                if self.depth == 1 {
                    self.key_buf = Some(Vec::new());
                }
            }
            b'{' | b'[' => self.depth += 1,
            b'}' | b']' => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        None
    }

    fn expect_byte(&mut self, b: u8) -> Option<Result<TimingPath, ParseError>> {
        if is_ws(b) {
            return None;
        }
        if b == b']' {
            self.state = JsonState::Done;
            return None;
        }
        if b == b',' {
            if self.first {
                self.stats.errors += 1;
                return Some(Err(err(self.line, "expected check object, found ','")));
            }
            self.first = true;
            return None;
        }
        if b == b'{' {
            self.state = JsonState::InElement;
            self.elem_buf.clear();
            self.elem_depth = 1;
            self.in_string = false;
            self.escape = false;
            self.elem_line = self.line;
            self.elem_buf.push(b);
            return None;
        }
        Some(self.fail("expected check object in \"checks\" array".to_owned()))
    }

    fn element_byte(&mut self, b: u8) -> Option<Result<TimingPath, ParseError>> {
        if self.elem_buf.len() >= ELEMENT_CAP_BYTES {
            self.state = JsonState::SkipElement;
            self.stats.errors += 1;
            let item = Some(Err(err(self.elem_line, "check exceeds 1 MiB cap")));
            if self.track_byte(b) {
                self.state = JsonState::ExpectElement;
                self.first = false;
            }
            return item;
        }
        self.elem_buf.push(b);
        if self.track_byte(b) {
            self.state = JsonState::ExpectElement;
            self.first = false;
            self.stats.blocks += 1;
            let line = self.elem_line;
            return convert_element(&self.elem_buf, line, &mut self.stats);
        }
        None
    }

    fn skip_byte(&mut self, b: u8) {
        if self.track_byte(b) {
            self.state = JsonState::ExpectElement;
            self.first = false;
            self.stats.blocks += 1;
        }
    }
}

impl<R: std::io::BufRead> Iterator for JsonPathIter<R> {
    type Item = Result<TimingPath, ParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.state == JsonState::Done {
            return None;
        }
        loop {
            let bytes: Vec<u8> = match self.reader.fill_buf() {
                Err(e) => {
                    self.state = JsonState::Done;
                    self.stats.errors += 1;
                    return Some(Err(err(self.line, format!("read error: {e}"))));
                }
                Ok(chunk) => chunk.to_vec(),
            };
            if bytes.is_empty() {
                let was_seeking = self.state == JsonState::SeekChecks;
                self.state = JsonState::Done;
                self.stats.errors += 1;
                return Some(Err(err(
                    self.line,
                    if was_seeking {
                        self.expected_checks_msg()
                    } else {
                        "truncated \"checks\" array".to_owned()
                    },
                )));
            }
            let mut consumed = 0;
            let mut item = None;
            for b in bytes {
                consumed += 1;
                if b == b'\n' {
                    self.line += 1;
                }
                let produced = match self.state {
                    JsonState::SeekChecks => self.seek_byte(b),
                    JsonState::SeekBracket => self.seek_bracket_byte(b),
                    JsonState::ExpectElement => self.expect_byte(b),
                    JsonState::InElement => self.element_byte(b),
                    JsonState::SkipElement => {
                        self.skip_byte(b);
                        None
                    }
                    JsonState::Done => None,
                };
                if produced.is_some() || self.state == JsonState::Done {
                    item = produced;
                    break;
                }
            }
            self.reader.consume(consumed);
            if item.is_some() || self.state == JsonState::Done {
                return item;
            }
        }
    }
}

/// Scan a window for top-level JSON keys. Returns (`has_checks`, `has_otps`).
fn scan_top_keys(buf: &[u8]) -> (bool, bool) {
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escape = false;
    let mut in_key = false;
    let mut key_buf = Vec::new();
    let mut pending: Option<Vec<u8>> = None;
    let mut has_checks = false;
    let mut has_otps = false;
    for &b in buf {
        if let Some(key) = pending.take() {
            if is_ws(b) {
                pending = Some(key);
                continue;
            }
            if b == b':' {
                if key == b"checks" {
                    has_checks = true;
                } else if key == b"schema_version" || key == b"paths" {
                    has_otps = true;
                }
                continue;
            }
        }
        if in_string {
            if escape {
                escape = false;
                if in_key {
                    key_buf.push(b);
                }
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
                if in_key {
                    in_key = false;
                    pending = Some(std::mem::take(&mut key_buf));
                }
            } else if in_key {
                key_buf.push(b);
            }
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                if depth == 1 {
                    in_key = true;
                    key_buf.clear();
                }
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    (has_checks, has_otps)
}

/// Peek at a JSON stream to dispatch on shape, returning the kind plus the
/// consumed bytes (chain them back before parsing).
///
/// # Errors
///
/// Returns an I/O error on read failure or unrecognized/mixed top level.
pub fn sniff_json_kind<R: std::io::BufRead>(
    reader: &mut R,
) -> Result<(InputKind, Vec<u8>), std::io::Error> {
    let mut buf = Vec::new();
    while buf.len() < 16_384 {
        let chunk: Vec<u8> = reader.fill_buf()?.to_vec();
        if chunk.is_empty() {
            break;
        }
        reader.consume(chunk.len());
        buf.extend_from_slice(&chunk);
    }
    match scan_top_keys(&buf) {
        (true, false) => Ok((InputKind::StaJson, buf)),
        (false, true) => Ok((InputKind::OtpsJson, buf)),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unrecognized JSON report (want \"checks\" or OTPS top level)",
        )),
    }
}

/// Parse native `-format json` from a stream. Unrepresentable checks are
/// skipped and counted on the report.
///
/// # Errors
///
/// Returns [`ParseError`] with an approximate 1-based line number on
/// malformed input.
pub fn parse_sta_json<R: std::io::BufRead>(
    reader: R,
    run: Run,
) -> Result<TimingReport, ParseError> {
    let mut iter = JsonPathIter::new(reader);
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

    const SETUP_CHECK: &str = r#"{
  "type": "check",
  "path_group": "clk",
  "path_type": "max",
  "startpoint": "u_core/ff1",
  "endpoint": "u_core/ff2",
  "source_clock": "clk",
  "source_clock_edge": "rise",
  "source_clock_path": [
    {"instance": "", "pin": "clk", "arrival": 0.000e+00, "slew": 0.000e+00},
    {"instance": "u_core/ff1", "cell": "DFF_X1", "pin": "u_core/ff1/CK",
     "net": "clk", "arrival": 1.000e-11, "slew": 1.000e-11}
  ],
  "source_path": [
    {"instance": "u_core/ff1", "cell": "DFF_X1", "pin": "u_core/ff1/Q",
     "net": "n1", "arrival": 6.500e-10, "capacitance": 1.900e-15,
     "slew": 1.100e-11},
    {"instance": "u_core/ff2", "cell": "DFF_X1", "pin": "u_core/ff2/D",
     "net": "n1", "arrival": 8.500e-10, "slew": 1.200e-11}
  ],
  "target_clock": "clk",
  "target_clock_edge": "rise",
  "target_clock_path": [
    {"instance": "", "pin": "clk", "arrival": 1.000e-09, "slew": 0.000e+00}
  ],
  "data_arrival_time": 8.500e-10,
  "crpr": 0.000e+00,
  "margin": 5.000e-11,
  "required_time": 9.000e-10,
  "slack": 5.000e-11
}"#;

    const HOLD_CHECK: &str = r#"{
  "type": "check",
  "path_group": "clk",
  "path_type": "min",
  "startpoint": "u_core/ff7",
  "endpoint": "u_core/ff8",
  "target_clock": "clk",
  "target_clock_edge": "rise",
  "source_path": [
    {"pin": "u_core/ff8/D", "net": "n7", "arrival": 6.000e-11,
     "slew": 1.000e-11}
  ],
  "data_arrival_time": 6.000e-11,
  "crpr": 0.000e+00,
  "margin": 1.000e-11,
  "required_time": 1.000e-11,
  "slack": 5.000e-11
}"#;

    fn parse_doc(checks: &str) -> TimingReport {
        let doc = format!("{{\"checks\": [{checks}]}}");
        parse_sta_json(std::io::Cursor::new(doc), Run::default()).expect("parse")
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn converts_setup_check() {
        let path = &parse_doc(SETUP_CHECK).paths[0];
        assert_eq!(path.check, Check::Setup);
        assert_eq!(path.flavor.as_deref(), Some("check"));
        assert_eq!(path.group, "clk");
        assert!(approx(path.arrival, 0.85));
        assert!(approx(path.required, 0.90));
        assert!(approx(path.slack, 0.05));
        // JSON margin is +setup; stored negated (required-offset contribution).
        assert!(path.margin.is_some_and(|m| approx(m, -0.05)));
        assert_eq!(path.external_delay, Some(0.0));
        assert_eq!(path.uncertainty, None);
        assert!(path.crpr.is_some_and(|c| approx(c, 0.0)));
        assert!(path.rounding_ns.is_some_and(|r| approx(r, 5e-4 * 0.9)));
        let launch = path.launch.as_ref().expect("launch");
        assert_eq!(launch.clock, "clk");
        assert_eq!(launch.edge, Edge::Rise);
        assert!(launch.edge_time.is_some_and(|t| approx(t, 0.0)));
        assert_eq!(launch.stages.len(), 2);
        assert!(approx(launch.network_delay, 0.01));
        assert!(approx(launch.total().expect("total"), 0.01));
        let capture = path.capture.as_ref().expect("capture");
        assert!(capture.edge_time.is_some_and(|t| approx(t, 1.0)));
        assert_eq!(path.points.len(), 2);
        let (q, d) = (&path.points[0], &path.points[1]);
        assert!(approx(q.cell_delay, 0.64) && approx(q.net_delay, 0.0));
        assert!(q.split_available);
        assert!(approx(d.cell_delay, 0.0) && approx(d.net_delay, 0.20));
        assert!(approx(q.slew, 0.011));
        assert!(approx(q.cap, 1.9));
        assert_eq!(q.transition, None);
        assert!(q.arrival.is_some_and(|a| approx(a, 0.65)));
        let validation = path.validate(1e-6);
        assert_eq!(validation.severity, Severity::Clean);
        assert_eq!(validation.unknown_terms, vec!["uncertainty".to_owned()]);
    }

    #[test]
    fn converts_hold_check_verbatim() {
        let path = &parse_doc(HOLD_CHECK).paths[0];
        assert_eq!(path.check, Check::Hold);
        assert!(path.margin.is_some_and(|m| approx(m, 0.01)));
        assert_eq!(path.launch, None);
        let capture = path.capture.as_ref().expect("capture");
        assert!(capture.condensed);
        assert_eq!(capture.edge_time, None);
        // No launch reference: first point carries its total, unsplit.
        assert_eq!(path.points.len(), 1);
        assert!(!path.points[0].split_available);
        assert!(approx(path.points[0].cell_delay, 0.06));
        let validation = path.validate(1e-6);
        assert_eq!(validation.severity, Severity::Clean);
        assert_eq!(
            validation.unknown_terms,
            vec![
                "launch clock path".to_owned(),
                "capture edge time".to_owned()
            ]
        );
    }

    #[test]
    fn output_delay_routes_margin_to_external() {
        let check = SETUP_CHECK.replace("\"type\": \"check\"", "\"type\": \"output_delay\"");
        let path = &parse_doc(&check).paths[0];
        assert_eq!(path.flavor.as_deref(), Some("output_delay"));
        assert_eq!(path.margin, Some(0.0));
        assert!(path.external_delay.is_some_and(|e| approx(e, -0.05)));
    }

    #[test]
    fn skips_unrepresentable_checks() {
        let doc = format!(
            "{{\"checks\": [{SETUP_CHECK}, \
             {{\"type\": \"unconstrained\", \"path_group\": \"clk\", \"path_type\": \"max\", \
              \"startpoint\": \"a\", \"endpoint\": \"b\"}}, \
             {{\"type\": \"check\", \"path_group\": \"clk\", \"path_type\": \"max\", \
              \"startpoint\": \"a\", \"endpoint\": \"b\", \"path_delay\": 1.0e-9}}, \
             {{\"type\": \"check\", \"path_group\": \"clk\", \"path_type\": \"max\", \
              \"startpoint\": \"a\", \"endpoint\": \"b\", \"path_margin\": 1.0e-12}}]}}"
        );
        let report = parse_sta_json(std::io::Cursor::new(doc), Run::default()).expect("parse");
        assert_eq!(report.paths.len(), 1);
        assert_eq!(report.skipped_paths, 3);
    }

    #[test]
    fn missing_keys_error() {
        let bad = SETUP_CHECK.replace("  \"path_type\": \"max\",\n", "");
        let doc = format!("{{\"checks\": [{bad}]}}");
        let err = parse_sta_json(std::io::Cursor::new(doc), Run::default()).expect_err("path_type");
        assert!(err.message.contains("path_type"), "{}", err.message);
        let bad = SETUP_CHECK.replace(
            "  \"required_time\": 9.000e-10,\n  \"slack\": 5.000e-11\n",
            "  \"required_time\": 9.000e-10\n",
        );
        let doc = format!("{{\"checks\": [{bad}]}}");
        let err = parse_sta_json(std::io::Cursor::new(doc), Run::default()).expect_err("scalars");
        assert!(err.message.contains("data_arrival_time"), "{}", err.message);
    }

    #[test]
    fn pump_recovers_after_bad_element() {
        let bad = SETUP_CHECK.replace("\"path_type\": \"max\"", "\"path_type\": \"bogus\"");
        let doc = format!("{{\"checks\": [{SETUP_CHECK}, {bad}, {SETUP_CHECK}]}}");
        let mut iter = JsonPathIter::new(std::io::Cursor::new(doc));
        let items: Vec<_> = iter.by_ref().collect();
        assert_eq!(items.len(), 3);
        assert!(items[0].is_ok());
        assert!(items[1].is_err());
        assert!(items[2].is_ok());
        assert_eq!(
            iter.stats(),
            IterStats {
                blocks: 3,
                skipped: 0,
                errors: 1
            }
        );
    }

    #[test]
    fn pump_rejects_oversize_element() {
        let pad = "x".repeat(1 << 20);
        let big = format!("{{\"type\": \"check\", \"pad\": \"{pad}\"}}");
        let doc = format!("{{\"checks\": [{big}, {SETUP_CHECK}]}}");
        let mut iter = JsonPathIter::new(std::io::Cursor::new(doc));
        let items: Vec<_> = iter.by_ref().collect();
        assert_eq!(items.len(), 2);
        assert!(items[0]
            .as_ref()
            .expect_err("cap")
            .message
            .contains("1 MiB"));
        assert!(items[1].is_ok());
        assert_eq!(iter.stats().errors, 1);
        assert_eq!(iter.stats().blocks, 2);
    }

    #[test]
    fn truncation_errors_after_yielded_items() {
        let doc = format!("{{\"checks\": [{SETUP_CHECK}");
        let items: Vec<_> = JsonPathIter::new(std::io::Cursor::new(doc)).collect();
        assert_eq!(items.len(), 2);
        assert!(items[0].is_ok());
        assert!(items[1]
            .as_ref()
            .expect_err("truncated")
            .message
            .contains("truncated"));
    }

    #[test]
    fn empty_checks_parses_empty() {
        let report = parse_doc("");
        assert_eq!(report.paths, Vec::new());
        assert_eq!(report.skipped_paths, 0);
    }

    #[test]
    fn pin_parent_fallback_splits_stages() {
        let pins = ["u_buf/A", "u_buf/Z", "u_next/A"]
            .into_iter()
            .enumerate()
            .map(|(i, pin)| RawPin {
                pin: Some(pin.to_owned()),
                net: None,
                cell: None,
                instance: None,
                #[allow(clippy::cast_precision_loss)]
                arrival: Some(1e-11 * (i as f64 + 1.0)),
                slew: None,
                capacitance: None,
            })
            .collect::<Vec<_>>();
        let points = difference_pins(&pins, Some(0.0), 1, "test").expect("diff");
        assert_eq!(points.len(), 3);
        assert!(approx(points[0].cell_delay, 0.01));
        assert!(approx(points[1].cell_delay, 0.01)); // same parent: cell arc
        assert!(approx(points[1].net_delay, 0.0));
        assert!(approx(points[2].net_delay, 0.01)); // new parent: net arc
        assert!(approx(points[2].cell_delay, 0.0));
    }

    #[test]
    fn sniff_dispatches_json_shapes() {
        use std::io::Read as _;
        let native = format!("{{\"checks\": [{SETUP_CHECK}]}}");
        let mut cursor = std::io::Cursor::new(native.as_bytes().to_vec());
        let (kind, peek) = sniff_json_kind(&mut cursor).expect("sniff native");
        assert_eq!(kind, InputKind::StaJson);
        let mut chained = std::io::BufReader::new(std::io::Cursor::new(peek).chain(cursor));
        let mut rest = String::new();
        chained.read_to_string(&mut rest).expect("chain");
        assert_eq!(rest, native);
        let mut canonical =
            std::io::Cursor::new(br#"{"schema_version": "0.2.0", "paths": []}"#.to_vec());
        let (kind, _) = sniff_json_kind(&mut canonical).expect("sniff canonical");
        assert_eq!(kind, InputKind::OtpsJson);
        let mut garbage = std::io::Cursor::new(br#"{"nope": 1}"#.to_vec());
        assert!(sniff_json_kind(&mut garbage).is_err());
    }
}

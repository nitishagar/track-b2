//! Parsers: OTPS text format v0.1 (an `OpenSTA report_checks` subset mapping)
//! plus a JSON report importer for in-tool exporter dumps.
//!
//! The text parser is hand-rolled (no regex) line parsing over `&str`,
//! so multi-GB reports stay limited by I/O, not by the parser.

use std::fmt;

use crate::schema::{Check, Run, TimingPath, TimingPoint, TimingReport};

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

fn err(line: usize, message: impl Into<String>) -> ParseError {
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

fn parse_check(raw: &str, lineno: usize) -> Result<Check> {
    match raw.to_ascii_lowercase().as_str() {
        "setup" | "max" => Ok(Check::Setup),
        "hold" | "min" => Ok(Check::Hold),
        other => Err(err(lineno, format!("bad check type '{other}'"))),
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
            other => return Err(err(lineno, format!("unknown attribute '{other}'"))),
        }
    }
    Ok(point)
}

type Line<'a> = (usize, &'a str);

fn next_line<'a>(lines: &[Line<'a>], idx: &mut usize, ctx: usize) -> Result<Line<'a>> {
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
        return Err(err(n, format!("expected '{prefix}'")));
    }
    Ok((n, v.to_owned()))
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
    let (n, v) = header(lines, idx, ctx, "Crpr:")?;
    let crpr = number(&v, "Crpr:", n)?;
    let (n, v) = header(lines, idx, ctx, "Uncertainty:")?;
    let uncertainty = number(&v, "Uncertainty:", n)?;
    let (n, v) = next_line(lines, idx, ctx)?;
    if v != "Points:" {
        return Err(err(n, "expected 'Points:'"));
    }

    let mut points = Vec::new();
    loop {
        let (n, v) = next_line(lines, idx, ctx)?;
        if v == "End" {
            break;
        }
        points.push(parse_point(v, n)?);
    }

    Ok(TimingPath {
        startpoint,
        endpoint,
        launch_clock,
        capture_clock,
        group,
        check,
        slack,
        required,
        arrival,
        crpr,
        uncertainty,
        points,
    })
}

/// Parse one OTPS text report. `run` describes the run the text came from.
///
/// # Errors
///
/// Returns [`ParseError`] with a 1-based line number when the input is not a
/// well-formed OTPS text report.
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
///
/// # Errors
///
/// Returns the `serde_json` error when the input is not a valid report.
pub fn parse_json(input: &str) -> serde_json::Result<TimingReport> {
    serde_json::from_str(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# sample\n\
        Startpoint: u_core/ff1\n\
        Endpoint: u_core/ff2\n\
        Launch clock: clk\n\
        Capture clock: clk\n\
        Group: clk\n\
        Check: setup\n\
        Slack: 0.100\n\
        Required: 1.000\n\
        Arrival: 0.900\n\
        Crpr: 0.000\n\
        Uncertainty: 0.050\n\
        Points:\n\
        u_core/ff1/CK | DFF | net_clk | cell=0.050 net=0.010 slew=0.020 cap=0.003 fanout=2\n\
        u_core/ff2/D | DFF | net_d | cell=0.640 net=0.200 slew=0.040 cap=0.005 fanout=1\n\
        End\n";

    #[test]
    fn parses_sample_and_validates() {
        let report = parse_text(SAMPLE, Run::default()).expect("parse");
        assert_eq!(report.paths.len(), 1);
        let path = &report.paths[0];
        assert_eq!(path.check, Check::Setup);
        assert_eq!(path.points.len(), 2);
        assert_eq!(path.validate(1e-9), Vec::<String>::new());
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
        let input = SAMPLE.replace("fanout=2\n", "fanout=2 | garbage\n");
        assert!(parse_text(&input, Run::default()).is_err());
    }

    #[test]
    fn json_round_trip() {
        let report = parse_text(SAMPLE, Run::default()).expect("parse");
        let json = serde_json::to_string(&report).expect("json");
        let back: TimingReport = parse_json(&json).expect("reparse");
        assert_eq!(report, back);
    }
}

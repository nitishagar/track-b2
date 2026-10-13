//! `otps-wasm`: WASM boundary for the otps diff engine (offline demo page).
//!
//! [`diff_reports`] mirrors `run_diff` in `crates/otps-cli/src/main.rs` using
//! only public `otps-core` API. Success output is `serde_json::to_string_pretty`
//! of a [`WasmDiffOutput`] whose field order matches the CLI `DiffOutput`
//! declaration order exactly, so the JSON is byte-identical to
//! `otps diff --format json` (the CLI adds one trailing newline via `println!`).

use std::io::{Cursor, Read};

use otps_core::{
    attribute, cluster, match_paths, parse_json, parse_text, sniff_json_kind, sniff_text_kind,
    Attribution, Cluster, InputKind, JsonPathIter, ParseOpts, PathIter, Run, StreamStats,
    TimingReport, SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Optional per-side run identity. Every field falls back: `new_run` fields
/// fall back to the resolved `base_run` values, `base_run` fields to the CLI
/// `diff` defaults (`tool = "opensta"`, everything else `"unknown"`).
#[derive(Debug, Default, Deserialize)]
pub struct Opts {
    /// Base-side run identity (all fields optional).
    #[serde(default)]
    pub base_run: RunOpts,
    /// New-side run overrides (all fields optional, nullable, fall back to base).
    #[serde(default)]
    pub new_run: RunOpts,
}

/// One side's run-identity overrides; `None` (missing or explicit `null`)
/// means "fall back".
#[derive(Debug, Default, Deserialize)]
pub struct RunOpts {
    /// Tool name (`"opensta"` by default on the base side).
    #[serde(default)]
    pub tool: Option<String>,
    /// Tool version (`"unknown"` by default).
    #[serde(default)]
    pub tool_version: Option<String>,
    /// Corner (`"unknown"` by default).
    #[serde(default)]
    pub corner: Option<String>,
    /// Mode (`"unknown"` by default).
    #[serde(default)]
    pub mode: Option<String>,
    /// PDK (`"unknown"` by default).
    #[serde(default)]
    pub pdk: Option<String>,
    /// Flow hash (`"unknown"` by default).
    #[serde(default)]
    pub flow_hash: Option<String>,
}

impl Opts {
    /// Parse the `opts_json` boundary object. Anything malformed (bad JSON,
    /// wrong types) is an error-string, never a throw.
    ///
    /// # Errors
    ///
    /// Returns the human-readable message when `json` is not a valid opts object.
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("bad opts_json: {e}"))
    }
}

fn base_run(opts: &RunOpts) -> Run {
    Run {
        tool: opts.tool.clone().unwrap_or_else(|| "opensta".to_owned()),
        version: opts
            .tool_version
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
        corner: opts.corner.clone().unwrap_or_else(|| "unknown".to_owned()),
        mode: opts.mode.clone().unwrap_or_else(|| "unknown".to_owned()),
        pdk: opts.pdk.clone().unwrap_or_else(|| "unknown".to_owned()),
        flow_hash: opts
            .flow_hash
            .clone()
            .unwrap_or_else(|| "unknown".to_owned()),
    }
}

fn new_run(opts: &RunOpts, base: &Run) -> Run {
    Run {
        tool: opts.tool.clone().unwrap_or_else(|| base.tool.clone()),
        version: opts
            .tool_version
            .clone()
            .unwrap_or_else(|| base.version.clone()),
        corner: opts.corner.clone().unwrap_or_else(|| base.corner.clone()),
        mode: opts.mode.clone().unwrap_or_else(|| base.mode.clone()),
        pdk: opts.pdk.clone().unwrap_or_else(|| base.pdk.clone()),
        flow_hash: opts
            .flow_hash
            .clone()
            .unwrap_or_else(|| base.flow_hash.clone()),
    }
}

/// Field-for-field replica of the CLI `DiffOutput` (same declaration order —
/// serde serializes in declaration order — so output bytes match the CLI).
#[derive(Debug, Serialize)]
struct WasmDiffOutput {
    schema_version: &'static str,
    base_paths: usize,
    new_paths: usize,
    matched: usize,
    unmatched_base: usize,
    unmatched_new: usize,
    match_rate: f64,
    fuzzy_rate: f64,
    combined_rate: f64,
    exact_matches: usize,
    fuzzy_matches: usize,
    worst_slack_delta_ns: f64,
    worst_residual_ns: f64,
    matches: Vec<otps_core::PathMatch>,
    attributions: Vec<Attribution>,
    clusters: Vec<Cluster>,
    /// Worst slack over unmatched-new paths (`None` when fully matched).
    unmatched_new_wns_ns: Option<f64>,
    unmatched_new_endpoints: Vec<String>,
    /// G10: per-side run identity plus field-level mismatches.
    base_run: Run,
    new_run: Run,
    run_mismatches: Vec<String>,
}

/// G10: field-level run comparison, e.g. `corner: 'tt' vs 'ss'`.
fn run_mismatches(base: &Run, new: &Run) -> Vec<String> {
    [
        ("tool", base.tool.as_str(), new.tool.as_str()),
        ("tool_version", base.version.as_str(), new.version.as_str()),
        ("corner", base.corner.as_str(), new.corner.as_str()),
        ("mode", base.mode.as_str(), new.mode.as_str()),
        ("pdk", base.pdk.as_str(), new.pdk.as_str()),
        ("flow_hash", base.flow_hash.as_str(), new.flow_hash.as_str()),
    ]
    .into_iter()
    .filter(|(_, b, n)| b != n)
    .map(|(field, b, n)| format!("{field}: '{b}' vs '{n}'"))
    .collect()
}

fn collect_streamed<I>(
    iter: I,
    label: &str,
    run: Run,
    opts: &ParseOpts,
) -> Result<TimingReport, String>
where
    I: Iterator<Item = Result<otps_core::TimingPath, otps_core::ParseError>> + StreamStats,
{
    let mut iter = iter;
    let mut paths = Vec::new();
    let mut err_count = 0usize;
    for item in iter.by_ref() {
        if let Ok(p) = item {
            paths.push(p);
        } else {
            err_count += 1;
            if err_count > opts.max_errors {
                return Err(format!("{label}: too many errors (>{})", opts.max_errors));
            }
        }
    }
    let stats = iter.stats();
    let mut report = TimingReport::new(run, paths);
    report.skipped_paths = stats.skipped;
    Ok(report)
}

fn load_report_str(text: &str, run: Run, label: &str) -> Result<TimingReport, String> {
    let opts = ParseOpts::default();
    // The CLI picks JSON-vs-text by file extension; the wasm boundary has no
    // filenames, so it sniffs content: `{` opens both JSON layouts
    // (OTPS JSON, STA native JSON), everything else goes down the text path.
    if text.trim_start().starts_with('{') {
        let mut reader = Cursor::new(text.as_bytes());
        let (kind, peek) = sniff_json_kind(&mut reader).map_err(|e| format!("{label}: {e}"))?;
        let mut chained = std::io::BufReader::new(Cursor::new(peek).chain(reader));
        return match kind {
            InputKind::OtpsJson => {
                let mut buf = String::new();
                chained
                    .read_to_string(&mut buf)
                    .map_err(|e| format!("{label}: read: {e}"))?;
                parse_json(&buf).map_err(|e| format!("{label}: json: {e}"))
            }
            InputKind::StaJson => collect_streamed(JsonPathIter::new(chained), label, run, &opts),
            InputKind::StaText | InputKind::OtpsLegacy => Err(format!(
                "{label}: internal error: text layout from JSON sniff"
            )),
        };
    }
    let mut reader = Cursor::new(text.as_bytes());
    let (kind, peek) = sniff_text_kind(&mut reader).map_err(|e| format!("{label}: {e}"))?;
    let mut chained = std::io::BufReader::new(Cursor::new(peek).chain(reader));
    match kind {
        InputKind::OtpsLegacy => {
            let mut buf = String::new();
            chained
                .read_to_string(&mut buf)
                .map_err(|e| format!("{label}: read: {e}"))?;
            parse_text(&buf, run).map_err(|e| format!("{label}: {e}"))
        }
        InputKind::StaText => {
            collect_streamed(PathIter::new(chained, opts.clone()), label, run, &opts)
        }
        InputKind::StaJson | InputKind::OtpsJson => Err(format!(
            "{label}: internal error: JSON layout from text sniff"
        )),
    }
}

fn err_shape(msg: &str) -> String {
    serde_json::to_string(&serde_json::json!({ "error": msg }))
        .unwrap_or_else(|_| r#"{"error":"internal error"}"#.to_owned())
}

/// Pure diff pipeline: mirrors CLI `run_diff` (JSON format, no gate).
/// Success is the pretty-printed diff JSON; any failure (parse errors,
/// validation `Reject`) is the `{"error":"..."}` shape, never a throw.
#[must_use]
pub fn diff_json(base: &str, new: &str, opts: &Opts) -> String {
    let outcome = (|| -> Result<String, String> {
        let base_run = base_run(&opts.base_run);
        let new_run = new_run(&opts.new_run, &base_run);
        let base_report = load_report_str(base, base_run, "base")?;
        let new_report = load_report_str(new, new_run, "new")?;
        for path in base_report.paths.iter().chain(new_report.paths.iter()) {
            let validation = path.validate(1e-6);
            if validation.severity == otps_core::Severity::Reject {
                return Err(format!(
                    "{}: {}",
                    path.endpoint,
                    validation.problems.join("; ")
                ));
            }
        }

        let matched = match_paths(&base_report.paths, &new_report.paths);
        let mut attributions: Vec<Attribution> = matched
            .matches
            .iter()
            .map(|m| attribute(&base_report.paths[m.base], &new_report.paths[m.new]))
            .collect();
        attributions.sort_by(|a, b| {
            a.slack_delta
                .partial_cmp(&b.slack_delta)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let clusters = cluster(&attributions);
        let worst_slack_delta_ns = attributions
            .iter()
            .map(|a| a.slack_delta)
            .fold(0.0, f64::min);
        let worst_residual_ns = attributions
            .iter()
            .map(|a| a.residual.abs())
            .fold(0.0, f64::max);

        let output = WasmDiffOutput {
            schema_version: SCHEMA_VERSION,
            base_paths: base_report.paths.len(),
            new_paths: new_report.paths.len(),
            matched: matched.matches.len(),
            unmatched_base: matched.unmatched_base.len(),
            unmatched_new: matched.unmatched_new.len(),
            match_rate: matched.match_rate,
            fuzzy_rate: matched.fuzzy_rate,
            combined_rate: matched.combined_rate,
            exact_matches: matched.exact_matches,
            fuzzy_matches: matched.fuzzy_matches,
            worst_slack_delta_ns,
            worst_residual_ns,
            matches: matched.matches.clone(),
            attributions,
            clusters,
            unmatched_new_wns_ns: matched
                .unmatched_new
                .iter()
                .map(|&j| new_report.paths[j].slack)
                .reduce(f64::min),
            unmatched_new_endpoints: matched
                .unmatched_new
                .iter()
                .map(|&j| new_report.paths[j].endpoint.clone())
                .collect(),
            base_run: base_report.run.clone(),
            new_run: new_report.run.clone(),
            run_mismatches: run_mismatches(&base_report.run, &new_report.run),
        };
        serde_json::to_string_pretty(&output).map_err(|e| format!("json: {e}"))
    })();
    match outcome {
        Ok(json) => json,
        Err(msg) => err_shape(&msg),
    }
}

/// Diff two timing reports given as UTF-8 text. `opts_json` is the
/// `{"base_run": {...}, "new_run": {...}}` object (all fields optional;
/// `"{}"` selects CLI defaults). Returns the pretty diff JSON, or
/// `{"error":"..."}` on any failure — never throws.
#[must_use]
#[wasm_bindgen]
pub fn diff_reports(base: &str, new: &str, opts_json: &str) -> String {
    console_error_panic_hook::set_once();
    let opts = match Opts::parse(opts_json) {
        Ok(opts) => opts,
        Err(msg) => return err_shape(&msg),
    };
    diff_json(base, new, &opts)
}

/// The OTPS schema version this build diffs against (== core `SCHEMA_VERSION`).
#[must_use]
#[wasm_bindgen]
pub fn schema_version() -> String {
    SCHEMA_VERSION.to_owned()
}

//! `otps`: timing-correlation diff CLI. Offline only, zero telemetry.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use otps_core::{
    attribute, cluster, match_paths, parse_json, parse_text, sniff_json_kind, sniff_text_kind,
    Attribution, Cluster, InputKind, JsonPathIter, ParseOpts, PathIter, Run, StreamStats,
    SCHEMA_VERSION,
};

#[derive(Debug, Parser)]
#[command(
    name = "otps",
    version,
    about = "Timing-correlation diff engine (offline, zero telemetry)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Parse one report and print its paths.
    Parse {
        file: PathBuf,
        #[command(flatten)]
        run: RunArgs,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
    },
    /// Diff two reports: match paths, attribute slack deltas, cluster causes.
    Diff {
        base: PathBuf,
        new: PathBuf,
        #[command(flatten)]
        base_run: BaseRunArgs,
        #[command(flatten)]
        new_run: NewRunArgs,
        #[arg(long, value_enum, default_value = "text")]
        format: Format,
        /// Fail (exit 2) if any matched slack delta is worse than -N ps.
        #[arg(long)]
        fail_on_regression_ps: Option<f64>,
    },
}

#[derive(Debug, Clone, clap::Args)]
struct RunArgs {
    #[arg(long, default_value = "opensta")]
    tool: String,
    #[arg(long, default_value = "unknown")]
    tool_version: String,
    #[arg(long, default_value = "unknown")]
    corner: String,
    #[arg(long, default_value = "unknown")]
    mode: String,
    #[arg(long, default_value = "unknown")]
    pdk: String,
    #[arg(long, default_value = "unknown")]
    flow_hash: String,
}

/// G10: base-side run identity for `diff`. The unprefixed spellings stay as
/// aliases so existing invocations (`--corner sky130hd`) keep working.
#[derive(Debug, Clone, clap::Args)]
struct BaseRunArgs {
    #[arg(long = "base-tool", visible_alias = "tool", default_value = "opensta")]
    tool: String,
    #[arg(
        long = "base-tool-version",
        visible_alias = "tool-version",
        default_value = "unknown"
    )]
    tool_version: String,
    #[arg(
        long = "base-corner",
        visible_alias = "corner",
        default_value = "unknown"
    )]
    corner: String,
    #[arg(long = "base-mode", visible_alias = "mode", default_value = "unknown")]
    mode: String,
    #[arg(long = "base-pdk", visible_alias = "pdk", default_value = "unknown")]
    pdk: String,
    #[arg(
        long = "base-flow-hash",
        visible_alias = "flow-hash",
        default_value = "unknown"
    )]
    flow_hash: String,
}

impl BaseRunArgs {
    fn into_run(self) -> Run {
        Run {
            tool: self.tool,
            version: self.tool_version,
            corner: self.corner,
            mode: self.mode,
            pdk: self.pdk,
            flow_hash: self.flow_hash,
        }
    }
}

/// G10: new-side run overrides for `diff`. Every field falls back to the
/// base-side value, so a plain `--base-corner X` run compares X against X.
#[derive(Debug, Clone, Default, clap::Args)]
struct NewRunArgs {
    // Explicit ids: both run structs flatten into `diff`, so arg ids must
    // be unique even though the field names match `BaseRunArgs`.
    #[arg(id = "new_tool", long = "new-tool")]
    tool: Option<String>,
    #[arg(id = "new_tool_version", long = "new-tool-version")]
    tool_version: Option<String>,
    #[arg(id = "new_corner", long = "new-corner")]
    corner: Option<String>,
    #[arg(id = "new_mode", long = "new-mode")]
    mode: Option<String>,
    #[arg(id = "new_pdk", long = "new-pdk")]
    pdk: Option<String>,
    #[arg(id = "new_flow_hash", long = "new-flow-hash")]
    flow_hash: Option<String>,
}

impl NewRunArgs {
    fn into_run(self, base: &Run) -> Run {
        Run {
            tool: self.tool.unwrap_or_else(|| base.tool.clone()),
            version: self.tool_version.unwrap_or_else(|| base.version.clone()),
            corner: self.corner.unwrap_or_else(|| base.corner.clone()),
            mode: self.mode.unwrap_or_else(|| base.mode.clone()),
            pdk: self.pdk.unwrap_or_else(|| base.pdk.clone()),
            flow_hash: self.flow_hash.unwrap_or_else(|| base.flow_hash.clone()),
        }
    }
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

impl RunArgs {
    fn into_run(self) -> Run {
        Run {
            tool: self.tool,
            version: self.tool_version,
            corner: self.corner,
            mode: self.mode,
            pdk: self.pdk,
            flow_hash: self.flow_hash,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, Serialize)]
struct DiffOutput<'a> {
    schema_version: &'a str,
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

fn extension_is(path: &Path, want: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(want))
}

fn open_input(path: &Path) -> Result<Box<dyn std::io::BufRead>, String> {
    if path.as_os_str() == "-" {
        return Ok(Box::new(std::io::BufReader::new(std::io::stdin())));
    }
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    if extension_is(path, "gz") {
        Ok(Box::new(std::io::BufReader::new(
            flate2::read::GzDecoder::new(file),
        )))
    } else {
        Ok(Box::new(std::io::BufReader::new(file)))
    }
}

fn is_json_path(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let stem_is_json = Path::new(stem)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    extension_is(path, "json") || (extension_is(path, "gz") && stem_is_json)
}

fn collect_streamed<I>(
    iter: I,
    path: &Path,
    run: Run,
    opts: &ParseOpts,
) -> Result<otps_core::TimingReport, String>
where
    I: Iterator<Item = Result<otps_core::TimingPath, otps_core::ParseError>> + StreamStats,
{
    let mut iter = iter;
    let mut paths = Vec::new();
    let mut err_count = 0usize;
    for item in iter.by_ref() {
        match item {
            Ok(p) => paths.push(p),
            Err(e) => {
                eprintln!("otps: {}: {e}", path.display());
                err_count += 1;
                if err_count > opts.max_errors {
                    return Err(format!(
                        "{}: too many errors (>{})",
                        path.display(),
                        opts.max_errors
                    ));
                }
            }
        }
    }
    let stats = iter.stats();
    if stats.skipped > 0 {
        eprintln!(
            "otps: {}: skipped {} unrepresentable path(s)",
            path.display(),
            stats.skipped
        );
    }
    let mut report = otps_core::TimingReport::new(run, paths);
    report.skipped_paths = stats.skipped;
    Ok(report)
}

fn load_report(path: &Path, run: Run) -> Result<otps_core::TimingReport, String> {
    let opts = ParseOpts::default();
    let mut reader = open_input(path)?;
    if is_json_path(path) {
        let (kind, peek) =
            sniff_json_kind(&mut reader).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut chained = std::io::BufReader::new(std::io::Cursor::new(peek).chain(reader));
        return match kind {
            InputKind::OtpsJson => {
                let mut text = String::new();
                chained
                    .read_to_string(&mut text)
                    .map_err(|e| format!("read {}: {e}", path.display()))?;
                parse_json(&text).map_err(|e| format!("json {}: {e}", path.display()))
            }
            InputKind::StaJson => collect_streamed(JsonPathIter::new(chained), path, run, &opts),
            InputKind::StaText | InputKind::OtpsLegacy => Err(format!(
                "{}: internal error: text layout from JSON sniff",
                path.display()
            )),
        };
    }
    let (kind, peek) =
        sniff_text_kind(&mut reader).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut chained = std::io::BufReader::new(std::io::Cursor::new(peek).chain(reader));
    match kind {
        InputKind::OtpsLegacy => {
            let mut text = String::new();
            chained
                .read_to_string(&mut text)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            parse_text(&text, run).map_err(|e| format!("{}: {e}", path.display()))
        }
        InputKind::StaText => {
            collect_streamed(PathIter::new(chained, opts.clone()), path, run, &opts)
        }
        InputKind::StaJson | InputKind::OtpsJson => Err(format!(
            "{}: internal error: JSON layout from text sniff",
            path.display()
        )),
    }
}

fn run_diff(
    base: &Path,
    new: &Path,
    base_run: Run,
    new_run: Run,
    format: Format,
    fail_on_regression_ps: Option<f64>,
) -> Result<i32, String> {
    let base_report = load_report(base, base_run)?;
    let new_report = load_report(new, new_run)?;
    for path in base_report.paths.iter().chain(new_report.paths.iter()) {
        let validation = path.validate(1e-6);
        match validation.severity {
            otps_core::Severity::Reject => {
                return Err(format!(
                    "{}: {}",
                    path.endpoint,
                    validation.problems.join("; ")
                ));
            }
            otps_core::Severity::Warn => {
                eprintln!(
                    "otps: warn {}: {}",
                    path.endpoint,
                    validation.problems.join("; ")
                );
            }
            otps_core::Severity::Clean => {}
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

    let output = DiffOutput {
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
        run_mismatches: run_mismatches(&base_report.run, &new_report.run),
        base_run: base_report.run.clone(),
        new_run: new_report.run.clone(),
    };

    match format {
        Format::Json => {
            let json = serde_json::to_string_pretty(&output).map_err(|e| format!("json: {e}"))?;
            println!("{json}");
        }
        Format::Text => print_text(&output),
    }

    if let Some(budget_ps) = fail_on_regression_ps {
        return apply_gate(&output, budget_ps);
    }
    Ok(0)
}

/// G9: CI gate. Exit 2 when the worst matched regression or the unmatched-new
/// absolute WNS exceeds the budget; exit 0 when both pass.
fn apply_gate(output: &DiffOutput<'_>, budget_ps: f64) -> Result<i32, String> {
    if !budget_ps.is_finite() || budget_ps < 0.0 {
        return Err(format!(
            "bad --fail-on-regression-ps '{budget_ps}': need >= 0"
        ));
    }
    let budget = budget_ps / 1000.0;
    if output.worst_slack_delta_ns < -budget {
        eprintln!(
            "otps: gate failed: worst slack delta {:+.3} ns exceeds budget {budget_ps} ps",
            output.worst_slack_delta_ns
        );
        return Ok(2);
    }
    // Unmatched-new paths have no delta; gate their absolute WNS.
    if let Some(wns) = output.unmatched_new_wns_ns {
        if wns < -budget {
            eprintln!(
                "otps: gate failed: unmatched-new WNS {wns:+.3} ns exceeds budget {budget_ps} ps"
            );
            return Ok(2);
        }
    }
    Ok(0)
}

fn print_text(output: &DiffOutput<'_>) {
    println!("otps diff (schema {})", output.schema_version);
    println!(
        "base: {} paths  new: {} paths  matched: {} (unmatched base {}, new {})",
        output.base_paths,
        output.new_paths,
        output.matched,
        output.unmatched_base,
        output.unmatched_new
    );
    if !output.run_mismatches.is_empty() {
        println!("run mismatch: {}", output.run_mismatches.join("; "));
    }
    println!("match rate: {:.1}%", output.match_rate * 100.0);
    println!(
        "exact: {}  fuzzy: {}  combined rate: {:.1}% (renamed/ECO-touched paths stay visible)",
        output.exact_matches,
        output.fuzzy_matches,
        output.combined_rate * 100.0
    );
    println!("worst slack delta: {:.3} ns", output.worst_slack_delta_ns);
    println!("worst residual: {:.6} ns", output.worst_residual_ns);
    if let Some(wns) = output.unmatched_new_wns_ns {
        println!("unmatched-new WNS: {wns:+.3} ns");
    }
    println!("top regressions:");
    for attr in output
        .attributions
        .iter()
        .filter(|a| a.slack_delta < 0.0)
        .take(5)
    {
        println!(
            "  {:+.3} ns  {:?}  {} -> {}  (explained {:.0}%)",
            attr.slack_delta,
            attr.dominant,
            attr.startpoint,
            attr.endpoint,
            attr.explained_ratio * 100.0,
        );
        let stages: Vec<String> = attr
            .stages
            .iter()
            .take(3)
            .map(|s| {
                format!(
                    "{}{} {:+.3}",
                    s.pin,
                    match s.status {
                        otps_core::StageStatus::Matched => "",
                        otps_core::StageStatus::Added => "+",
                        otps_core::StageStatus::Removed => "-",
                    },
                    s.cell_delta + s.net_delta,
                )
            })
            .collect();
        println!(
            "    residual {:+.6} ns  confidence {}  unknowns [{}]  stages: {}",
            attr.residual,
            match &attr.confidence {
                otps_core::Confidence::High => "high".to_owned(),
                otps_core::Confidence::Reduced(reasons) =>
                    format!("reduced ({})", reasons.join("; ")),
            },
            attr.unknowns.join(", "),
            if stages.is_empty() {
                "-".to_owned()
            } else {
                stages.join(", ")
            },
        );
    }
    println!("clusters:");
    for cluster in &output.clusters {
        println!(
            "  {:?} x{} [{}]: total {:+.3} ns",
            cluster.dominant, cluster.count, cluster.prefix, cluster.total_slack_delta
        );
    }
}

fn run() -> Result<i32, String> {
    let cli = Cli::parse();
    match cli.command {
        Command::Parse { file, run, format } => {
            let report = load_report(&file, run.into_run())?;
            match format {
                Format::Json => {
                    let json =
                        serde_json::to_string_pretty(&report).map_err(|e| format!("json: {e}"))?;
                    println!("{json}");
                }
                Format::Text => {
                    println!(
                        "{} paths (schema {})",
                        report.paths.len(),
                        report.schema_version
                    );
                    for path in &report.paths {
                        println!(
                            "  {:+.3} ns  {:?}  {} -> {}",
                            path.slack, path.check, path.startpoint, path.endpoint
                        );
                    }
                }
            }
            Ok(0)
        }
        Command::Diff {
            base,
            new,
            base_run,
            new_run,
            format,
            fail_on_regression_ps,
        } => {
            let base_run = base_run.into_run();
            let new_run = new_run.into_run(&base_run);
            run_diff(
                &base,
                &new,
                base_run,
                new_run,
                format,
                fail_on_regression_ps,
            )
        }
    }
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("otps: {e}");
            std::process::exit(1);
        }
    }
}

//! `otps`: timing-correlation diff CLI. Offline only, zero telemetry.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

use otps_core::{
    attribute, cluster, match_paths, parse_json, parse_text, Attribution, Cluster, MatchKind, Run,
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
        base_run: RunArgs,
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
    worst_slack_delta_ns: f64,
    worst_residual_ns: f64,
    attributions: Vec<Attribution>,
    clusters: Vec<Cluster>,
}

fn extension_is(path: &Path, want: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(want))
}

fn read_input(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut text = String::new();
    if extension_is(path, "gz") {
        flate2::read::GzDecoder::new(file)
            .read_to_string(&mut text)
            .map_err(|e| format!("gunzip {}: {e}", path.display()))?;
    } else {
        std::io::BufReader::new(file)
            .read_to_string(&mut text)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
    }
    Ok(text)
}

fn load_report(path: &Path, run: Run) -> Result<otps_core::TimingReport, String> {
    let text = read_input(path)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let stem_is_json = Path::new(stem)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    let is_json = extension_is(path, "json") || (extension_is(path, "gz") && stem_is_json);
    if is_json {
        parse_json(&text).map_err(|e| format!("json {}: {e}", path.display()))
    } else {
        parse_text(&text, run).map_err(|e| format!("{}: {e}", path.display()))
    }
}

fn run_diff(
    base: &Path,
    new: &Path,
    run: Run,
    format: Format,
    fail_on_regression_ps: Option<f64>,
) -> Result<i32, String> {
    let base_report = load_report(base, run.clone())?;
    let new_report = load_report(new, run)?;
    for path in base_report.paths.iter().chain(new_report.paths.iter()) {
        let problems = path.validate(1e-6);
        if !problems.is_empty() {
            return Err(format!("{}: {}", path.endpoint, problems.join("; ")));
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
        worst_slack_delta_ns,
        worst_residual_ns,
        attributions,
        clusters,
    };

    match format {
        Format::Json => {
            let json = serde_json::to_string_pretty(&output).map_err(|e| format!("json: {e}"))?;
            println!("{json}");
        }
        Format::Text => print_text(&output, &matched.matches),
    }

    if let Some(budget_ps) = fail_on_regression_ps {
        if output.worst_slack_delta_ns < -budget_ps / 1000.0 {
            return Ok(2);
        }
    }
    Ok(0)
}

fn print_text(output: &DiffOutput<'_>, matches: &[otps_core::PathMatch]) {
    println!("otps diff (schema {})", output.schema_version);
    println!(
        "base: {} paths  new: {} paths  matched: {} (unmatched base {}, new {})",
        output.base_paths,
        output.new_paths,
        output.matched,
        output.unmatched_base,
        output.unmatched_new
    );
    println!("match rate: {:.1}%", output.match_rate * 100.0);
    let fuzzy = matches
        .iter()
        .filter(|m| matches!(m.kind, MatchKind::Fuzzy { .. }))
        .count();
    if fuzzy > 0 {
        println!("fuzzy matches: {fuzzy} (renamed/ECO-touched paths)");
    }
    println!("worst slack delta: {:.3} ns", output.worst_slack_delta_ns);
    println!("worst residual: {:.6} ns", output.worst_residual_ns);
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
            format,
            fail_on_regression_ps,
        } => run_diff(
            &base,
            &new,
            base_run.into_run(),
            format,
            fail_on_regression_ps,
        ),
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

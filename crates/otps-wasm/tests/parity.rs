//! Native parity tests for the wasm boundary: `diff_json` must reproduce the
//! CLI `diff --format json` values on all three fixture pairs, and failures
//! must use the `{"error":"..."}` shape.

use otps_wasm::{diff_json, Opts};
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn diff_pair(base: &str, new: &str) -> Value {
    let out = diff_json(
        &fixture(base),
        &fixture(new),
        &Opts::parse("{}").expect("default opts"),
    );
    // Parity trap guard: pretty JSON must open with schema_version first,
    // exactly like the CLI `DiffOutput` declaration order.
    assert!(
        out.starts_with("{\n  \"schema_version\": "),
        "field order drift: {out:.120}"
    );
    serde_json::from_str(&out).expect("success output parses")
}

#[test]
fn legacy_pair_matches_cli_values() {
    let d = diff_pair("base.rpt", "route.rpt");
    assert_eq!(d["schema_version"], "0.2.0");
    assert_eq!(d["base_paths"], 6);
    assert_eq!(d["new_paths"], 7);
    assert_eq!(d["matched"], 6);
    assert_eq!(d["unmatched_base"], 0);
    assert_eq!(d["unmatched_new"], 1);
    assert_eq!(d["match_rate"], 6.0 / 7.0);
    assert_eq!(d["exact_matches"], 6);
    assert_eq!(d["fuzzy_matches"], 0);
    assert_eq!(d["base_run"]["tool"], "opensta");
    assert_eq!(d["run_mismatches"], Value::Array(Vec::new()));
}

#[test]
fn sta_text_pair_matches_cli_values() {
    let d = diff_pair("sta_base.rpt", "sta_route.rpt");
    assert_eq!(d["base_paths"], 2);
    assert_eq!(d["new_paths"], 3);
    assert_eq!(d["matched"], 2);
    assert_eq!(d["match_rate"], 2.0 / 3.0);
    assert_eq!(d["unmatched_new"], 1);
}

#[test]
fn sta_json_pair_matches_cli_values() {
    let d = diff_pair("sta_native_base.json", "sta_native_route.json");
    assert_eq!(d["base_paths"], 2);
    assert_eq!(d["new_paths"], 3);
    assert_eq!(d["matched"], 2);
    assert_eq!(d["match_rate"], 2.0 / 3.0);
    assert_eq!(d["unmatched_new"], 1);
}

#[test]
fn garbage_input_returns_error_shape() {
    let out = diff_json("@@@ not a report @@@\n", "also garbage\n", &Opts::default());
    let v: Value = serde_json::from_str(&out).expect("error shape parses");
    assert!(v.get("error").is_some(), "missing error key: {out}");
    assert!(
        v.get("matched").is_none(),
        "error must not carry diff: {out}"
    );
}

#[test]
fn malformed_opts_returns_error_shape() {
    let out = otps_wasm::diff_reports("x", "y", "{nope");
    let v: Value = serde_json::from_str(&out).expect("error shape parses");
    assert!(v.get("error").is_some(), "missing error key: {out}");
}

#[test]
fn new_run_falls_back_to_base_run() {
    let opts = Opts::parse(
        r#"{"base_run": {"corner": "tt", "tool": "opensta"}, "new_run": {"corner": null}}"#,
    )
    .expect("opts parse");
    let out = diff_json(&fixture("base.rpt"), &fixture("route.rpt"), &opts);
    let d: Value = serde_json::from_str(&out).expect("success parses");
    assert_eq!(d["base_run"]["corner"], "tt");
    assert_eq!(d["new_run"]["corner"], "tt");
    assert_eq!(d["run_mismatches"], Value::Array(Vec::new()));
}

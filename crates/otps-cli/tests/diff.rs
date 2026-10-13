//! CLI end-to-end tests over the synthetic fixtures.

use std::process::Command;

fn fixtures() -> (String, String) {
    let dir = format!("{}/../../fixtures", env!("CARGO_MANIFEST_DIR"));
    (format!("{dir}/base.rpt"), format!("{dir}/route.rpt"))
}

/// G9 fixtures: matched pair regresses 10 ps; added path violates at -300 ps.
fn gate_fixtures() -> (String, String) {
    let dir = format!("{}/../../fixtures", env!("CARGO_MANIFEST_DIR"));
    (
        format!("{dir}/gate_base.rpt"),
        format!("{dir}/gate_route.rpt"),
    )
}

#[test]
fn diff_text_reports_match_rate() {
    let (base, new) = fixtures();
    let out = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new])
        .output()
        .expect("run otps");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("match rate: 85.7%"), "{stdout}");
    assert!(stdout.contains("worst slack delta: -0.102 ns"), "{stdout}");
    assert!(stdout.contains("NetDelay"), "{stdout}");
}

#[test]
fn diff_json_parses_and_gate_fails() {
    let (base, new) = fixtures();
    let out = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--format", "json"])
        .output()
        .expect("run otps");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json output");
    assert_eq!(json["matched"], 6);
    assert!((json["match_rate"].as_f64().unwrap() - 6.0 / 7.0).abs() < 1e-12);

    // Gate: 50 ps budget; worst regression is 102 ps, so this must fail with 2.
    let gated = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--fail-on-regression-ps", "50"])
        .output()
        .expect("run otps");
    assert_eq!(gated.status.code(), Some(2));

    // Gate: 200 ps budget passes.
    let passing = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--fail-on-regression-ps", "200"])
        .output()
        .expect("run otps");
    assert!(passing.status.success());

    // Gate: negative budget is rejected with exit 1, not a false failure.
    let negative = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--fail-on-regression-ps=-5"])
        .output()
        .expect("run otps");
    assert_eq!(negative.status.code(), Some(1));
}

#[test]
fn diff_real_layout_reports() {
    let dir = format!("{}/../../fixtures", env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args([
            "diff",
            &format!("{dir}/sta_base.rpt"),
            &format!("{dir}/sta_route.rpt"),
        ])
        .output()
        .expect("run otps");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("match rate: 66.7%"), "{stdout}");
    assert!(stdout.contains("worst slack delta: -0.100 ns"), "{stdout}");
}

#[test]
fn gate_fails_on_unmatched_new_violation() {
    let (base, new) = gate_fixtures();
    // 50 ps: the matched regression is only 10 ps, but the added path
    // violates at -300 ps, so the unmatched-new gate must trip with exit 2.
    let tripped = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--fail-on-regression-ps", "50"])
        .output()
        .expect("run otps");
    assert_eq!(tripped.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&tripped.stderr);
    assert!(stderr.contains("unmatched-new WNS"), "{stderr}");

    // 500 ps budget passes both gates.
    let passing = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--fail-on-regression-ps", "500"])
        .output()
        .expect("run otps");
    assert!(
        passing.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&passing.stderr)
    );
    let stdout = String::from_utf8_lossy(&passing.stdout);
    assert!(stdout.contains("unmatched-new WNS: -0.300 ns"), "{stdout}");

    // JSON carries the absolute WNS plus the offending endpoints.
    let out = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--format", "json"])
        .output()
        .expect("run otps");
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json output");
    assert!((json["unmatched_new_wns_ns"].as_f64().unwrap() + 0.300).abs() < 1e-12);
    assert!(json["unmatched_new_endpoints"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e.as_str() == Some("u_core/ff4")));
}

#[test]
fn per_side_runs_and_mismatch_visible() {
    let (base, new) = gate_fixtures();
    // Differing corners must be visible in text and JSON.
    let out = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args([
            "diff",
            &base,
            &new,
            "--base-corner",
            "tt",
            "--new-corner",
            "ss",
            "--format",
            "json",
        ])
        .output()
        .expect("run otps");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json output");
    assert_eq!(json["base_run"]["corner"], "tt");
    assert_eq!(json["new_run"]["corner"], "ss");
    assert!(json["run_mismatches"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m.as_str() == Some("corner: 'tt' vs 'ss'")));

    let text = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args([
            "diff",
            &base,
            &new,
            "--base-corner",
            "tt",
            "--new-corner",
            "ss",
        ])
        .output()
        .expect("run otps");
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(
        stdout.contains("run mismatch: corner: 'tt' vs 'ss'"),
        "{stdout}"
    );

    // New side falls back to base values: no false mismatch, and the legacy
    // unprefixed spelling still sets the base side.
    let fallback = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args(["diff", &base, &new, "--corner", "sky130hd"])
        .output()
        .expect("run otps");
    assert!(fallback.status.success());
    let stdout = String::from_utf8_lossy(&fallback.stdout);
    assert!(!stdout.contains("run mismatch"), "{stdout}");
}

#[test]
fn diff_native_json_reports() {
    let dir = format!("{}/../../fixtures", env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(env!("CARGO_BIN_EXE_otps"))
        .args([
            "diff",
            &format!("{dir}/sta_native_base.json"),
            &format!("{dir}/sta_native_route.json"),
        ])
        .output()
        .expect("run otps");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("match rate: 66.7%"), "{stdout}");
    assert!(stdout.contains("worst slack delta: -0.100 ns"), "{stdout}");
}

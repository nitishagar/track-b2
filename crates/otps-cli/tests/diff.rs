//! CLI end-to-end tests over the synthetic fixtures.

use std::process::Command;

fn fixtures() -> (String, String) {
    let dir = format!("{}/../../fixtures", env!("CARGO_MANIFEST_DIR"));
    (format!("{dir}/base.rpt"), format!("{dir}/route.rpt"))
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

//! Native-JSON fixture tests: synthesized `-format json` pair with the same
//! perturbation narrative as the real-layout text pair (net growth on the
//! long route, hold regression, one added reg-to-output path).

use std::fs::File;
use std::io::BufReader;

use otps_core::{attribute, match_paths, parse_sta_json, Component, Run, Severity};

fn load(name: &str) -> otps_core::TimingReport {
    let path = format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let file = File::open(&path).expect("open fixture");
    parse_sta_json(BufReader::new(file), Run::default()).expect("parse fixture")
}

#[test]
fn native_json_fixtures_validate_clean() {
    let base = load("sta_native_base.json");
    assert_eq!(base.paths.len(), 2);
    assert_eq!(base.skipped_paths, 1);
    let route = load("sta_native_route.json");
    assert_eq!(route.paths.len(), 3);
    assert_eq!(route.skipped_paths, 0);
    for path in base.paths.iter().chain(route.paths.iter()) {
        let validation = path.validate(1e-6);
        assert_eq!(
            validation.severity,
            Severity::Clean,
            "{} {:?}",
            path.endpoint,
            validation.problems
        );
    }
}

#[test]
fn native_json_diff_attributes_perturbations() {
    let base = load("sta_native_base.json");
    let new = load("sta_native_route.json");
    // JSON setup margin (+setup raw) is stored negated (contribution).
    assert!(
        base.paths[0]
            .margin
            .is_some_and(|m| (m + 0.05).abs() < 1e-9),
        "{:?}",
        base.paths[0].margin
    );
    // Output-delay margin is routed to external_delay, negated for setup.
    let out = new
        .paths
        .iter()
        .find(|p| p.endpoint == "u_io/pad5")
        .expect("output path");
    assert_eq!(out.margin, Some(0.0));
    assert!(
        out.external_delay.is_some_and(|e| (e + 0.20).abs() < 1e-9),
        "{:?}",
        out.external_delay
    );
    let matched = match_paths(&base.paths, &new.paths);
    assert_eq!(matched.matches.len(), 2);
    assert_eq!(matched.unmatched_new.len(), 1);
    for m in &matched.matches {
        let attr = attribute(&base.paths[m.base], &new.paths[m.new]);
        assert!(
            attr.residual.abs() < 1e-9,
            "large residual on {}",
            attr.endpoint
        );
        match attr.endpoint.as_str() {
            "u_core/ff2" => {
                assert_eq!(attr.dominant, Component::NetDelay);
                assert!((attr.slack_delta + 0.10).abs() < 1e-12);
                assert!(attr.explained_ratio > 0.95);
            }
            "u_core/ff8" => {
                assert!((attr.slack_delta + 0.01).abs() < 1e-12);
            }
            other => panic!("unexpected match {other}"),
        }
    }
}

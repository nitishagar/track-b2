//! Real-layout fixture tests: synthesized `report_checks` text pair.

use otps_core::{attribute, match_paths, parse_auto, Component, ParseOpts, Run, Severity};

fn load(name: &str) -> otps_core::TimingReport {
    let path = format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("read fixture");
    parse_auto(&text, Run::default(), &ParseOpts::default()).expect("parse fixture")
}

#[test]
fn sta_fixtures_validate_clean() {
    for name in ["sta_base.rpt", "sta_route.rpt"] {
        let report = load(name);
        assert!(!report.paths.is_empty(), "{name} has no paths");
        assert_eq!(report.skipped_paths, 0, "{name} skips");
        for path in &report.paths {
            let validation = path.validate(1e-9);
            assert_eq!(
                validation.severity,
                Severity::Clean,
                "{name}: {path:?} {:?}",
                validation.problems
            );
            assert!(
                path.rounding_ns.is_some(),
                "{name}: rounding inferred for {}",
                path.endpoint
            );
        }
    }
}

#[test]
fn sta_diff_attributes_perturbations() {
    let base = load("sta_base.rpt");
    let new = load("sta_route.rpt");
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
                // Hold regression: arrival fell, slack fell with it.
                assert!((attr.slack_delta + 0.01).abs() < 1e-12);
            }
            other => panic!("unexpected match {other}"),
        }
    }
}

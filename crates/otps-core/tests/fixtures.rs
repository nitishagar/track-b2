//! Fixture tests: pre-route vs post-route diff on synthetic OpenSTA-subset data.

use otps_core::{attribute, cluster, match_paths, parse_text, Component, Run};

fn load(name: &str) -> otps_core::TimingReport {
    let path = format!("{}/../../fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).expect("read fixture");
    parse_text(&text, Run::default()).expect("parse fixture")
}

#[test]
fn fixtures_validate() {
    for name in ["base.rpt", "route.rpt"] {
        let report = load(name);
        assert!(!report.paths.is_empty(), "{name} has no paths");
        for path in &report.paths {
            assert!(path.validate(1e-9).is_empty(), "{name}: {path:?}");
        }
    }
}

#[test]
fn route_diff_match_rate_is_reported() {
    let base = load("base.rpt");
    let new = load("route.rpt");
    let matched = match_paths(&base.paths, &new.paths);
    assert_eq!(matched.matches.len(), 6);
    assert_eq!(matched.unmatched_new.len(), 1);
    assert!((matched.match_rate - 6.0 / 7.0).abs() < 1e-12);
}

#[test]
fn perturbation_test_attributes_net_delay() {
    // Ground truth via controlled perturbation: route.rpt injects net-delay
    // growth on the long u_core routes. The engine must blame net delay with
    // >95% explained ratio on those pairs.
    let base = load("base.rpt");
    let new = load("route.rpt");
    let matched = match_paths(&base.paths, &new.paths);
    let mut net_blamed = 0;
    for m in &matched.matches {
        let attr = attribute(&base.paths[m.base], &new.paths[m.new]);
        assert!(
            attr.residual.abs() < 1e-9,
            "large residual on {}",
            attr.endpoint
        );
        if attr.endpoint == "u_core/ff2" || attr.endpoint == "u_core/ff4" {
            assert_eq!(attr.dominant, Component::NetDelay, "{}", attr.endpoint);
            assert!(attr.explained_ratio > 0.95, "{}", attr.endpoint);
            net_blamed += 1;
        }
        if attr.endpoint == "u_core/ff6" {
            // IO path: data unchanged, capture clock latency moved required.
            assert_eq!(attr.dominant, Component::Required);
        }
    }
    assert_eq!(net_blamed, 2);

    let attrs: Vec<_> = matched
        .matches
        .iter()
        .map(|m| attribute(&base.paths[m.base], &new.paths[m.new]))
        .collect();
    let clusters = cluster(&attrs);
    let top = &clusters[0];
    assert_eq!(top.dominant, Component::NetDelay);
    assert!(top.count >= 2, "net cluster should group the long routes");
}

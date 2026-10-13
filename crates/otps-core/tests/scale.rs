//! B3/D5 scale gates: ingest and fuzzy matching must scale (near-)linearly.
//! Asserts *ratios* (2x input < 3x time), never absolute wall time.

mod common;

use std::time::Instant;

use otps_core::{
    match_paths, parse_sta_text, Check, Edge, ParseOpts, Run, TimingPath, TimingPoint,
};

fn parse_n(n: usize) -> (usize, std::time::Duration) {
    let input = common::synth_input(n);
    let t0 = Instant::now();
    let report =
        parse_sta_text(&input, Run::default(), &ParseOpts::default()).expect("scale parse");
    (report.paths.len(), t0.elapsed())
}

#[test]
fn scale_ratio_8k_vs_4k_under_3x() {
    let (warm_paths, _) = parse_n(4000);
    assert_eq!(warm_paths, 4000);
    let (small_paths, t_small) = parse_n(4000);
    let (large_paths, t_large) = parse_n(8000);
    assert_eq!(small_paths, 4000);
    assert_eq!(large_paths, 8000);
    eprintln!("scale: 4k={t_small:?} 8k={t_large:?}");
    assert!(
        t_large.as_secs_f64() < 3.0 * t_small.as_secs_f64(),
        "8k ({t_large:?}) should take < 3x the 4k time ({t_small:?})"
    );
}

fn scale_path(endpoint: &str, unique: &str) -> TimingPath {
    // Sparse sharing (realistic ECO leftovers): 5 unique pins per path, no
    // bus pins. Each base overlaps only its renamed counterpart.
    let mut pins: Vec<String> = (0..5).map(|b| format!("{unique}_p{b}")).collect();
    pins.push(unique.to_owned());
    TimingPath {
        startpoint: "s".to_owned(),
        endpoint: endpoint.to_owned(),
        launch_clock: "clk".to_owned(),
        capture_clock: "clk".to_owned(),
        launch_edge: Edge::Rise,
        capture_edge: Edge::Rise,
        group: "clk".to_owned(),
        flavor: Some("check".to_owned()),
        check: Check::Setup,
        launch: None,
        capture: None,
        points: pins
            .into_iter()
            .map(|pin| TimingPoint {
                pin,
                cell: String::new(),
                net: String::new(),
                cell_delay: 0.0,
                net_delay: 0.0,
                slew: 0.0,
                cap: 0.0,
                fanout: 0,
                derate: None,
                sigma: None,
                si_delta: None,
                transition: None,
                arrival: None,
                split_available: true,
            })
            .collect(),
        slack: 0.0,
        required: 0.0,
        arrival: 0.0,
        uncertainty: None,
        crpr: None,
        margin: None,
        external_delay: None,
        rounding_ns: None,
    }
}

fn match_fuzzy_n(n: usize) -> (usize, std::time::Duration) {
    let base: Vec<TimingPath> = (0..n)
        .map(|i| scale_path(&format!("ep{i}"), &format!("u{i}")))
        .collect();
    // Renamed endpoints, identical pin sets: all-fuzzy, Jaccard 1.0.
    let new: Vec<TimingPath> = (0..n)
        .map(|i| scale_path(&format!("ep{i}_eco"), &format!("u{i}")))
        .collect();
    let t0 = Instant::now();
    let matched = match_paths(&base, &new);
    (matched.matches.len(), t0.elapsed())
}

#[test]
fn fuzzy_scale_ratio_1k_vs_500_under_3x() {
    let (warm, _) = match_fuzzy_n(500);
    assert_eq!(warm, 500);
    let (small, t_small) = match_fuzzy_n(500);
    let (large, t_large) = match_fuzzy_n(1000);
    assert_eq!(small, 500);
    assert_eq!(large, 1000);
    eprintln!("fuzzy scale: 500={t_small:?} 1k={t_large:?}");
    assert!(
        t_large.as_secs_f64() < 3.0 * t_small.as_secs_f64(),
        "1k ({t_large:?}) should take < 3x the 500 time ({t_small:?})"
    );
}

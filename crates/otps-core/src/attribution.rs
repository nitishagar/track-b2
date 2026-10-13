//! Delta attribution: explain *why* slack moved, part by part.
//!
//! Identity (`s` = check sign, +1 setup / -1 hold):
//! `Δslack_hdr = s·(Δreq_parts − Δarr_parts) + residual`, where part deltas
//! sum every known term: clock edges, clock networks, data stages,
//! uncertainty, CRPR, margin, external delay. Unknown terms contribute 0 and
//! are listed; the residual absorbs the rest. A large residual means
//! "uncertain", never "clean".

use std::cmp::Ordering;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schema::{ClockSegment, Severity, TimingPath, TimingPoint};

/// Dominant contributor to a slack delta.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    CellDelay,
    NetDelay,
    LaunchClock,
    CaptureClock,
    ClockEdge,
    Uncertainty,
    Crpr,
    CheckMargin,
    ExternalDelay,
    Unexplained,
    Balanced,
}

/// Which part of the path a stage belongs to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    LaunchClock,
    Data,
    CaptureClock,
}

/// How a stage paired across the two runs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Matched,
    Added,
    Removed,
}

/// One paired stage and its move. Derate/SI are sub-split explanations of
/// the cell/net move, never added twice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StageDelta {
    pub pin: String,
    pub cell: String,
    pub net: String,
    pub kind: StageKind,
    pub status: StageStatus,
    pub cell_delta: f64,
    pub net_delta: f64,
    /// Paired `si_delta` move, when either side reports it. Informational:
    /// the net move already includes SI effects.
    pub si_delta: Option<f64>,
    pub derate_base: Option<f64>,
    pub derate_new: Option<f64>,
    /// Stage move explained by the derate change:
    /// `incr_new·(1 − derate_base/derate_new)`. `None` when derates are
    /// absent or the new derate is ~0.
    pub derate_effect: Option<f64>,
    pub slew_base: f64,
    pub slew_new: f64,
    pub fanout_base: u32,
    pub fanout_new: u32,
}

/// Confidence in the attribution. Anything but full knowledge reduces it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Reduced(Vec<String>),
}

/// Per-pair attribution. All deltas are `new - base`, in ns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attribution {
    pub startpoint: String,
    pub endpoint: String,
    /// Path group (new side preferred); empty when the input has none.
    pub group: String,
    pub slack_delta: f64,
    pub cell_delta: f64,
    pub net_delta: f64,
    pub data_delta: f64,
    /// Header required move (informative; the residual uses parts, not this).
    pub required_delta: f64,
    /// Launch edge-time move; 0 + listed when either edge is unknown.
    pub launch_edge_delta: f64,
    /// Launch network move (source + network + macro, no edge).
    pub launch_clock_delta: f64,
    /// `Δarr_parts`: launch edge + launch network + data.
    pub arrival_delta: f64,
    pub capture_edge_delta: f64,
    pub capture_clock_delta: f64,
    pub uncertainty_delta: f64,
    pub crpr_delta: f64,
    pub margin_delta: f64,
    pub external_delta: f64,
    /// `Δreq_parts`: capture edge + network + unc + CRPR + margin + external.
    pub required_parts_delta: f64,
    /// `slack_delta − s·(required_parts_delta − arrival_delta)`; near zero
    /// when sound.
    pub residual: f64,
    /// `1 − |residual| / |slack_delta|` (D6 branch at zero); always [0, 1].
    pub explained_ratio: f64,
    pub dominant: Component,
    /// Top-8 matched stages by |move| plus all added/removed, sorted by
    /// (|move| desc, pin, kind, status).
    pub stages: Vec<StageDelta>,
    /// Matched stages beyond the top 8 (their moves still count in totals).
    pub overflow_count: usize,
    /// Part terms unknown on at least one side (sorted, deduped).
    pub unknowns: Vec<String>,
    /// RSS path sigmas (data + clock stages).
    pub sigma_base: f64,
    pub sigma_new: f64,
    /// True when `|residual|` is within 3σ of the RSS'd pair variation.
    pub within_variation: bool,
    pub confidence: Confidence,
}

/// Matched stages carried per pair; added/removed always ride along.
const TOP_K_STAGES: usize = 8;
/// Moves at or below this read as quiet (`Balanced`).
const QUIET_EPS: f64 = 1e-12;
/// Tolerance for the confidence-only validation inside [`attribute`].
const VALIDATE_TOL_NS: f64 = 1e-6;
/// New derates within this of 0 trip the `derate_effect` guard.
const DERATE_GUARD: f64 = 1e-12;

/// All part deltas for one pair (unknown terms read 0 + listed).
struct PartDeltas {
    cell: f64,
    net: f64,
    launch_edge: f64,
    launch_clock: f64,
    capture_edge: f64,
    capture_clock: f64,
    uncertainty: f64,
    crpr: f64,
    margin: f64,
    external: f64,
}

impl PartDeltas {
    fn data(&self) -> f64 {
        self.cell + self.net
    }
    fn arrival(&self) -> f64 {
        self.launch_edge + self.launch_clock + self.data()
    }
    fn required_parts(&self) -> f64 {
        self.capture_edge
            + self.capture_clock
            + self.uncertainty
            + self.crpr
            + self.margin
            + self.external
    }
}

fn part_deltas(base: &TimingPath, new: &TimingPath, unknowns: &mut Vec<String>) -> PartDeltas {
    let data_pairs = pair_stages(&base.points, &new.points);
    let cell: f64 = data_pairs
        .iter()
        .map(|p| delta_of(p, |t| t.cell_delay))
        .sum();
    let net: f64 = data_pairs
        .iter()
        .map(|p| delta_of(p, |t| t.net_delay))
        .sum();
    let (launch_edge, launch_clock) = clock_deltas(
        base.launch.as_ref(),
        new.launch.as_ref(),
        "launch",
        unknowns,
    );
    let (capture_edge, capture_clock) = clock_deltas(
        base.capture.as_ref(),
        new.capture.as_ref(),
        "capture",
        unknowns,
    );
    PartDeltas {
        cell,
        net,
        launch_edge,
        launch_clock,
        capture_edge,
        capture_clock,
        uncertainty: opt_delta(base.uncertainty, new.uncertainty, "uncertainty", unknowns),
        margin: opt_delta(base.margin, new.margin, "check margin", unknowns),
        // Absent CRPR/external delay mean "off" (= 0), never unknown.
        crpr: new.crpr.unwrap_or(0.0) - base.crpr.unwrap_or(0.0),
        external: new.external_delay.unwrap_or(0.0) - base.external_delay.unwrap_or(0.0),
    }
}

/// Top-8 matched stages plus all added/removed, with the overflow count.
fn collect_stages(
    base: &TimingPath,
    new: &TimingPath,
    unknowns: &mut Vec<String>,
) -> (Vec<StageDelta>, usize) {
    let mut matched: Vec<StageDelta> = Vec::new();
    let mut edge: Vec<StageDelta> = Vec::new();
    for pair in pair_stages(&base.points, &new.points) {
        push_stage(&pair, StageKind::Data, &mut matched, &mut edge);
    }
    for (side, base_seg, new_seg, kind) in [
        (
            "launch",
            base.launch.as_ref(),
            new.launch.as_ref(),
            StageKind::LaunchClock,
        ),
        (
            "capture",
            base.capture.as_ref(),
            new.capture.as_ref(),
            StageKind::CaptureClock,
        ),
    ] {
        if let (Some(b), Some(n)) = (base_seg, new_seg) {
            match (expanded_stages(b), expanded_stages(n)) {
                (Some(bs), Some(ns)) => {
                    for pair in pair_stages(bs, ns) {
                        push_stage(&pair, kind, &mut matched, &mut edge);
                    }
                }
                (None, None) => {}
                _ => unknowns.push(format!("{side} clock stages")),
            }
        }
    }
    matched.sort_by(stage_order);
    let overflow = matched.len().saturating_sub(TOP_K_STAGES);
    matched.truncate(TOP_K_STAGES);
    let mut stages = matched;
    stages.append(&mut edge);
    stages.sort_by(stage_order);
    (stages, overflow)
}

/// Loudest part move wins; Unexplained wins ties; quiet reads Balanced.
fn pick_dominant(parts: &PartDeltas, residual: f64) -> Component {
    let mut best = residual.abs();
    let mut dominant = Component::Unexplained;
    for (comp, mag) in [
        (Component::CellDelay, parts.cell.abs()),
        (Component::NetDelay, parts.net.abs()),
        (Component::LaunchClock, parts.launch_clock.abs()),
        (Component::CaptureClock, parts.capture_clock.abs()),
        (
            Component::ClockEdge,
            parts.launch_edge.abs() + parts.capture_edge.abs(),
        ),
        (Component::Uncertainty, parts.uncertainty.abs()),
        (Component::Crpr, parts.crpr.abs()),
        (Component::CheckMargin, parts.margin.abs()),
        (Component::ExternalDelay, parts.external.abs()),
    ] {
        if mag > best {
            best = mag;
            dominant = comp;
        }
    }
    if best <= QUIET_EPS {
        dominant = Component::Balanced;
    }
    dominant
}

/// Attribute one matched pair. The pair must share its check class (the
/// matcher guarantees this); the header slack sign flows through `s`.
#[must_use]
pub fn attribute(base: &TimingPath, new: &TimingPath) -> Attribution {
    let mut unknowns: Vec<String> = Vec::new();
    let parts = part_deltas(base, new, &mut unknowns);

    let slack_delta = new.slack - base.slack;
    let required_delta = new.required - base.required;
    let residual = slack_delta - base.check.sign() * (parts.required_parts() - parts.arrival());
    // Near-zero slack movement has no meaningful ratio denominator: judge the
    // residual on an absolute scale instead, and always clamp to [0, 1].
    let explained_ratio = if slack_delta.abs() < 1e-9 {
        if residual.abs() < 1e-9 {
            1.0
        } else {
            0.0
        }
    } else {
        (1.0 - residual.abs() / slack_delta.abs()).clamp(0.0, 1.0)
    };

    let (stages, overflow_count) = collect_stages(base, new, &mut unknowns);
    unknowns.sort();
    unknowns.dedup();

    let sigma_base = path_sigma(base);
    let sigma_new = path_sigma(new);
    let sigma_pair = (sigma_base * sigma_base + sigma_new * sigma_new).sqrt();
    let within_variation = residual.abs() < 3.0 * sigma_pair;

    let mut reasons: Vec<String> = unknowns.iter().map(|u| format!("unknown: {u}")).collect();
    for (label, path) in [("base", base), ("new", new)] {
        let validation = path.validate(VALIDATE_TOL_NS);
        match validation.severity {
            Severity::Clean => {}
            Severity::Warn => {
                reasons.push(format!("{label} warns: {}", validation.problems.join("; ")));
            }
            Severity::Reject => reasons.push(format!(
                "{label} rejects: {}",
                validation.problems.join("; ")
            )),
        }
    }
    let confidence = if reasons.is_empty() {
        Confidence::High
    } else {
        Confidence::Reduced(reasons)
    };

    let dominant = pick_dominant(&parts, residual);

    Attribution {
        startpoint: base.startpoint.clone(),
        endpoint: base.endpoint.clone(),
        group: if new.group.is_empty() {
            base.group.clone()
        } else {
            new.group.clone()
        },
        slack_delta,
        cell_delta: parts.cell,
        net_delta: parts.net,
        data_delta: parts.data(),
        required_delta,
        launch_edge_delta: parts.launch_edge,
        launch_clock_delta: parts.launch_clock,
        arrival_delta: parts.arrival(),
        capture_edge_delta: parts.capture_edge,
        capture_clock_delta: parts.capture_clock,
        uncertainty_delta: parts.uncertainty,
        crpr_delta: parts.crpr,
        margin_delta: parts.margin,
        external_delta: parts.external,
        required_parts_delta: parts.required_parts(),
        residual,
        explained_ratio,
        dominant,
        stages,
        overflow_count,
        unknowns,
        sigma_base,
        sigma_new,
        within_variation,
        confidence,
    }
}

/// Edge + network deltas for one clock side. Canonical totals compare even
/// across condensed/expanded pairs; missing terms read 0 + listed.
fn clock_deltas(
    base: Option<&ClockSegment>,
    new: Option<&ClockSegment>,
    side: &str,
    unknowns: &mut Vec<String>,
) -> (f64, f64) {
    if let (Some(b), Some(n)) = (base, new) {
        let edge = if let (Some(a), Some(c)) = (b.edge_time, n.edge_time) {
            c - a
        } else {
            unknowns.push(format!("{side} edge time"));
            0.0
        };
        let network = (n.source_latency + n.network_delay + n.macro_delay)
            - (b.source_latency + b.network_delay + b.macro_delay);
        (edge, network)
    } else {
        unknowns.push(format!("{side} clock path"));
        (0.0, 0.0)
    }
}

/// Delta of an `Option` term that is unknown (not zero) when absent.
fn opt_delta(base: Option<f64>, new: Option<f64>, name: &str, unknowns: &mut Vec<String>) -> f64 {
    if let (Some(b), Some(n)) = (base, new) {
        n - b
    } else {
        unknowns.push(name.to_owned());
        0.0
    }
}

/// `Some` stages only when the segment is genuinely expanded.
fn expanded_stages(seg: &ClockSegment) -> Option<&[TimingPoint]> {
    if seg.condensed || seg.stages.is_empty() {
        None
    } else {
        Some(&seg.stages)
    }
}

struct StagePair<'a> {
    base: Option<&'a TimingPoint>,
    new: Option<&'a TimingPoint>,
}

/// Pair stages FIFO by pin. Leftover base stages read as removed, leftover
/// new stages as added.
fn pair_stages<'a>(base: &'a [TimingPoint], new: &'a [TimingPoint]) -> Vec<StagePair<'a>> {
    let mut base_by_pin: HashMap<&str, Vec<&'a TimingPoint>> = HashMap::new();
    for p in base {
        base_by_pin.entry(p.pin.as_str()).or_default().push(p);
    }
    for stages in base_by_pin.values_mut() {
        stages.reverse();
    }
    let mut pairs = Vec::new();
    for p in new {
        let b = base_by_pin.get_mut(p.pin.as_str()).and_then(Vec::pop);
        pairs.push(StagePair {
            base: b,
            new: Some(p),
        });
    }
    let mut leftover: Vec<(&str, &'a TimingPoint)> = base_by_pin
        .into_iter()
        .flat_map(|(pin, stages)| stages.into_iter().map(move |p| (pin, p)))
        .collect();
    leftover.sort_by(|a, b| a.0.cmp(b.0));
    for (_, p) in leftover {
        pairs.push(StagePair {
            base: Some(p),
            new: None,
        });
    }
    pairs
}

fn delta_of(pair: &StagePair, field: impl Fn(&TimingPoint) -> f64) -> f64 {
    pair.new.map_or(0.0, &field) - pair.base.map_or(0.0, &field)
}

fn stage_delta(pair: &StagePair, kind: StageKind) -> StageDelta {
    let (status, b, n) = match (pair.base, pair.new) {
        (Some(b), Some(n)) => (StageStatus::Matched, Some(b), Some(n)),
        (None, Some(n)) => (StageStatus::Added, None, Some(n)),
        (Some(b), None) => (StageStatus::Removed, Some(b), None),
        (None, None) => unreachable!("pair always has a side"),
    };
    let cell = |p: Option<&TimingPoint>| p.map_or(0.0, |t| t.cell_delay);
    let net = |p: Option<&TimingPoint>| p.map_or(0.0, |t| t.net_delay);
    let si_delta = match (b.and_then(|t| t.si_delta), n.and_then(|t| t.si_delta)) {
        (None, None) => None,
        (sb, sn) => Some(sn.unwrap_or(0.0) - sb.unwrap_or(0.0)),
    };
    let derate_effect = match (b.and_then(|t| t.derate), n.and_then(|t| t.derate)) {
        (Some(db), Some(dn)) if dn.abs() > DERATE_GUARD => {
            let point = n.expect("new side carries the new derate");
            Some((point.cell_delay + point.net_delay) * (1.0 - db / dn))
        }
        _ => None,
    };
    let named = n.or(b).expect("pair always has a side");
    StageDelta {
        pin: named.pin.clone(),
        cell: named.cell.clone(),
        net: named.net.clone(),
        kind,
        status,
        cell_delta: cell(n) - cell(b),
        net_delta: net(n) - net(b),
        si_delta,
        derate_base: b.and_then(|t| t.derate),
        derate_new: n.and_then(|t| t.derate),
        derate_effect,
        slew_base: b.map_or(0.0, |t| t.slew),
        slew_new: n.map_or(0.0, |t| t.slew),
        fanout_base: b.map_or(0, |t| t.fanout),
        fanout_new: n.map_or(0, |t| t.fanout),
    }
}

fn push_stage(
    pair: &StagePair,
    kind: StageKind,
    matched: &mut Vec<StageDelta>,
    edge: &mut Vec<StageDelta>,
) {
    let stage = stage_delta(pair, kind);
    if stage.status == StageStatus::Matched {
        matched.push(stage);
    } else {
        edge.push(stage);
    }
}

fn stage_order(a: &StageDelta, b: &StageDelta) -> Ordering {
    let ma = (a.cell_delta + a.net_delta).abs();
    let mb = (b.cell_delta + b.net_delta).abs();
    mb.partial_cmp(&ma)
        .unwrap_or(Ordering::Equal)
        .then(a.pin.cmp(&b.pin))
        .then(a.kind.cmp(&b.kind))
        .then(a.status.cmp(&b.status))
}

/// RSS path sigma over data + clock stages.
fn path_sigma(path: &TimingPath) -> f64 {
    path.points
        .iter()
        .chain(
            path.launch
                .as_ref()
                .map_or(&[] as &[TimingPoint], |s| &s.stages),
        )
        .chain(
            path.capture
                .as_ref()
                .map_or(&[] as &[TimingPoint], |s| &s.stages),
        )
        .filter_map(|p| p.sigma)
        .map(|s| s * s)
        .sum::<f64>()
        .sqrt()
}

/// A cluster of paths sharing a root cause.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cluster {
    pub dominant: Component,
    /// Group name, or the endpoint hierarchy prefix when paths are ungrouped.
    pub prefix: String,
    pub count: usize,
    pub total_slack_delta: f64,
    pub members: Vec<String>,
}

/// Group attributions by dominant component + group (or endpoint hierarchy
/// prefix when the input carries no group).
#[must_use]
pub fn cluster(attributions: &[Attribution]) -> Vec<Cluster> {
    let mut groups: HashMap<(Component, String), Vec<&Attribution>> = HashMap::new();
    for attr in attributions {
        let key = if attr.group.is_empty() {
            prefix_of(&attr.endpoint)
        } else {
            attr.group.clone()
        };
        groups.entry((attr.dominant, key)).or_default().push(attr);
    }
    let mut out: Vec<Cluster> = groups
        .into_iter()
        .map(|((dominant, prefix), members)| Cluster {
            dominant,
            prefix,
            count: members.len(),
            total_slack_delta: members.iter().map(|m| m.slack_delta).sum(),
            members: members.iter().map(|m| m.endpoint.clone()).collect(),
        })
        .collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(
                b.total_slack_delta
                    .abs()
                    .partial_cmp(&a.total_slack_delta.abs())
                    .unwrap_or(Ordering::Equal),
            )
            // Total tiebreak: cluster order must be deterministic across runs.
            .then(a.dominant.cmp(&b.dominant))
            .then(a.prefix.cmp(&b.prefix))
    });
    out
}

/// Parent hierarchy of the endpoint (leaf dropped), capped at two segments.
/// `u_core/ff2` groups under `u_core`; `top/mid/low/ff` under `top/mid`.
fn prefix_of(endpoint: &str) -> String {
    let parent = endpoint.rsplit_once('/').map_or(endpoint, |(p, _)| p);
    let mut parts = parent.split('/');
    match (parts.next(), parts.next()) {
        (Some(a), Some(b)) => format!("{a}/{b}"),
        (Some(a), None) => a.to_owned(),
        (None, _) => endpoint.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Check, ClockSegment, Edge, TimingPoint};

    fn point(pin: &str, cell: f64, net: f64) -> TimingPoint {
        TimingPoint {
            pin: pin.to_owned(),
            cell: String::new(),
            net: String::new(),
            cell_delay: cell,
            net_delay: net,
            slew: 0.0,
            cap: 0.0,
            fanout: 0,
            derate: None,
            sigma: None,
            si_delta: None,
            transition: None,
            arrival: None,
            split_available: true,
        }
    }

    fn path(slack: f64, required: f64, points: Vec<TimingPoint>) -> TimingPath {
        let arrival: f64 = points.iter().map(|p| p.cell_delay + p.net_delay).sum();
        TimingPath {
            startpoint: "a".to_owned(),
            endpoint: "u_core/b".to_owned(),
            launch_clock: "clk".to_owned(),
            capture_clock: "clk".to_owned(),
            launch_edge: Edge::Unknown,
            capture_edge: Edge::Unknown,
            group: String::new(),
            flavor: None,
            check: Check::Setup,
            launch: None,
            capture: None,
            slack,
            required,
            arrival,
            uncertainty: None,
            crpr: None,
            margin: None,
            external_delay: None,
            rounding_ns: None,
            points,
        }
    }

    fn seg(edge: Option<f64>, network: f64) -> ClockSegment {
        ClockSegment {
            clock: "clk".to_owned(),
            edge: Edge::Rise,
            edge_time: edge,
            source_latency: 0.0,
            network_delay: network,
            macro_delay: 0.0,
            condensed: true,
            stages: Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn full(
        check: Check,
        slack: f64,
        required: f64,
        arrival: f64,
        unc: Option<f64>,
        margin: Option<f64>,
        launch: Option<ClockSegment>,
        capture: Option<ClockSegment>,
        points: Vec<TimingPoint>,
    ) -> TimingPath {
        TimingPath {
            startpoint: "a".to_owned(),
            endpoint: "u_core/b".to_owned(),
            launch_clock: "clk".to_owned(),
            capture_clock: "clk".to_owned(),
            launch_edge: Edge::Rise,
            capture_edge: Edge::Rise,
            group: "clk".to_owned(),
            flavor: None,
            check,
            launch,
            capture,
            slack,
            required,
            arrival,
            uncertainty: unc,
            crpr: None,
            margin,
            external_delay: None,
            rounding_ns: None,
            points,
        }
    }

    #[test]
    fn blames_injected_net_perturbation() {
        let base = path(0.2, 1.0, vec![point("p1", 0.5, 0.3)]);
        // Inject +0.1 net delay; arrival rises to 0.9, slack falls to 0.1.
        let new = path(0.1, 1.0, vec![point("p1", 0.5, 0.4)]);
        let attr = attribute(&base, &new);
        assert_eq!(attr.dominant, Component::NetDelay);
        assert!((attr.slack_delta + 0.1).abs() < 1e-12);
        assert!(attr.residual.abs() < 1e-12);
        assert!(attr.explained_ratio > 0.95);
    }

    #[test]
    fn explained_ratio_stays_bounded_near_zero_delta() {
        // Zero slack movement + sub-tolerance point noise must not explode.
        let base = path(0.2, 1.0, vec![point("p1", 0.5, 0.3)]);
        let new = path(0.2, 1.0, vec![point("p1", 0.5, 0.3 + 1e-7)]);
        let attr = attribute(&base, &new);
        assert!((0.0..=1.0).contains(&attr.explained_ratio));
    }

    #[test]
    fn sign_matrix_setup_hold_met_violated() {
        // (check, data cell, unc, margin_base, margin_new): headers recomputed
        // from parts so the residual must be exactly 0 in all four cells.
        let cases = [
            (Check::Setup, 0.80, -0.05, -0.05, -0.06), // MET (+0.10)
            (Check::Setup, 1.00, -0.05, -0.05, -0.06), // VIOLATED (-0.10)
            (Check::Hold, 0.10, 0.02, 0.01, 0.02),     // MET (+0.07)
            (Check::Hold, 0.01, 0.02, 0.01, 0.02),     // VIOLATED (-0.02)
        ];
        for (check, data, unc, m_base, m_new) in cases {
            let edge = if check == Check::Setup { 1.0 } else { 0.0 };
            let mk = |margin: f64| {
                let required = edge + unc + margin;
                let slack = check.slack(data, required);
                full(
                    check,
                    slack,
                    required,
                    data,
                    Some(unc),
                    Some(margin),
                    Some(seg(Some(0.0), 0.0)),
                    Some(seg(Some(edge), 0.0)),
                    vec![point("d", data, 0.0)],
                )
            };
            let attr = attribute(&mk(m_base), &mk(m_new));
            assert!(attr.residual.abs() < 1e-12, "{check:?} residual");
            assert!((attr.explained_ratio - 1.0).abs() < 1e-9, "{check:?} ratio");
            assert_eq!(attr.dominant, Component::CheckMargin, "{check:?} dominant");
            assert_eq!(attr.confidence, Confidence::High, "{check:?} confidence");
        }
    }

    #[test]
    fn inserted_stage_names_pinned() {
        let base = path(0.2, 1.0, vec![point("p1", 0.5, 0.3)]);
        let new = path(
            -0.1,
            1.0,
            vec![point("p1", 0.5, 0.3), point("p2", 0.1, 0.2)],
        );
        let attr = attribute(&base, &new);
        let added: Vec<_> = attr
            .stages
            .iter()
            .filter(|s| s.status == StageStatus::Added)
            .collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].pin, "p2");
        assert!((added[0].net_delta - 0.2).abs() < 1e-12);
        assert_eq!(attr.overflow_count, 0);

        let attr = attribute(&new, &base);
        let removed: Vec<_> = attr
            .stages
            .iter()
            .filter(|s| s.status == StageStatus::Removed)
            .collect();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].pin, "p2");
    }

    #[test]
    fn condensed_consistent_scalar_explained() {
        let mk_capture = |condensed: bool| {
            if condensed {
                seg(Some(1.0), 0.05)
            } else {
                ClockSegment {
                    clock: "clk".to_owned(),
                    edge: Edge::Rise,
                    edge_time: Some(1.0),
                    source_latency: 0.0,
                    network_delay: 0.05,
                    macro_delay: 0.0,
                    condensed: false,
                    stages: vec![point("ck", 0.02, 0.03)],
                }
            }
        };
        let mk = |condensed: bool| {
            full(
                Check::Setup,
                0.00,
                0.90,
                0.90,
                Some(-0.05),
                Some(-0.10),
                Some(seg(Some(0.0), 0.0)),
                Some(mk_capture(condensed)),
                vec![point("d", 0.9, 0.0)],
            )
        };
        // Headers: req = 1.0 + 0.05 - 0.05 - 0.10 = 0.90; arr = 0.90.
        let attr = attribute(&mk(true), &mk(false));
        assert!(attr.residual.abs() < 1e-12);
        assert!((attr.explained_ratio - 1.0).abs() < 1e-12);
        assert_eq!(attr.unknowns, vec!["capture clock stages".to_owned()]);
        assert!(matches!(attr.confidence, Confidence::Reduced(_)));
        assert_eq!(attr.dominant, Component::Balanced);
    }

    #[test]
    fn condensed_unknown_edge_unexplained() {
        // Binary-exact numbers so the residual ties the capture move exactly.
        let mk = |edge: Option<f64>, network: f64, required: f64| {
            full(
                Check::Setup,
                required - 0.50,
                required,
                0.50,
                Some(-0.25),
                Some(-0.25),
                Some(seg(Some(0.0), 0.0)),
                Some(ClockSegment {
                    clock: "clk".to_owned(),
                    edge: Edge::Rise,
                    edge_time: edge,
                    source_latency: 0.0,
                    network_delay: network,
                    macro_delay: 0.0,
                    condensed: edge.is_some(),
                    stages: if edge.is_some() {
                        Vec::new()
                    } else {
                        vec![point("ck", 0.125, 0.125)]
                    },
                }),
                vec![point("d", 0.5, 0.0)],
            )
        };
        // Headers move +0.50; parts see only +0.25 (edge unknown).
        let attr = attribute(&mk(Some(1.0), 0.0, 0.50), &mk(None, 0.25, 1.00));
        assert!((attr.residual - 0.25).abs() < 1e-12);
        assert!((attr.explained_ratio - 0.5).abs() < 1e-12);
        // Residual ties the capture move: Unexplained wins ties.
        assert_eq!(attr.dominant, Component::Unexplained);
        assert!(attr.unknowns.contains(&"capture edge time".to_owned()));
        assert!(attr.unknowns.contains(&"capture clock stages".to_owned()));
    }

    #[test]
    fn zero_delta_identical_paths() {
        let p = full(
            Check::Setup,
            0.00,
            0.90,
            0.90,
            Some(-0.05),
            Some(-0.05),
            Some(seg(Some(0.0), 0.0)),
            Some(seg(Some(1.0), 0.0)),
            vec![point("d", 0.9, 0.0)],
        );
        let attr = attribute(&p, &p);
        assert!(attr.residual.abs() < 1e-12);
        assert!((attr.explained_ratio - 1.0).abs() < 1e-12);
        assert_eq!(attr.dominant, Component::Balanced);
        assert_eq!(attr.confidence, Confidence::High);
        assert!(attr.unknowns.is_empty());
    }

    #[test]
    fn within_variation_flag() {
        let mk = |sigma: f64, unc_new: Option<f64>, required: f64| {
            let mut base_pt = point("d", 0.9, 0.0);
            base_pt.sigma = Some(sigma);
            let mut new_pt = point("d", 0.9, 0.0);
            new_pt.sigma = Some(sigma);
            let base = full(
                Check::Setup,
                0.00,
                0.90,
                0.90,
                Some(-0.05),
                Some(-0.05),
                Some(seg(Some(0.0), 0.0)),
                Some(seg(Some(1.0), 0.0)),
                vec![base_pt],
            );
            let new = full(
                Check::Setup,
                required - 0.90,
                required,
                0.90,
                unc_new,
                Some(-0.05),
                Some(seg(Some(0.0), 0.0)),
                Some(seg(Some(1.0), 0.0)),
                vec![new_pt],
            );
            attribute(&base, &new)
        };
        // Headers move +0.05 as if unc lifted; new unc unreported → residual.
        let attr = mk(0.05, None, 0.95);
        assert!((attr.residual - 0.05).abs() < 1e-12);
        assert!(attr.within_variation);
        let attr = mk(0.001, None, 0.95);
        assert!(!attr.within_variation);
    }

    #[test]
    fn derate_effect_computed_with_guard() {
        let mut b = point("p1", 0.5, 0.5);
        b.derate = Some(1.0);
        let mut n = point("p1", 0.55, 0.55);
        n.derate = Some(1.1);
        let attr = attribute(&path(0.0, 1.0, vec![b]), &path(0.0, 1.0, vec![n]));
        assert_eq!(attr.stages.len(), 1);
        let effect = attr.stages[0].derate_effect.expect("effect");
        assert!((effect - 1.1 * (1.0 - 1.0 / 1.1)).abs() < 1e-12);

        let mut z = point("p1", 0.55, 0.55);
        z.derate = Some(0.0);
        let mut b2 = point("p1", 0.5, 0.5);
        b2.derate = Some(1.0);
        let attr = attribute(&path(0.0, 1.0, vec![b2]), &path(0.0, 1.0, vec![z]));
        assert_eq!(attr.stages[0].derate_effect, None);
    }

    #[test]
    fn si_delta_reported_without_double_count() {
        let mut b = point("p1", 0.5, 0.3);
        b.si_delta = Some(0.01);
        let mut n = point("p1", 0.5, 0.3);
        n.si_delta = Some(0.03);
        let attr = attribute(&path(0.2, 1.0, vec![b]), &path(0.2, 1.0, vec![n]));
        assert!(attr.net_delta.abs() < 1e-12);
        assert!((attr.stages[0].si_delta.expect("si") - 0.02).abs() < 1e-12);
    }

    #[test]
    fn confidence_reduced_on_warn() {
        // Known arrival parts disagree with the header: Warn, not Reject.
        let bad = full(
            Check::Setup,
            0.1,
            1.0,
            0.9,
            None,
            None,
            Some(seg(Some(0.0), 0.0)),
            None,
            vec![point("p1", 0.5, 0.3)],
        );
        let attr = attribute(&bad, &bad);
        match &attr.confidence {
            Confidence::Reduced(reasons) => {
                assert!(
                    reasons.iter().any(|r| r.contains("base warns")),
                    "{reasons:?}"
                );
                assert!(
                    reasons.iter().any(|r| r.contains("new warns")),
                    "{reasons:?}"
                );
            }
            Confidence::High => panic!("warn paths must reduce confidence"),
        }
    }

    #[test]
    fn tied_clusters_order_deterministically() {
        let mk = |ep: &str, dom: Component| Attribution {
            startpoint: "s".to_owned(),
            endpoint: ep.to_owned(),
            group: String::new(),
            slack_delta: -0.01,
            cell_delta: 0.0,
            net_delta: 0.0,
            data_delta: 0.0,
            required_delta: 0.0,
            launch_edge_delta: 0.0,
            launch_clock_delta: 0.0,
            arrival_delta: 0.0,
            capture_edge_delta: 0.0,
            capture_clock_delta: 0.0,
            uncertainty_delta: 0.0,
            crpr_delta: 0.0,
            margin_delta: 0.0,
            external_delta: 0.0,
            required_parts_delta: 0.0,
            residual: 0.0,
            explained_ratio: 1.0,
            dominant: dom,
            stages: Vec::new(),
            overflow_count: 0,
            unknowns: Vec::new(),
            sigma_base: 0.0,
            sigma_new: 0.0,
            within_variation: false,
            confidence: Confidence::High,
        };
        // Two clusters tied on count and |total|: order must be stable.
        let attrs = vec![
            mk("b/x", Component::CaptureClock),
            mk("a/y", Component::NetDelay),
        ];
        let first = cluster(&attrs);
        for _ in 0..50 {
            assert_eq!(cluster(&attrs), first);
        }
        assert_eq!(first[0].dominant, Component::NetDelay);
    }

    #[test]
    fn clusters_by_component_and_prefix() {
        let base = path(0.2, 1.0, vec![point("p1", 0.5, 0.3)]);
        let new = path(0.1, 1.0, vec![point("p1", 0.5, 0.4)]);
        let attr = attribute(&base, &new);
        let clusters = cluster(&[attr]);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].dominant, Component::NetDelay);
        assert_eq!(clusters[0].prefix, "u_core");
    }

    #[test]
    fn clusters_by_group_when_grouped() {
        let mut base = path(0.2, 1.0, vec![point("p1", 0.5, 0.3)]);
        base.group = "clk".to_owned();
        let mut new = path(0.1, 1.0, vec![point("p1", 0.5, 0.4)]);
        new.group = "clk".to_owned();
        let attr = attribute(&base, &new);
        assert_eq!(attr.group, "clk");
        let clusters = cluster(&[attr]);
        assert_eq!(clusters[0].prefix, "clk");
    }
}

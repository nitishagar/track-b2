//! Delta attribution: explain *why* slack moved, stage by stage.
//!
//! Identity: `slack = required - arrival`, so for a matched pair
//! `Δslack = Δrequired - Δarrival` where `Δarrival = Σcell Δ + Σnet Δ`.
//! The residual (unexplained part) is always reported; a large residual
//! means "uncertain", never "clean".

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::schema::TimingPath;

/// Dominant contributor to a slack delta.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    CellDelay,
    NetDelay,
    Required,
    Balanced,
}

/// Per-pair attribution. All deltas are `new - base`, in ns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attribution {
    pub startpoint: String,
    pub endpoint: String,
    pub slack_delta: f64,
    pub cell_delta: f64,
    pub net_delta: f64,
    pub data_delta: f64,
    pub required_delta: f64,
    /// `slack_delta - (required_delta - data_delta)`; near zero when sound.
    pub residual: f64,
    /// `1 - |residual| / max(|slack_delta|, eps)`; 1.0 when fully explained.
    pub explained_ratio: f64,
    pub dominant: Component,
}

/// Attribute one matched pair.
#[must_use]
pub fn attribute(base: &TimingPath, new: &TimingPath) -> Attribution {
    let cell_delta: f64 = paired_sum(&base.points, &new.points, |p| p.cell_delay);
    let net_delta: f64 = paired_sum(&base.points, &new.points, |p| p.net_delay);
    let data_delta = cell_delta + net_delta;
    let required_delta = new.required - base.required;
    let slack_delta = new.slack - base.slack;
    let residual = slack_delta - (required_delta - data_delta);
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

    let mut dominant = Component::Balanced;
    let mut best = 1e-12;
    for (comp, mag) in [
        (Component::CellDelay, cell_delta.abs()),
        (Component::NetDelay, net_delta.abs()),
        (Component::Required, required_delta.abs()),
    ] {
        if mag > best {
            best = mag;
            dominant = comp;
        }
    }

    Attribution {
        startpoint: base.startpoint.clone(),
        endpoint: base.endpoint.clone(),
        slack_delta,
        cell_delta,
        net_delta,
        data_delta,
        required_delta,
        residual,
        explained_ratio,
        dominant,
    }
}

/// Sum of per-stage `new - base` for a point field, pairing stages by pin.
/// Unpaired stages count in full on their own side.
fn paired_sum(
    base: &[crate::schema::TimingPoint],
    new: &[crate::schema::TimingPoint],
    field: impl Fn(&crate::schema::TimingPoint) -> f64,
) -> f64 {
    let mut base_by_pin: HashMap<&str, f64> = HashMap::new();
    for p in base {
        *base_by_pin.entry(p.pin.as_str()).or_insert(0.0) += field(p);
    }
    let mut delta = 0.0;
    for p in new {
        let b = base_by_pin.remove(p.pin.as_str()).unwrap_or(0.0);
        delta += field(p) - b;
    }
    for leftover in base_by_pin.values() {
        delta -= leftover;
    }
    delta
}

/// A cluster of paths sharing a root cause.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cluster {
    pub dominant: Component,
    pub prefix: String,
    pub count: usize,
    pub total_slack_delta: f64,
    pub members: Vec<String>,
}

/// Group attributions by dominant component + endpoint hierarchy prefix.
#[must_use]
pub fn cluster(attributions: &[Attribution]) -> Vec<Cluster> {
    let mut groups: HashMap<(Component, String), Vec<&Attribution>> = HashMap::new();
    for attr in attributions {
        groups
            .entry((attr.dominant, prefix_of(&attr.endpoint)))
            .or_default()
            .push(attr);
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
                    .unwrap_or(std::cmp::Ordering::Equal),
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
    use crate::schema::{Check, TimingPoint};

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
        }
    }

    fn path(slack: f64, required: f64, points: Vec<TimingPoint>) -> TimingPath {
        let arrival: f64 = points.iter().map(|p| p.cell_delay + p.net_delay).sum();
        TimingPath {
            startpoint: "a".to_owned(),
            endpoint: "u_core/b".to_owned(),
            launch_clock: "clk".to_owned(),
            capture_clock: "clk".to_owned(),
            group: String::new(),
            check: Check::Setup,
            slack,
            required,
            arrival,
            crpr: 0.0,
            uncertainty: 0.0,
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
    fn tied_clusters_order_deterministically() {
        let mk = |ep: &str, dom: Component| Attribution {
            startpoint: "s".to_owned(),
            endpoint: ep.to_owned(),
            slack_delta: -0.01,
            cell_delta: 0.0,
            net_delta: 0.0,
            data_delta: 0.0,
            required_delta: 0.0,
            residual: 0.0,
            explained_ratio: 1.0,
            dominant: dom,
        };
        // Two clusters tied on count and |total|: order must be stable.
        let attrs = vec![
            mk("b/x", Component::Required),
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
}

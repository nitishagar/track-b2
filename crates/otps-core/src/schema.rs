//! Open Timing Path Schema (OTPS) v0.1: versioned, tool-neutral timing-path model.
//!
//! All delays are in nanoseconds unless a field name says otherwise.

use serde::{Deserialize, Serialize};

/// Schema version. Bump on any breaking change to [`TimingReport`] JSON.
pub const SCHEMA_VERSION: &str = "0.1.0";

/// One timing-analysis run: the tool/corner/mode/PDK/flow identity of a report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Run {
    /// Timing engine, e.g. `opensta`. Never a commercial-tool comparison label.
    pub tool: String,
    pub version: String,
    /// Process corner, e.g. `slow`.
    pub corner: String,
    /// Analysis mode, e.g. `func`.
    pub mode: String,
    /// PDK identifier, e.g. `sky130hd`.
    pub pdk: String,
    /// Content hash of the flow/scripts that produced this run.
    pub flow_hash: String,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            tool: "opensta".to_owned(),
            version: "unknown".to_owned(),
            corner: "unknown".to_owned(),
            mode: "unknown".to_owned(),
            pdk: "unknown".to_owned(),
            flow_hash: "unknown".to_owned(),
        }
    }
}

/// Timing check type. `max` maps to [`Check::Setup`], `min` to [`Check::Hold`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Check {
    Setup,
    Hold,
}

/// One stage (pin) on a timing path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingPoint {
    pub pin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cell: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub net: String,
    /// Cell (gate) incremental delay, ns.
    #[serde(default)]
    pub cell_delay: f64,
    /// Net (wire) incremental delay, ns.
    #[serde(default)]
    pub net_delay: f64,
    #[serde(default)]
    pub slew: f64,
    #[serde(default)]
    pub cap: f64,
    #[serde(default)]
    pub fanout: u32,
    /// Applied derate multiplier, if the report carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derate: Option<f64>,
    /// POCV/LVF sigma, ns, if statistical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sigma: Option<f64>,
    /// SI-induced delta delay, ns, if reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub si_delta: Option<f64>,
}

impl TimingPoint {
    /// Incremental stage delay: cell + net.
    #[must_use]
    pub fn incr(&self) -> f64 {
        self.cell_delay + self.net_delay
    }
}

/// One timing path with its stages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingPath {
    pub startpoint: String,
    pub endpoint: String,
    pub launch_clock: String,
    pub capture_clock: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    pub check: Check,
    /// Slack in ns: `required - arrival`.
    pub slack: f64,
    pub required: f64,
    pub arrival: f64,
    /// Clock reconvergence pessimism removal credit, ns.
    #[serde(default)]
    pub crpr: f64,
    /// Clock uncertainty applied to required time, ns.
    #[serde(default)]
    pub uncertainty: f64,
    pub points: Vec<TimingPoint>,
}

impl TimingPath {
    /// Sum of stage increments; must equal `arrival` for a well-formed path.
    #[must_use]
    pub fn data_arrival(&self) -> f64 {
        self.points.iter().map(TimingPoint::incr).sum()
    }

    /// Self-consistency invariants. Empty means the path is well-formed.
    /// A non-empty result flags a parser bug, never a signoff verdict.
    #[must_use]
    pub fn validate(&self, tol_ns: f64) -> Vec<String> {
        let mut problems = Vec::new();
        for (name, value) in [
            ("slack", self.slack),
            ("required", self.required),
            ("arrival", self.arrival),
            ("crpr", self.crpr),
            ("uncertainty", self.uncertainty),
        ] {
            if !value.is_finite() {
                problems.push(format!("non-finite {name}"));
            }
        }
        for point in &self.points {
            for (name, value) in [
                ("cell_delay", point.cell_delay),
                ("net_delay", point.net_delay),
                ("slew", point.slew),
                ("cap", point.cap),
            ] {
                if !value.is_finite() {
                    problems.push(format!("non-finite {name} on {}", point.pin));
                }
            }
            for (name, value) in [
                ("derate", point.derate),
                ("sigma", point.sigma),
                ("si_delta", point.si_delta),
            ] {
                if value.is_some_and(|v| !v.is_finite()) {
                    problems.push(format!("non-finite {name} on {}", point.pin));
                }
            }
        }
        if !problems.is_empty() {
            return problems;
        }
        let arrival_err = (self.data_arrival() - self.arrival).abs();
        if arrival_err > tol_ns {
            problems.push(format!(
                "arrival mismatch: sum(incr)={:.6} arrival={:.6}",
                self.data_arrival(),
                self.arrival
            ));
        }
        let slack_err = ((self.required - self.arrival) - self.slack).abs();
        if slack_err > tol_ns {
            problems.push(format!(
                "slack mismatch: required-arrival={:.6} slack={:.6}",
                self.required - self.arrival,
                self.slack
            ));
        }
        problems
    }
}

/// A parsed timing report: one run plus its paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingReport {
    pub schema_version: String,
    pub run: Run,
    pub paths: Vec<TimingPath>,
}

impl TimingReport {
    #[must_use]
    pub fn new(run: Run, paths: Vec<TimingPath>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_owned(),
            run,
            paths,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_path() -> TimingPath {
        TimingPath {
            startpoint: "a".to_owned(),
            endpoint: "b".to_owned(),
            launch_clock: "clk".to_owned(),
            capture_clock: "clk".to_owned(),
            group: String::new(),
            check: Check::Setup,
            slack: 0.1,
            required: 1.0,
            arrival: 0.9,
            crpr: 0.0,
            uncertainty: 0.0,
            points: vec![TimingPoint {
                pin: "p".to_owned(),
                cell: String::new(),
                net: String::new(),
                cell_delay: 0.5,
                net_delay: 0.4,
                slew: 0.0,
                cap: 0.0,
                fanout: 0,
                derate: None,
                sigma: None,
                si_delta: None,
            }],
        }
    }

    #[test]
    fn flags_non_finite_values() {
        for mutate in [
            |p: &mut TimingPath| p.slack = f64::NAN,
            |p: &mut TimingPath| p.required = f64::INFINITY,
            |p: &mut TimingPath| p.points[0].net_delay = f64::NAN,
            |p: &mut TimingPath| p.points[0].sigma = Some(f64::NEG_INFINITY),
        ] {
            let mut path = valid_path();
            mutate(&mut path);
            assert_ne!(path.validate(1e-9), Vec::<String>::new());
        }
        assert_eq!(valid_path().validate(1e-9), Vec::<String>::new());
    }
}

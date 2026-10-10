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

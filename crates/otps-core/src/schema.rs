//! Open Timing Path Schema (OTPS) v0.2: versioned, tool-neutral timing-path model.
//!
//! All delays are in nanoseconds unless a field name says otherwise.
//! v0.2 changes: clock paths live in their own segments (data `points` only),
//! per-check slack sign, `Option` unknowns instead of zero defaults, per-point
//! transition and cumulative arrival.

use serde::{Deserialize, Serialize};

/// Schema version. Bump on any breaking change to [`TimingReport`] JSON.
pub const SCHEMA_VERSION: &str = "0.2.0";

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

impl Check {
    /// Header slack from arrival and required, with this check's sign:
    /// setup is `required - arrival`, hold is `arrival - required`.
    #[must_use]
    pub fn slack(self, arrival: f64, required: f64) -> f64 {
        match self {
            Check::Setup => required - arrival,
            Check::Hold => arrival - required,
        }
    }

    /// Sign relating part deltas to header slack deltas:
    /// `Δslack = sign * (Δrequired_parts - Δarrival_parts)`.
    #[must_use]
    pub fn sign(self) -> f64 {
        match self {
            Check::Setup => 1.0,
            Check::Hold => -1.0,
        }
    }
}

/// Clock edge direction. `Unknown` when the report does not say.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Rise,
    Fall,
    #[default]
    Unknown,
}

/// Per-point transition marker (`^` rise, `v` fall in real reports).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Transition {
    Rise,
    Fall,
    Unknown,
}

/// One side's clock path. `None` at the path level when unreported.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ClockSegment {
    pub clock: String,
    /// Clock edge direction; `Unknown` when the report does not say.
    #[serde(default)]
    pub edge: Edge,
    /// Clock edge time, ns: the `clock <name>` row (text) or first
    /// clock-path pin arrival (native JSON). `None` when unreported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_time: Option<f64>,
    /// Clock source latency, ns. 0.0 when the report has no such row.
    #[serde(default)]
    pub source_latency: f64,
    /// Canonical network delay, ns: the condensed summary scalar, or the
    /// stage sum when expanded. The parser canonicalizes this.
    #[serde(default)]
    pub network_delay: f64,
    /// Macro clock tree delay, ns (required-side `macro clock tree delay`
    /// rows). Kept separate so the stage-sum consistency check is unaffected.
    #[serde(default)]
    pub macro_delay: f64,
    /// True when `stages` is empty and `network_delay` is the summary scalar.
    #[serde(default)]
    pub condensed: bool,
    /// Clock stages; empty when condensed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<TimingPoint>,
}

impl ClockSegment {
    /// Edge + source latency + network + macro delay, or `None` when the
    /// edge time is unknown. Other unknowns are impossible by construction.
    #[must_use]
    pub fn total(&self) -> Option<f64> {
        self.edge_time
            .map(|t| t + self.source_latency + self.network_delay + self.macro_delay)
    }
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
    /// Slew, ns (native JSON ×1e9 from seconds). 0.0 when unreported.
    #[serde(default)]
    pub slew: f64,
    /// Load capacitance, femtofarads (native JSON drivers only, ×1e15 from
    /// farads, matching the `-fields capacitance` scale). 0.0 when unreported.
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
    /// Transition marker, if the report carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// Cumulative arrival at this pin, ns. Set by JSON ingest only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrival: Option<f64>,
    /// False when `cell_delay` holds a folded stage total (net+cell combined)
    /// because the report cannot split them (default text without input-pin
    /// rows). Attribution must not blame cell vs net from such stages.
    #[serde(default = "split_available_default")]
    pub split_available: bool,
}

fn split_available_default() -> bool {
    true
}

impl TimingPoint {
    /// Incremental stage delay: cell + net.
    #[must_use]
    pub fn incr(&self) -> f64 {
        self.cell_delay + self.net_delay
    }
}

/// One timing path with its clock and data stages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingPath {
    pub startpoint: String,
    pub endpoint: String,
    pub launch_clock: String,
    pub capture_clock: String,
    /// Launch/capture edge directions; `Unknown` when unreported.
    #[serde(default)]
    pub launch_edge: Edge,
    #[serde(default)]
    pub capture_edge: Edge,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Report-specific check flavor (e.g. the check `type` from `OpenSTA`),
    /// for matcher blocking. `None` when the input has no such field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flavor: Option<String>,
    pub check: Check,
    /// Launch/capture clock paths; `None` when the report omits them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<ClockSegment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<ClockSegment>,
    /// DATA stages only. Clock-pin rows live in the segments.
    pub points: Vec<TimingPoint>,
    /// Header slack, ns, with this check's sign.
    pub slack: f64,
    pub required: f64,
    pub arrival: f64,
    /// Clock uncertainty, ns, as a required-offset contribution (added to
    /// required; negative for setup). Text-verbatim; native JSON omits it.
    /// `None` when unreported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<f64>,
    /// CRPR credit, ns, as a required-offset contribution. Verbatim in both
    /// formats. `None` means CRPR is off (= 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crpr: Option<f64>,
    /// Setup/hold/data-check time, ns, as a required-offset contribution
    /// (added to required; negative for setup). Text-verbatim; native JSON
    /// `margin` is raw `margin()` (positive setup), so setup checks negate
    /// it at ingest. See `reportRequired` in the B4 research notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub margin: Option<f64>,
    /// Input/output external delay, ns, as a required-offset contribution
    /// (added to required; negative for max-side output delay).
    /// Text-verbatim; native JSON `output_delay` checks route their `margin`
    /// key here with the same setup negation. `None` means no external
    /// delay (= 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_delay: Option<f64>,
    /// Half-ULP of the report's printed decimals, in ns. Set by text ingest
    /// (`-digits` rounding); native JSON sets 5e-4 × max(|arrival|,
    /// |required|) (the `{:.3e}` relative half-ULP); `None` means exact
    /// (legacy). Validation widens tolerances by terms × this value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rounding_ns: Option<f64>,
}

impl TimingPath {
    /// Sum of data-stage increments.
    #[must_use]
    pub fn data_arrival(&self) -> f64 {
        self.points.iter().map(TimingPoint::incr).sum()
    }

    /// Arrival from parts: launch clock total + data stages. `None` (with
    /// the missing terms named) when the launch side is unreported.
    #[must_use]
    pub fn arrival_parts(&self) -> (Option<f64>, Vec<String>) {
        match &self.launch {
            None => (None, vec!["launch clock path".to_owned()]),
            Some(seg) => match seg.total() {
                Some(t) => (Some(t + self.data_arrival()), Vec::new()),
                None => (None, vec!["launch edge time".to_owned()]),
            },
        }
    }

    /// Required from parts. `None` (with the missing terms named) when any
    /// load-bearing term is unknown. Absent CRPR/external delay count as 0:
    /// their report rows exist only when the feature applies.
    #[must_use]
    pub fn required_parts(&self) -> (Option<f64>, Vec<String>) {
        let mut unknown = Vec::new();
        let Some(seg) = &self.capture else {
            return (None, vec!["capture clock path".to_owned()]);
        };
        let Some(edge) = seg.total() else {
            return (None, vec!["capture edge time".to_owned()]);
        };
        let mut total = edge;
        match self.uncertainty {
            Some(u) => total += u,
            None => unknown.push("uncertainty".to_owned()),
        }
        total += self.crpr.unwrap_or(0.0);
        match self.margin {
            Some(m) => total += m,
            None => unknown.push("check margin".to_owned()),
        }
        total += self.external_delay.unwrap_or(0.0);
        if unknown.is_empty() {
            (Some(total), unknown)
        } else {
            (None, unknown)
        }
    }

    /// Self-consistency check. [`Severity::Reject`] (non-finite values or a
    /// slack-sign mismatch) means the input is untrustworthy and must abort;
    /// [`Severity::Warn`] (part gaps) loads with the gap flowing into the
    /// attribution residual. Unknown terms are recorded, never zero-filled
    /// (except CRPR/external delay, whose absence means "off").
    #[must_use]
    pub fn validate(&self, tol_ns: f64) -> Validation {
        // Rounded reports (`-digits`): each printed term errs by up to
        // half-ULP, so widen every tolerance by terms × rounding.
        let terms = self.points.len()
            + self.launch.as_ref().map_or(0, |s| s.stages.len())
            + self.capture.as_ref().map_or(0, |s| s.stages.len())
            + 8;
        #[allow(clippy::cast_precision_loss)]
        let tol_eff = tol_ns + terms as f64 * self.rounding_ns.unwrap_or(0.0);
        let mut rejects = self.rejects_for_finiteness();
        if rejects.is_empty() {
            let predicted = self.check.slack(self.arrival, self.required);
            if (predicted - self.slack).abs() > tol_eff {
                rejects.push(format!(
                    "slack mismatch: {:?} predicts {predicted:.6} header slack {:.6}",
                    self.check, self.slack
                ));
            }
        }
        let (mut warns, unknown_terms) = if rejects.is_empty() {
            self.part_warnings(tol_eff)
        } else {
            (Vec::new(), Vec::new())
        };

        let severity = if rejects.is_empty() {
            if warns.is_empty() {
                Severity::Clean
            } else {
                Severity::Warn
            }
        } else {
            Severity::Reject
        };
        let arrival_gap = self
            .arrival_parts()
            .0
            .map_or(0.0, |parts| self.arrival - parts);
        let required_gap = self
            .required_parts()
            .0
            .map_or(0.0, |parts| self.required - parts);
        rejects.append(&mut warns);
        Validation {
            severity,
            problems: rejects,
            arrival_gap,
            required_gap,
            unknown_terms,
        }
    }

    fn rejects_for_finiteness(&self) -> Vec<String> {
        let mut rejects = Vec::new();
        for (name, value) in [
            ("slack", self.slack),
            ("required", self.required),
            ("arrival", self.arrival),
        ] {
            if !value.is_finite() {
                rejects.push(format!("non-finite {name}"));
            }
        }
        for (name, value) in [
            ("uncertainty", self.uncertainty),
            ("crpr", self.crpr),
            ("margin", self.margin),
            ("external_delay", self.external_delay),
        ] {
            if value.is_some_and(|v| !v.is_finite()) {
                rejects.push(format!("non-finite {name}"));
            }
        }
        for (side, seg) in [("launch", &self.launch), ("capture", &self.capture)] {
            if let Some(seg) = seg {
                if seg.edge_time.is_some_and(|v| !v.is_finite()) {
                    rejects.push(format!("non-finite {side} edge time"));
                }
                for (name, value) in [
                    ("source latency", seg.source_latency),
                    ("network delay", seg.network_delay),
                    ("macro delay", seg.macro_delay),
                ] {
                    if !value.is_finite() {
                        rejects.push(format!("non-finite {side} {name}"));
                    }
                }
                check_points_finite(&seg.stages, &mut rejects);
            }
        }
        check_points_finite(&self.points, &mut rejects);
        rejects
    }

    /// Part-gap warnings plus unknown terms. Only called on finite,
    /// slack-consistent paths.
    fn part_warnings(&self, tol_ns: f64) -> (Vec<String>, Vec<String>) {
        let mut warns = Vec::new();
        let mut unknown_terms = Vec::new();
        let (arrival, mut unknown) = self.arrival_parts();
        unknown_terms.append(&mut unknown);
        if let Some(parts) = arrival {
            let gap = self.arrival - parts;
            if gap.abs() > tol_ns {
                warns.push(format!("arrival gap: header-arrival-parts={gap:.6}"));
            }
        }
        let (required, mut unknown) = self.required_parts();
        unknown_terms.append(&mut unknown);
        if let Some(parts) = required {
            let gap = self.required - parts;
            if gap.abs() > tol_ns {
                warns.push(format!("required gap: header-required-parts={gap:.6}"));
            }
        }
        for (side, seg) in [("launch", &self.launch), ("capture", &self.capture)] {
            if let Some(seg) = seg.as_ref().filter(|s| !s.condensed) {
                let sum: f64 = seg.stages.iter().map(TimingPoint::incr).sum();
                if (sum - seg.network_delay).abs() > tol_ns {
                    warns.push(format!(
                        "{side} network inconsistent: stages={sum:.6} scalar={:.6}",
                        seg.network_delay
                    ));
                }
            }
        }
        (warns, unknown_terms)
    }
}

fn check_points_finite(points: &[TimingPoint], rejects: &mut Vec<String>) {
    for point in points {
        for (name, value) in [
            ("cell_delay", point.cell_delay),
            ("net_delay", point.net_delay),
            ("slew", point.slew),
            ("cap", point.cap),
        ] {
            if !value.is_finite() {
                rejects.push(format!("non-finite {name} on {}", point.pin));
            }
        }
        for (name, value) in [
            ("derate", point.derate),
            ("sigma", point.sigma),
            ("si_delta", point.si_delta),
            ("arrival", point.arrival),
        ] {
            if value.is_some_and(|v| !v.is_finite()) {
                rejects.push(format!("non-finite {name} on {}", point.pin));
            }
        }
    }
}

/// Validation outcome for one path.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Clean,
    Warn,
    Reject,
}

/// Full validation result: severity, messages, part gaps, unknown terms.
#[derive(Debug, Clone, PartialEq)]
pub struct Validation {
    pub severity: Severity,
    pub problems: Vec<String>,
    pub arrival_gap: f64,
    pub required_gap: f64,
    pub unknown_terms: Vec<String>,
}

/// A parsed timing report: one run plus its paths.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingReport {
    pub schema_version: String,
    pub run: Run,
    pub paths: Vec<TimingPath>,
    /// Blocks skipped during ingest (e.g. unconstrained paths carry no
    /// slack). Skips are counted, never silent.
    #[serde(default)]
    pub skipped_paths: u64,
}

impl TimingReport {
    #[must_use]
    pub fn new(run: Run, paths: Vec<TimingPath>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_owned(),
            run,
            paths,
            skipped_paths: 0,
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
            launch_edge: Edge::Rise,
            capture_edge: Edge::Rise,
            group: String::new(),
            flavor: None,
            check: Check::Setup,
            launch: Some(ClockSegment {
                clock: "clk".to_owned(),
                edge: Edge::Rise,
                edge_time: Some(0.0),
                source_latency: 0.0,
                network_delay: 0.0,
                macro_delay: 0.0,
                condensed: true,
                stages: Vec::new(),
            }),
            capture: Some(ClockSegment {
                clock: "clk".to_owned(),
                edge: Edge::Rise,
                edge_time: Some(1.0),
                source_latency: 0.0,
                network_delay: 0.0,
                macro_delay: 0.0,
                condensed: true,
                stages: Vec::new(),
            }),
            slack: 0.1,
            required: 1.0,
            arrival: 0.9,
            uncertainty: Some(0.0),
            crpr: None,
            margin: Some(0.0),
            external_delay: None,
            rounding_ns: None,
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
                transition: None,
                arrival: None,
                split_available: true,
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
            |p: &mut TimingPath| p.uncertainty = Some(f64::NAN),
        ] {
            let mut path = valid_path();
            mutate(&mut path);
            assert_eq!(path.validate(1e-9).severity, Severity::Reject);
        }
        assert_eq!(valid_path().validate(1e-9).severity, Severity::Clean);
    }

    #[test]
    fn hold_slack_uses_arrival_minus_required() {
        let mut path = valid_path();
        path.check = Check::Hold;
        path.arrival = 0.30;
        path.required = 0.05;
        path.slack = 0.25;
        path.points[0].cell_delay = 0.20;
        path.points[0].net_delay = 0.10;
        path.capture.as_mut().expect("capture").edge_time = Some(0.05);
        assert_eq!(path.validate(1e-9).severity, Severity::Clean);
        // Setup sign on a hold path must reject.
        path.slack = -0.25;
        assert_eq!(path.validate(1e-9).severity, Severity::Reject);
    }

    #[test]
    fn part_gaps_warn_and_unknowns_record() {
        let mut path = valid_path();
        // Move arrival and slack together so the sign check passes while the
        // parts gap warns.
        path.arrival = 0.95;
        path.slack = 0.05;
        let v = path.validate(1e-9);
        assert_eq!(v.severity, Severity::Warn);
        assert!((v.arrival_gap - 0.05).abs() < 1e-12);

        let mut path = valid_path();
        path.launch = None;
        let v = path.validate(1e-9);
        assert_eq!(v.severity, Severity::Clean);
        assert_eq!(v.unknown_terms, vec!["launch clock path".to_owned()]);
        assert!((v.arrival_gap).abs() < 1e-12);
    }
}

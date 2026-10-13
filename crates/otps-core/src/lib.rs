//! `otps-core`: schema, parsers, matcher, and attribution for the
//! open-source timing-correlation diff engine (Track B2).

pub mod attribution;
pub mod matcher;
pub mod parser;
pub mod schema;
pub mod sta;
pub mod sta_json;

pub use attribution::{
    attribute, cluster, Attribution, Cluster, Component, Confidence, StageDelta, StageKind,
    StageStatus,
};
pub use matcher::{match_paths, MatchKind, MatchResult, PathKey, PathMatch, FUZZY_THRESHOLD};
pub use parser::{parse_json, parse_text, ParseError};
pub use schema::{
    Check, ClockSegment, Edge, Run, Severity, TimingPath, TimingPoint, TimingReport, Transition,
    Validation, SCHEMA_VERSION,
};
pub use sta::{
    detect_text_kind, parse_auto, parse_sta_text, sniff_text_kind, InputKind, IterStats, ParseOpts,
    PathIter, StreamStats,
};
pub use sta_json::{parse_sta_json, sniff_json_kind, JsonPathIter};

//! `otps-core`: schema, parsers, matcher, and attribution for the
//! open-source timing-correlation diff engine (Track B2).

pub mod attribution;
pub mod matcher;
pub mod parser;
pub mod schema;

pub use attribution::{attribute, cluster, Attribution, Cluster, Component};
pub use matcher::{match_paths, MatchKind, MatchResult, PathKey, PathMatch, FUZZY_THRESHOLD};
pub use parser::{parse_json, parse_text, ParseError};
pub use schema::{Check, Run, TimingPath, TimingPoint, TimingReport, SCHEMA_VERSION};

//! Path matcher: align the "same" path across two runs.
//!
//! Exact key match first, then Jaccard similarity over through-pin sets for
//! renamed or ECO-touched paths. The match rate is always reported alongside
//! any diff so a low rate reads as "uncertain", never as "clean".

use std::collections::{HashMap, HashSet};

use crate::schema::{Check, TimingPath};

/// Identity key for exact matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    pub startpoint: String,
    pub endpoint: String,
    pub launch_clock: String,
    pub capture_clock: String,
    pub check: Check,
}

impl From<&TimingPath> for PathKey {
    fn from(p: &TimingPath) -> Self {
        Self {
            startpoint: p.startpoint.clone(),
            endpoint: p.endpoint.clone(),
            launch_clock: p.launch_clock.clone(),
            capture_clock: p.capture_clock.clone(),
            check: p.check,
        }
    }
}

/// How a pair was matched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchKind {
    Exact,
    Fuzzy { score: f32 },
}

/// One aligned pair: `base[i]` is the "same" path as `new[j]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathMatch {
    pub base: usize,
    pub new: usize,
    pub kind: MatchKind,
}

/// Full matcher output.
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub matches: Vec<PathMatch>,
    pub unmatched_base: Vec<usize>,
    pub unmatched_new: Vec<usize>,
    /// `matches / max(base, new)`; 1.0 when both sides are empty.
    pub match_rate: f64,
}

/// Minimum Jaccard score for a fuzzy match.
pub const FUZZY_THRESHOLD: f32 = 0.5;

fn pin_set(path: &TimingPath) -> HashSet<&str> {
    path.points.iter().map(|p| p.pin.as_str()).collect()
}

// Counts here are pin-set sizes (tiny); float conversion cannot lose precision.
#[allow(clippy::cast_precision_loss)]
fn jaccard(a: &HashSet<&str>, b: &HashSet<&str>) -> f32 {
    // Empty pin sets carry no identity: unrelated zero-point paths must not
    // fuzzy-match (exact keys still match identical empty paths).
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f32;
    let union = a.union(b).count() as f32;
    inter / union
}

/// Match `new` paths against `base` paths.
#[must_use]
// Path counts fit exactly in f64 for any report that fits in memory.
#[allow(clippy::cast_precision_loss)]
pub fn match_paths(base: &[TimingPath], new: &[TimingPath]) -> MatchResult {
    let mut matches = Vec::new();
    let mut used_new = vec![false; new.len()];

    // Pass 1: exact keys.
    let mut by_key: HashMap<PathKey, Vec<usize>> = HashMap::new();
    for (j, path) in new.iter().enumerate() {
        by_key.entry(PathKey::from(path)).or_default().push(j);
    }
    let mut pending_base = Vec::new();
    for (i, path) in base.iter().enumerate() {
        let mut hit = None;
        if let Some(candidates) = by_key.get_mut(&PathKey::from(path)) {
            while let Some(j) = candidates.pop() {
                if !used_new[j] {
                    hit = Some(j);
                    break;
                }
            }
        }
        match hit {
            Some(j) => {
                used_new[j] = true;
                matches.push(PathMatch {
                    base: i,
                    new: j,
                    kind: MatchKind::Exact,
                });
            }
            None => pending_base.push(i),
        }
    }

    // Pass 2: greedy fuzzy match on leftovers (same check type only).
    let mut pending_new: Vec<usize> = used_new
        .iter()
        .enumerate()
        .filter(|(_, used)| !**used)
        .map(|(j, _)| j)
        .collect();
    let base_sets: Vec<HashSet<&str>> = pending_base.iter().map(|&i| pin_set(&base[i])).collect();
    let mut new_sets: Vec<(usize, HashSet<&str>)> = pending_new
        .drain(..)
        .map(|j| (j, pin_set(&new[j])))
        .collect();

    for (k, &i) in pending_base.iter().enumerate() {
        let mut best: Option<(usize, f32)> = None;
        for (pos, (j, set)) in new_sets.iter().enumerate() {
            if base[i].check != new[*j].check {
                continue;
            }
            let score = jaccard(&base_sets[k], set);
            if score >= FUZZY_THRESHOLD && best.map_or(true, |(_, s)| score > s) {
                best = Some((pos, score));
            }
        }
        if let Some((pos, score)) = best {
            let (j, _) = new_sets.remove(pos);
            used_new[j] = true;
            matches.push(PathMatch {
                base: i,
                new: j,
                kind: MatchKind::Fuzzy { score },
            });
        }
    }

    let unmatched_base: Vec<usize> = pending_base
        .into_iter()
        .filter(|i| !matches.iter().any(|m| m.base == *i))
        .collect();
    let unmatched_new: Vec<usize> = new_sets.iter().map(|(j, _)| *j).collect();

    let denom = base.len().max(new.len()) as f64;
    let match_rate = if denom == 0.0 {
        1.0
    } else {
        matches.len() as f64 / denom
    };

    matches.sort_by_key(|m| m.base);
    MatchResult {
        matches,
        unmatched_base,
        unmatched_new,
        match_rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::TimingPoint;

    fn path(start: &str, end: &str, pins: &[&str]) -> TimingPath {
        TimingPath {
            startpoint: start.to_owned(),
            endpoint: end.to_owned(),
            launch_clock: "clk".to_owned(),
            capture_clock: "clk".to_owned(),
            group: String::new(),
            check: Check::Setup,
            slack: 0.0,
            required: 0.0,
            arrival: 0.0,
            crpr: 0.0,
            uncertainty: 0.0,
            points: pins
                .iter()
                .map(|pin| TimingPoint {
                    pin: (*pin).to_owned(),
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
                })
                .collect(),
        }
    }

    #[test]
    fn exact_match_rate_is_one() {
        let base = vec![path("a", "b", &["p1", "p2"])];
        let new = vec![path("a", "b", &["p1", "p2"])];
        let r = match_paths(&base, &new);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].kind, MatchKind::Exact);
        assert!((r.match_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn fuzzy_match_on_renamed_endpoints() {
        let base = vec![path("a", "old_b", &["p1", "p2", "p3"])];
        let new = vec![path("a", "new_b", &["p1", "p2", "p3"])];
        let r = match_paths(&base, &new);
        assert_eq!(r.matches.len(), 1);
        assert!(matches!(r.matches[0].kind, MatchKind::Fuzzy { .. }));
    }

    #[test]
    fn zero_point_paths_do_not_fuzzy_match() {
        let base = vec![path("a", "b", &[])];
        let new = vec![path("c", "d", &[])];
        let r = match_paths(&base, &new);
        assert!(r.matches.is_empty());
        // Identical empty paths still match exactly.
        let same = vec![path("a", "b", &[])];
        let r = match_paths(&base, &same);
        assert_eq!(r.matches.len(), 1);
    }

    #[test]
    fn unmatched_paths_lower_rate() {
        let base = vec![path("a", "b", &["p1"])];
        let new = vec![path("a", "b", &["p1"]), path("c", "d", &["q9"])];
        let r = match_paths(&base, &new);
        assert_eq!(r.unmatched_new, vec![1]);
        assert!((r.match_rate - 0.5).abs() < f64::EPSILON);
    }
}

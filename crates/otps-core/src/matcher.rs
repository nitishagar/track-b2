//! Path matcher: align the "same" path across two runs.
//!
//! Exact key match first (FIFO on duplicate keys), then an inverted-index
//! fuzzy pass over DATA-pin Jaccard for renamed or ECO-touched paths.
//! Honesty rules: `match_rate` counts exact pairs only; fuzzy pairs are
//! always reported alongside with their own rate, so a low exact rate reads
//! as "uncertain", never as "clean".
//!
//! Bounds (documented, G8): the fuzzy pass caps full Jaccard evaluations at
//! [`FUZZY_TOP_K`] survivors per base path (ranked by pin overlap among
//! still-unused candidates). Overlap counting itself scales with inverted-
//! index posting mass: adversarial workloads where every path shares many
//! pins degrade toward quadratic. Exact matching is linear.
//!
//! Identity rules (D1): the exact key is startpoint/endpoint/clocks/check
//! plus group, both clock edges, and flavor verbatim. Consequences:
//! same-format runs match precisely (JSON `latch_check` never conflates
//! with `check`); cross-format text-vs-JSON pairs (flavors `max` vs `check`)
//! do not match — diff same-form runs. Fuzzy hard-blocks on check and on
//! flavor (both-known-and-different only; unknown flavor never blocks);
//! clocks are a selection preference, never a block.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::schema::{Check, Edge, TimingPath};

/// Identity key for exact matching.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey {
    pub startpoint: String,
    pub endpoint: String,
    pub launch_clock: String,
    pub capture_clock: String,
    pub check: Check,
    pub group: String,
    pub launch_edge: Edge,
    pub capture_edge: Edge,
    pub flavor: Option<String>,
}

impl From<&TimingPath> for PathKey {
    fn from(p: &TimingPath) -> Self {
        Self {
            startpoint: p.startpoint.clone(),
            endpoint: p.endpoint.clone(),
            launch_clock: p.launch_clock.clone(),
            capture_clock: p.capture_clock.clone(),
            check: p.check,
            group: p.group.clone(),
            launch_edge: p.launch_edge,
            capture_edge: p.capture_edge,
            flavor: p.flavor.clone(),
        }
    }
}

/// How a pair was matched.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    Exact,
    Fuzzy { score: f32 },
}

/// One aligned pair: `base[i]` is the "same" path as `new[j]`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
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
    /// `exact / max(base, new)`; 1.0 when both sides are empty. Fuzzy pairs
    /// never inflate this (G8b).
    pub match_rate: f64,
    /// `fuzzy / max(base, new)`; 0.0 when both sides are empty.
    pub fuzzy_rate: f64,
    /// `(exact + fuzzy) / max(base, new)`; 1.0 when both sides are empty.
    pub combined_rate: f64,
    pub exact_matches: usize,
    pub fuzzy_matches: usize,
}

/// Minimum Jaccard score for a fuzzy match.
pub const FUZZY_THRESHOLD: f32 = 0.5;
/// Full-Jaccard survivors per base path (ranked by pin overlap).
const FUZZY_TOP_K: usize = 128;

/// Jaccard over sorted, deduped pin-id lists. Empty sets carry no
/// identity: unrelated zero-point paths must not fuzzy-match (exact keys
/// still match identical empty paths).
#[allow(clippy::cast_precision_loss)]
fn jaccard_ids(a: &[u32], b: &[u32]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (mut i, mut j, mut inter) = (0, 0, 0u32);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                inter += 1;
                i += 1;
                j += 1;
            }
        }
    }
    inter as f32 / ((a.len() + b.len()) as f32 - inter as f32)
}

/// True when the flavor pair must not fuzzy-match: both flavors known and
/// different. Unknown flavor never blocks.
fn flavor_blocks(base: Option<&str>, new: Option<&str>) -> bool {
    match (base, new) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}

/// Exact keys, FIFO on duplicates (ascending index on both sides).
/// Returns (matches, `used_new` flags, pending base indices).
fn exact_pass(base: &[TimingPath], new: &[TimingPath]) -> (Vec<PathMatch>, Vec<bool>, Vec<usize>) {
    let mut matches = Vec::new();
    let mut used_new = vec![false; new.len()];
    let mut by_key: HashMap<PathKey, VecDeque<usize>> = HashMap::new();
    for (j, path) in new.iter().enumerate() {
        by_key.entry(PathKey::from(path)).or_default().push_back(j);
    }
    let mut pending_base = Vec::new();
    for (i, path) in base.iter().enumerate() {
        let hit = by_key
            .get_mut(&PathKey::from(path))
            .and_then(VecDeque::pop_front);
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
    (matches, used_new, pending_base)
}

/// Fuzzy-match working set: interned pin ids plus an inverted index over
/// leftover new paths. Candidates sharing no pin with a base path are
/// never evaluated.
struct FuzzyIndex<'a> {
    base: &'a [TimingPath],
    new: &'a [TimingPath],
    pending_new: Vec<usize>,
    intern: HashMap<&'a str, u32>,
    entry_ids: Vec<Vec<u32>>,
    index: HashMap<u32, Vec<usize>>,
}

impl<'a> FuzzyIndex<'a> {
    fn build(
        base: &'a [TimingPath],
        new: &'a [TimingPath],
        pending_base: &[usize],
        used_new: &[bool],
    ) -> Self {
        let pending_new: Vec<usize> = used_new
            .iter()
            .enumerate()
            .filter(|(_, used)| !**used)
            .map(|(j, _)| j)
            .collect();
        let mut intern: HashMap<&str, u32> = HashMap::new();
        let mut next_id = 0u32;
        for path in pending_base
            .iter()
            .map(|&i| &base[i])
            .chain(pending_new.iter().map(|&j| &new[j]))
        {
            for point in &path.points {
                intern.entry(point.pin.as_str()).or_insert_with(|| {
                    let id = next_id;
                    next_id += 1;
                    id
                });
            }
        }
        let mut entry_ids: Vec<Vec<u32>> = Vec::with_capacity(pending_new.len());
        let mut index: HashMap<u32, Vec<usize>> = HashMap::new();
        for (pos, &j) in pending_new.iter().enumerate() {
            let mut ids: Vec<u32> = new[j]
                .points
                .iter()
                .map(|p| intern[p.pin.as_str()])
                .collect();
            ids.sort_unstable();
            ids.dedup();
            for &id in &ids {
                index.entry(id).or_default().push(pos);
            }
            entry_ids.push(ids);
        }
        Self {
            base,
            new,
            pending_new,
            intern,
            entry_ids,
            index,
        }
    }

    /// Best fuzzy partner for one base path: top overlap survivors first,
    /// then Jaccard with check + flavor blocks, clocks as preference.
    fn best_for(&self, i: usize, used_new: &[bool]) -> Option<(usize, f32)> {
        let b = &self.base[i];
        let mut bids: Vec<u32> = b
            .points
            .iter()
            .map(|p| self.intern[p.pin.as_str()])
            .collect();
        bids.sort_unstable();
        bids.dedup();
        if bids.is_empty() {
            return None;
        }
        let mut overlaps: HashMap<usize, u32> = HashMap::new();
        for &id in &bids {
            if let Some(postings) = self.index.get(&id) {
                for &pos in postings {
                    *overlaps.entry(pos).or_insert(0) += 1;
                }
            }
        }
        let mut survivors: Vec<(usize, u32)> = overlaps
            .into_iter()
            .filter(|(pos, _)| {
                // Skip used candidates before the top-K cut so greedy order
                // cannot starve later bases (uniform-overlap workloads).
                if used_new[self.pending_new[*pos]] {
                    return false;
                }
                let ids = &self.entry_ids[*pos];
                if ids.is_empty() {
                    return false;
                }
                // Length prefilter: Jaccard >= t needs min/max >= t.
                // Exact in f32 for t = 0.5 (power of two, tiny ints).
                let (lo, hi) = (bids.len().min(ids.len()), bids.len().max(ids.len()));
                #[allow(clippy::cast_precision_loss)]
                let ok = (lo as f32) >= FUZZY_THRESHOLD * (hi as f32);
                ok
            })
            .collect();
        survivors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        survivors.truncate(FUZZY_TOP_K);
        let mut best: Option<(usize, f32, u8)> = None;
        for (pos, _) in survivors {
            let j = self.pending_new[pos];
            let n = &self.new[j];
            if b.check != n.check {
                continue;
            }
            if flavor_blocks(b.flavor.as_deref(), n.flavor.as_deref()) {
                continue;
            }
            let score = jaccard_ids(&bids, &self.entry_ids[pos]);
            if score < FUZZY_THRESHOLD {
                continue;
            }
            // Jaccard scores are quotients in [0, 1], never NaN; the total
            // order below keeps selection deterministic.
            let clock_pref = u8::from(b.launch_clock == n.launch_clock)
                + u8::from(b.capture_clock == n.capture_clock);
            let better = match best {
                None => true,
                Some((best_pos, best_score, best_pref)) => match score.partial_cmp(&best_score) {
                    Some(std::cmp::Ordering::Greater) => true,
                    Some(std::cmp::Ordering::Equal) => {
                        clock_pref > best_pref || (clock_pref == best_pref && pos < best_pos)
                    }
                    _ => false,
                },
            };
            if better {
                best = Some((pos, score, clock_pref));
            }
        }
        best.map(|(pos, score, _)| (self.pending_new[pos], score))
    }
}

/// Match `new` paths against `base` paths.
#[must_use]
// Path counts fit exactly in f64 for any report that fits in memory.
#[allow(clippy::cast_precision_loss)]
pub fn match_paths(base: &[TimingPath], new: &[TimingPath]) -> MatchResult {
    let (mut matches, mut used_new, pending_base) = exact_pass(base, new);

    let fuzzy = FuzzyIndex::build(base, new, &pending_base, &used_new);
    for &i in &pending_base {
        if let Some((j, score)) = fuzzy.best_for(i, &used_new) {
            used_new[j] = true;
            matches.push(PathMatch {
                base: i,
                new: j,
                kind: MatchKind::Fuzzy { score },
            });
        }
    }

    let matched_base: HashSet<usize> = matches.iter().map(|m| m.base).collect();
    let unmatched_base: Vec<usize> = pending_base
        .into_iter()
        .filter(|i| !matched_base.contains(i))
        .collect();
    let unmatched_new: Vec<usize> = (0..new.len()).filter(|j| !used_new[*j]).collect();

    matches.sort_by_key(|m| m.base);
    let exact_matches = matches
        .iter()
        .filter(|m| m.kind == MatchKind::Exact)
        .count();
    let fuzzy_matches = matches.len() - exact_matches;
    let denom = base.len().max(new.len()) as f64;
    let (match_rate, fuzzy_rate, combined_rate) = if denom == 0.0 {
        (1.0, 0.0, 1.0)
    } else {
        (
            exact_matches as f64 / denom,
            fuzzy_matches as f64 / denom,
            matches.len() as f64 / denom,
        )
    };

    MatchResult {
        matches,
        unmatched_base,
        unmatched_new,
        match_rate,
        fuzzy_rate,
        combined_rate,
        exact_matches,
        fuzzy_matches,
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
            launch_edge: crate::schema::Edge::Unknown,
            capture_edge: crate::schema::Edge::Unknown,
            group: String::new(),
            flavor: None,
            check: Check::Setup,
            launch: None,
            capture: None,
            slack: 0.0,
            required: 0.0,
            arrival: 0.0,
            uncertainty: None,
            crpr: None,
            margin: None,
            external_delay: None,
            rounding_ns: None,
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
                    transition: None,
                    arrival: None,
                    split_available: true,
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
        assert_eq!(r.exact_matches, 1);
        assert_eq!(r.fuzzy_matches, 0);
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
        assert_eq!(r.matches, Vec::new());
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

    #[test]
    fn fifo_duplicate_keys_pair_in_order() {
        // G7 probe: identical keys must pair (0,0),(1,1), never reversed.
        let base = vec![path("a", "b", &["p1"]), path("a", "b", &["p1"])];
        let new = vec![path("a", "b", &["p1"]), path("a", "b", &["p1"])];
        let r = match_paths(&base, &new);
        assert_eq!(r.matches.len(), 2);
        assert_eq!((r.matches[0].base, r.matches[0].new), (0, 0));
        assert_eq!((r.matches[1].base, r.matches[1].new), (1, 1));
        assert!(r.matches.iter().all(|m| m.kind == MatchKind::Exact));
    }

    #[test]
    fn key_splits_on_group_and_edge() {
        let mut other_group = path("a", "b", &["p1", "p2"]);
        other_group.group = "clk2".to_owned();
        let r = match_paths(&[path("a", "b", &["p1", "p2"])], &[other_group]);
        assert_eq!(r.matches.len(), 1);
        // Same pins fall back to fuzzy; exact rate stays honest at 0.
        assert!(matches!(r.matches[0].kind, MatchKind::Fuzzy { .. }));
        assert!(r.match_rate.abs() < f64::EPSILON);
        assert!((r.combined_rate - 1.0).abs() < f64::EPSILON);

        let mut other_edge = path("a", "b", &["p1", "p2"]);
        other_edge.launch_edge = crate::schema::Edge::Fall;
        let r = match_paths(&[path("a", "b", &["p1", "p2"])], &[other_edge]);
        assert!(matches!(r.matches[0].kind, MatchKind::Fuzzy { .. }));
    }

    #[test]
    fn flavor_blocks_fuzzy_when_both_known() {
        let mut latch = path("a", "b", &["p1", "p2"]);
        latch.flavor = Some("latch_check".to_owned());
        let mut data = path("a", "c", &["p1", "p2"]);
        data.flavor = Some("data_check".to_owned());
        let r = match_paths(&[latch], &[data]);
        assert_eq!(r.matches, Vec::new());

        // Unknown flavor never blocks.
        let plain = path("a", "b", &["p1", "p2"]);
        let mut typed = path("a", "c", &["p1", "p2"]);
        typed.flavor = Some("check".to_owned());
        let r = match_paths(&[plain], &[typed]);
        assert_eq!(r.matches.len(), 1);
    }

    #[test]
    fn fuzzy_requires_same_check() {
        let mut hold = path("a", "b", &["p1", "p2"]);
        hold.check = Check::Hold;
        let setup = path("a", "c", &["p1", "p2"]);
        let r = match_paths(&[hold], &[setup]);
        assert_eq!(r.matches, Vec::new());
    }

    #[test]
    fn honesty_rates_split_exact_and_fuzzy() {
        let base = vec![path("a", "b", &["p1", "p2"]), path("c", "d", &["q1", "q2"])];
        let new = vec![
            path("a", "b", &["p1", "p2"]),
            path("c", "d_renamed", &["q1", "q2"]),
            path("e", "f", &["z1"]),
        ];
        let r = match_paths(&base, &new);
        assert_eq!(r.exact_matches, 1);
        assert_eq!(r.fuzzy_matches, 1);
        assert!((r.match_rate - 1.0 / 3.0).abs() < 1e-12);
        assert!((r.fuzzy_rate - 1.0 / 3.0).abs() < 1e-12);
        assert!((r.combined_rate - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn clock_preference_breaks_score_ties() {
        let base = vec![path("a", "b", &["p1", "p2", "p3"])];
        let mut other_clk = path("a", "b1", &["p1", "p2", "p3"]);
        other_clk.launch_clock = "other".to_owned();
        let same_clk = path("a", "b2", &["p1", "p2", "p3"]);
        // Index order prefers new[0]; clocks must override to new[1].
        let r = match_paths(&base, &[other_clk, same_clk]);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].new, 1);
    }

    #[test]
    fn fuzzy_matching_is_deterministic() {
        let base = vec![
            path("a", "b", &["p1", "p2", "p3"]),
            path("c", "d", &["p2", "p3", "p4"]),
            path("e", "f", &["p5"]),
        ];
        let new = vec![
            path("a", "b_new", &["p1", "p2", "p3"]),
            path("c", "d_new", &["p2", "p3", "p4"]),
            path("g", "h", &["p9"]),
        ];
        let first = match_paths(&base, &new);
        for _ in 0..50 {
            let again = match_paths(&base, &new);
            assert_eq!(again.matches, first.matches);
            assert_eq!(again.unmatched_base, first.unmatched_base);
            assert_eq!(again.unmatched_new, first.unmatched_new);
        }
    }

    #[test]
    fn fuzzy_top_k_does_not_starve_uniform_workloads() {
        // 150 > FUZZY_TOP_K paths sharing 10 bus pins: used candidates are
        // filtered before the top-K cut, so every base still pairs.
        let mk = |end: &str, unique: &str| {
            let mut pins: Vec<String> = (0..10).map(|b| format!("bus{b}")).collect();
            pins.push(unique.to_owned());
            let refs: Vec<&str> = pins.iter().map(String::as_str).collect();
            path(end, end, &refs)
        };
        let base: Vec<TimingPath> = (0..150).map(|i| mk(&format!("ep{i}"), "u")).collect();
        // Same pins, renamed endpoints: pure fuzzy, Jaccard 1.0.
        let new: Vec<TimingPath> = (0..150).map(|i| mk(&format!("ep{i}_eco"), "u")).collect();
        let r = match_paths(&base, &new);
        assert_eq!(r.matches.len(), 150);
        assert!(r
            .matches
            .iter()
            .all(|m| matches!(m.kind, MatchKind::Fuzzy { .. })));
    }

    #[test]
    fn empty_reports_match_vacuously() {
        let r = match_paths(&[], &[]);
        assert_eq!(r.matches, Vec::new());
        assert!((r.match_rate - 1.0).abs() < f64::EPSILON);
        assert!(r.fuzzy_rate.abs() < f64::EPSILON);
        assert!((r.combined_rate - 1.0).abs() < f64::EPSILON);
    }
}

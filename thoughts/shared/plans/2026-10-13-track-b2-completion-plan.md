# Track B2 completion plan — 2026-10-13

Source: `thoughts/shared/light/2026-10-13-track-b2-completion-gap-analysis.md`
(G1–G13) + Hard Core research (`/tmp/b2-r1/ingest-research.md`,
`/tmp/b2-r2/hardcore2-attribution-schema.md`, `/tmp/b2-r3/` + session design).
Locked: real `report_checks` text is primary ingest; streaming for scale
(126 MB / 400k-path bar); schema 0.2.0 break allowed.

## Order (cores 1+2 couple through the schema; matcher needs edge fields)

### Phase A — schema 0.2.0 + fixtures (foundation)
- A1 `schema.rs`: `Edge`/`Transition` enums; `ClockSegment{clock,edge,edge_time,
  source_latency,network_delay,condensed,stages}`; `TimingPath` gains
  `launch`/`capture: Option<ClockSegment>`, `margin`/`external_delay: Option<f64>`,
  `uncertainty`/`crpr: f64→Option<f64>` (signed as-reported); `points` = data
  stages only; `TimingPoint` gains `transition`, `arrival: Option<f64>`;
  `Check::slack(arrival, required)`; `Validation{Clean,Warn,Reject}` +
  gaps + unknowns; `SCHEMA_VERSION = "0.2.0"`.
- A2 legacy OTPS text grammar v0.2: `Launch/Capture edge|srclat|net` headers,
  signed `Uncertainty`/`Crpr`, `Margin:`/`External:`, `Launch/Capture clock
  points:` sections around data-only `Points:`. v0.1 input rejected with a
  migration pointer (no external users on 0.x).
- A3 fixtures rewritten: real-layout synthesized pair (GPL-safe, never copied
  from goldens) + legacy-0.2 pair; hold signs fixed to arrival−required with a
  real hold regression; route path-4 becomes capture-clock stage deltas; one
  condensed-clock path; one JSON-derived path (unknown edge/unc).
- A4 `validate()` redesign: Reject = non-finite or per-check slack-sign
  mismatch; Warn = arrival/required part gaps (loads, flows into residual);
  unknowns recorded, never zero-defaulted.

### Phase B — ingest (G1–G3)
- B1 real StaText parser: 4-line headers (role parenthetical tolerated),
  row-kind-driven table tokenizer (never fixed-width; right-split
  Description, left-split numerics; `-no_split` continuation joiner);
  arrival/required split; clock-vs-data split at launch flop CK
  (ports/unclocked starts: first pin); required-side vocabulary incl.
  `inter-clock uncertainty` sum, sign-flipped min rows stored verbatim;
  footer (`data required/arrival` negated, `slack (MET|VIOLATED)`);
  `short`/`end`/pulse-width shapes rejected with reasons; unconstrained
  skipped + counted (strict: error).
- B2 streaming: `PathIter<R: BufRead>` yielding per-path `Result` (per-block
  errors resume at next `Startpoint:`, `--max-errors` cap, `skipped` count);
  `sniff()` dispatch (magic + first lines; `--input-kind` override);
  `ParseOpts{time_unit_ns, digits_tol_ns, strict}`; `parse_text`/`parse_json`
  stay as collecting wrappers; CLI owns files/gzip/stdin. Memory bound:
  O(one path), 1 MiB block cap.
- B3 in-repo scale test: synthetic generator + timing test; CI asserts a
  *ratio* (8k vs 4k < 3×), never absolute wall time.
- B4 native `-format json` ingest (streaming pump to `checks[]`, per-check
  `RawValue`): seconds→ns, cumulative-arrival differencing, absent keys →
  unknowns. Pre-step: verify `margin()` sign in PathEnd.cc (read-only);
  if unverifiable, margin=None+unknown (documented).

### Phase C — attribution (G4–G6, G13)
- C1 rewrite: sign-aware (`s=+1/-1`) full accounting
  `residual = Δslack_hdr − s·(Δreq_parts − Δarr_parts)`; `StageDelta`
  top-K=8 + added/removed, deterministic order; `Component` extended
  (Launch/CaptureClock, ClockEdge, Uncertainty, Crpr, CheckMargin,
  ExternalDelay, Unexplained-wins-on-max, Balanced); derate/SI sub-splits
  (no double count); sigma RSS + 3σ note; `confidence: High|Reduced(reasons)`.
- C2 cluster key → (dominant, group-or-prefix); CLI text + JSON carry stage
  rows, residual, unknowns, confidence.
- C3 tests: setup/hold × MET/VIOLATED sign matrix; inserted stage names pin;
  condensed-vs-expanded (residual≠0, explained<1, Unexplained case); D6
  bounds incl. zero-Δ; v0.1 rejection message.

### Phase D — matcher (G7–G8b)
- D1 `PathKey` += `group`, `launch_edge`, `capture_edge` (+ `flavor:
  Option<String>` for JSON `type` / Path Type verbatim; fuzzy blocks on it).
- D2 duplicate keys pair FIFO with stable ties (fixes reverse `pop()`).
- D3 inverted-index fuzzy: u32 pin interning, length prefilter, overlap
  counting, top-K=128 survivors, in-place `used` flags; Jaccard over DATA
  pins only (R2 coupling note); clocks a preference, never a hard block.
- D4 honesty: `MatchKind` Serialize; `matches: Vec<MatchRecord>` in JSON
  (+ `exact_matches`, `fuzzy_matches`); `match_rate = exact/max` plus
  `fuzzy_rate`, `combined_rate`; text prints the split. Existing 6/7
  assertions use exact-only data and survive.
- D5 scale harness + CI ratio gate (shared with B3).

### Phase E — light items
- E1 (G9): gate also fails on unmatched-new violations (absolute WNS on new
  side vs budget); exit codes 0/1/2 preserved.
- E2 (G10): per-side run flags (`--base-*`/`--new-*`); `DiffOutput` carries
  both runs; corner/mode/pdk/flow mismatches visible in text + JSON.
- E3 (G11): `rust-version = "1.85"` (clap 4.6 floor, verified by metadata);
  CI MSRV job (1.85 build+test only — lint versions churn); verify with a
  locally installed 1.85 toolchain.
- E4 (G12): README + docs sync ("(in progress)", "per stage", "drop two
  reports" all true now); social numbers re-verified against new output.

### Phase F — verification + release
Full local gates; oracle spot-checks against read-only goldens; docs/social
re-verification; LEDGER §3 update + new decisions; commit (no co-author
trailers); push; CI + Pages green.

## Invariants (must hold at every commit)
Spec §10 from the bundle: real-report ingest, memory=code, full accounting,
nameable causes, check semantics, matching, gate completeness, run identity,
standing guarantees (D7/D8/D19, unsafe forbidden, 4 gates), claims=code.

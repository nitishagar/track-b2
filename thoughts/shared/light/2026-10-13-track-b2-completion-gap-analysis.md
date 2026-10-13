---
date: 2026-10-13T08:23:25+05:30
researcher: Claude (research_codebase_light)
git_commit: bde7cc0e7a28af91dbd6786363569de56b0fdd2d
branch: main
repository: nitishagar/track-b2
topic: "Understand the current state of implementation from LEDGER.md and focus on missing pieces and misaligned implementation thesis or missing features. Prepare a research to complete the current implementation and also if required take a step back to review a rearchitecture at any step if needed."
track: light
scale: large (escalated; see Hard Cores)
status: researched
---
# track-b2 completion gap analysis — light bundle

## Intent
- Problem: LEDGER.md (2026-10-10) reports v0.1.0 shipped and green; the user wants the gaps between that state and the project thesis (explain why slack moved, "drop two files" adoption, multi-GB, honest residual) found.
- Proposed outcome: a research basis to complete the implementation, plus a rearchitecture review wherever the evidence says a seam cannot meet the thesis.
- Constraints: Apache-2.0; OpenSTA-style public data only, no commercial data (D19); no co-author trailers (D18); 4 gates green (D12); `unsafe` forbidden. OpenSTA's golden files are GPL-3.0: read-only references, never copied into this repo (D19).
- Open questions: Q1 which input does the thesis mean: OTPS text (bespoke), real `report_checks` text, or OpenSTA's native `-format json`? Q2 is multi-GB the real target scale? Q3 fix order: adoption (ingest) or attribution depth first? Q4 is a schema 0.2.0 break allowed? Q5 commit `thoughts/` to the public repo or gitignore it (frontmatter says `researcher: Claude`; D18 convention would need that line changed)?

## Research   (findings only; probes ran from a scratch crate depending on `otps-core`; OpenSTA goldens read via `gh api` into the scratchpad)
- Ledger "verified state" re-confirmed: 19 tests, clippy `-D warnings` and fmt clean, CI + Pages `success` (run 38047704239), site 200 — `LEDGER.md:34-38`, `ci.yml:13-17` [V]. `LEDGER.md` is untracked in git [V]. D6-D11 match code and tests; D3/D5/D12 fail on G3/G5/G11 [V].
- G1 Ingest: OTPS text is bespoke (`Launch clock:`/`Check:`/`Required:` headers, `Points:` … `End`, `pin | cell | net | k=v`) — `parser.rs:139-205` [V]. Real OpenSTA text is `Startpoint: reg1 (rising edge-triggered flip-flop clocked by clk)` / `Path Group` / `Path Type` / `Delay Time Description` table / `data arrival time` / `data required time` / `slack (MET)`; `-fields` adds a Fanout column — OpenSTA `search/test/search_path_delay_output.ok`, `search_crpr_data_checks.ok`, `search_report_fields_formats.ok` [V]. D2 chose the bespoke format on purpose, but the two header sets do not overlap, so "subset" is inaccurate. Real-layout input fails: `line 3: expected 'Launch clock:'` [V].
- G1b OpenSTA natively emits `report_checks -format json` (`search_report_json_formats.ok`): per path `startpoint, endpoint, path_group, path_type, source_clock, target_clock, source_clock_path[], source_path[], target_clock_path[], data_arrival_time, required_time, crpr, margin, slack`; per pin `arrival, slew, capacitance, cell, net`; values in seconds (`1.106e-09`), no explicit cell/net delay fields [V]. Our `parse_json` deserializes `TimingReport {schema_version, run, paths}` (`schema.rs:178-182`, `parser.rs:227-229`), so it cannot read this either [V].
- G2 Docs promise "drop two OpenSTA reports / report files" (`docs/roadmap.html:30`, `docs/adopters.html:35`), yet accepted input comes only from exporters that are roadmap-only and described as JSON Lines (`docs/roadmap.html:35-38`) while `parse_json` takes one document [V]. The ≥5-external-runs gate (`LEDGER.md:158-160`) has no ingest path today.
- G3 Memory: whole-file `read_to_string` (`main.rs:111,115`), `Vec<Line>` (`parser.rs:208`), owned header Strings (`parser.rs:136`), both reports held (`main.rs:146-147`). Probe: 126 MB / 400k paths parse in 1.19 s, RSS +431 MB on top of the 126 MB input (~4.4x) [V]. Contradicts "multi-GB … I/O" (`parser.rs:5`, `README.md:52`, `architecture.html:30-33`).
- G4 Attribution is path-level only (`attribution.rs:26-39`); `paired_sum` is algebraically Σnew−Σbase, so pin pairing never names a stage (`attribution.rs:91-110`); probe: an inserted stage yields cell 0.200 / net 0.100 and no pin [V]. README says "per stage" (`README.md:55-56`).
- G5 Residual is vacuous: `run_diff` aborts on any path failing `validate` (`main.rs:148-153`, `schema.rs:156-170`), so accepted input gives residual ≈ 0 by algebra; probe: required-only move of −0.2 ns → residual 0, explained 100%, dominant `Required` [V]. The "large residual = uncertain" promise (`README.md:19-21`) can only be triggered by match_rate.
- G6 `Required` is an opaque bucket. In real reports arrival includes clock-path stages (`clk1 → ck1buf1 → ck1buf2 → reg2/CK → reg2/Q`, arrival 0.17) and required = edge + clock network delay − uncertainty + CRPR − external delay (`search_crpr_data_checks.ok`) [V]. `crpr`, `uncertainty`, `derate`, `sigma`, `si_delta`, `slew`, `cap`, `fanout`, `group` are parsed and stored but never read by attribution/matcher/CLI [V]. `validate` forces Σ(point incr) == arrival (`schema.rs:156`), so the clock path has no place in v0.1 [V].
- G7 Duplicate keys pair in reverse: two identical 2-path runs with the same start/end pair as (0,1),(1,0) (`matcher.rs:95` `pop()`; probe) [V]. Key lacks rise/fall, through-pins and group (`matcher.rs:13-19`).
- G8 Fuzzy pass is O(P×N) with O(n) removal and an O(U×M) filter (`matcher.rs:130,140,152`): 2k → 0.26 s, 8k → 4.4 s, 16k → 22.5 s (release, all-fuzzy) [V].
- G8b Fuzzy honesty: fuzzy compares only `check` and pin Jaccard ≥ 0.5 (`matcher.rs:131,135`), ignoring start/end/clocks; `match_rate` counts a fuzzy pair as 1 (`matcher.rs:156-160`); probe P4 with all-different start/end names reported rate 1.000 [V]. Text mode prints `fuzzy matches: N` (`main.rs:222-228`); JSON carries no fuzzy count and no per-pair kind/score (`MatchKind` not `Serialize`, `matcher.rs:34`; `DiffOutput`, `main.rs:86-98`) [V].
- G9 Gate hole: a new unmatched path at slack −2.0 ns leaves `worst slack delta: 0.000 ns`, exit 0 (`main.rs:167-171,198-206`); no absolute WNS/TNS gate [V].
- G10 Run identity is dead: one `run` cloned to both sides (`main.rs:146-147`), one set of run flags, the flattened `base_run` (`main.rs:41`), `DiffOutput` carries no run (`main.rs:86-98`), nothing compares corner/mode/pdk/flow_hash; README quickstart passes `--corner sky130hd`, a PDK (`README.md:40`) [V].
- G11 `rust-version = "1.74"` (`Cargo.toml:7`) is false under the committed lock: clap 4.6.7 declares 1.85, zlib-rs 0.6.8 declares 1.75 (cargo metadata); CI uses `@stable` only (`ci.yml:12`); no 1.74 toolchain installed [V].
- G12 Hygiene: `README.md:82-83` still says docs/social "(in progress)"; 2 hand-authored fixtures, tests assert only net-perturbation attribution (`fixtures.rs:37-72`); no bench/scale test in repo [V].
- G13 Hold sign: OpenSTA's min report has arrival 0.10, required −2.00, slack 2.10 (MET), i.e. hold slack = arrival − required (`search_path_delay_output.ok`, `Path Type: min`) [V]. `schema.rs:164` and the slack doc apply required − arrival to every `Check`. The shipped hold fixture (`base.rpt:71-74`: required 0.050, arrival 0.070, slack −0.020) uses the setup sign, so the same numbers would read +0.020 MET in OpenSTA. A real-sign hold path (required 0.05, arrival 0.30, slack 0.25) is rejected `slack mismatch`, and `run_diff` aborts, exit 1 [V]. No real hold path can be diffed today.
- Structure: `otps-core` depends only on serde + serde_json (`otps-core/Cargo.toml:9-11`); flate2/clap live in the CLI [V]. Core has no IO, so the roadmap WASM demo is not blocked by dependencies.
- Rearchitecture triggers (structure cannot meet a stated claim): (a) ingest interface `&str → full TimingReport` (G3) · (b) schema v0.1 + `validate` (arrival == Σ data increments, one slack sign, no clock path) (G6, G13) · (c) attribution output shape, path-level scalars (G4-G5). Not triggers: workspace/crate split, matcher passes (algorithm-level, G7-G8b), CLI gate/run-identity (G9-G10, local).

## Spec — invariants any change must uphold   (requirements, not designs)
- **Real-report ingest**: a report produced by documented OpenSTA `report_checks` (text, or `-format json` if that is the chosen form) loads without a user-side converter, or docs stop claiming it (G1-G2). Edge: partial failure (one bad path in a large file → `ParseError` with line number, never a panic, `CONTRIBUTING.md`).
- **Memory claim = code**: documented memory behaviour holds on the scale probe (126 MB / 400k paths). Edge: boundary (multi-GB, `.gz`, JSON).
- **Full accounting**: components reported for a matched pair sum to Δslack and the unexplained part can be non-zero; an input the model cannot explain surfaces as low `explained_ratio`, never clean (G5). Edge: near-zero Δ stays bounded in [0,1] (D6).
- **Cause is nameable**: a data-path delta names the stage (pin/net/cell); clock-side change (latency, CRPR, uncertainty, setup) is separable from data-side (G4, G6).
- **Check semantics**: setup and hold follow OpenSTA's slack sign; `validate`, fixtures and attribution agree with real reports, and a real hold path loads (G13). Edge: hold at near-zero slack.
- **Matching**: identical inputs match 100% exact with identity pairing and a stable tie rule (G7); cost is not quadratic in unmatched count past a documented bound (G8); fuzzy pairs are distinguishable from exact in every output format and cannot silently inflate `match_rate` (G8b). Edge: duplicate keys, 16k+ all-fuzzy.
- **Gate completeness**: `--fail-on-regression-ps` also fails on unmatched-new violations; exit codes 0/1/2 preserved (D10) (G9).
- **Run identity visible**: both sides' tool/corner/mode/pdk/flow_hash are in the output and a cross-run mismatch is visible (G10). Edge: JSON import with differing `schema_version` (O1).
- **Standing guarantees stay**: deterministic cluster order (D7), non-finite rejected (D8), OpenSTA-only data (D19), `unsafe` forbidden, four gates green.
- **Claims = code**: MSRV, "I/O-bound", "per stage", "drop two reports" are true or removed; a kept MSRV has a CI job (G11-G12).
- Bounding assumptions: the verdict doc was not available; a schema bump (0.x) is assumed allowed pending Q4; public fixtures are synthesized in OpenSTA's layout, never copied from its GPL-3.0 goldens; core stays dependency-light.

## Verification Surface
- Commands: build `cargo build --locked` · lint `cargo clippy --all-targets -- -D warnings` · test-all `cargo test --locked` · **test-one** `cargo test --locked -p otps-core --lib matcher::tests::exact_match_rate_is_one` (`ci.yml:13-17`; ran green) [V]. Integration/CLI one-tests: `… -p otps-core --test fixtures perturbation_test_attributes_net_delay`, `… -p otps-cli --test diff diff_json_parses_and_gate_fails` [V].
- Oracle: OpenSTA golden `.ok` files, fetched with `gh api -H "Accept: application/vnd.github.raw" repos/The-OpenROAD-Project/OpenSTA/contents/search/test/<name>.ok` (text, `-fields`, JSON, hold sign; GPL-3.0, read-only) · `fixtures/{base,route}.rpt` (`fixtures.rs:37-72`) confirms net-delay blame and 85.7% match only. No local OpenSTA binary (`which sta opensta openroad` empty).
- Fixtures: 6/7-path synthetic, setup-sign hold; no real-layout fixture; no scale harness in repo (probes sit in the scratchpad) | Observability: `--format json` (`main.rs:86-98`), exit codes 0/1/2.
- Gaps: no vendored real-layout sample (must be synthesized), scale, MSRV, unmatched-new gate, non-vacuous residual have no observer today. Detail: `thoughts/shared/VERIFICATION_SURFACE.md`.

## Hard Cores
- ESCALATE: 3 named — run `research_codebase_generic_v2_7` for each:
  1. **Ingest**: three input forms are structurally different (OTPS text, OpenSTA text, OpenSTA JSON) + bounded-memory pipeline (G1-G3).
  2. **Attribution + schema**: stage-level blame, clock-vs-data split, setup/hold sign, non-vacuous residual; needs schema bump (G4-G6, G13).
  3. **Path identity + scale**: matcher key, duplicate pairing, sub-quadratic fuzzy pass, fuzzy visibility (G7-G8b).
  Cores 1 and 2 are coupled through the schema: parser output must carry whatever clock-path and sign data attribution consumes.
- Light-track sized (independent, no hard core; they need their own small bundle, not this one): G9 gate, G10 run identity in output, G11 MSRV, G12 docs/README sync.

## Coverage & Open Questions
- Not searched: the verdict doc (not in repo or on disk; the thesis is read from the LEDGER header, D1-D2, D19, README and docs); OpenSTA `-format full_clock_expanded` golden and JSON units beyond the key set above; ORFS report corpus; `social/*.md` bodies; JSON-path performance; `.gz` memory behaviour.
- Overlap check (LEDGER §3 watch item): one standard web search found no "timing-report-explorer" and no path-level slack-attribution diff tool; absence is weak evidence [R].
- Unconfirmed `[R]` (3): text-report cell/net split needs `-fields input_pins nets` (PathView README; goldens show only the Fanout column) · MSRV failure under an actual 1.74 toolchain (declared `rust-version` only) · overlap-check absence. The plan validator re-verifies the first if a plan relies on it.
- Open questions Q1-Q5: see Intent.

## Follow-up 2026-10-13 (advisor review)
- Added: G13 hold sign, G8b fuzzy honesty, G1b OpenSTA native JSON. Upgraded G1 and G6 from `[R]` to `[V]` with OpenSTA golden reports (the guessed raw URL had 404'd; `gh api` tree listing found them). Spec folded the two matcher invariants into one to stay at 10.
- Reframed Q1 (three input forms) and G1 (D2 was deliberate; the header sets do not overlap).

## Closure 2026-10-13 (completion phase)
- All items closed via research → plan → implement (`thoughts/shared/plans/2026-10-13-track-b2-completion-plan.md`, phases A–F).
- Ingest: real `report_checks` text (`sta.rs`, streaming `PathIter` + `sniff_text_kind`), native STA JSON (`sta_json.rs`, `JsonPathIter` + `sniff_json_kind`), CLI streams files/gzip/stdin (G1, G1b, G2, G6, G13).
- Attribution + schema 0.2.0: sign-aware 11-component accounting, stage list, confidence, sigma RSS + 3σ, derate/SI sub-splits, group clustering (G3–G5, G7).
- Matcher + scale: rich `PathKey`, FIFO dupes, inverted-index fuzzy (top-128, flavor/check blocks, clock preference), honesty rate split, `tests/scale.rs` sub-3x CI ratio gates (G8, G8b).
- Light items: G9 unmatched-new gate, G10 per-side `--base-*`/`--new-*` runs, G11 MSRV 1.85 + CI job, G12 docs/social sync.
- Gates green: build, clippy `-D warnings`, fmt, 77 tests (stable + 1.85 MSRV build/test).

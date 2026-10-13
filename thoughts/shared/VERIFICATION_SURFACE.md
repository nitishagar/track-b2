# Verification Surface — track-b2 (`otps`)

Refreshed 2026-10-13 in the completion phase (first commit after bde7cc0) by implement_plan. Durable project-level source for the verify skills.

## Commands (all [V]: run on 2026-10-13, `cargo 1.97.1` stable + `1.85.1` MSRV)
- build: `cargo build --locked`
- lint: `cargo clippy --all-targets -- -D warnings`
- fmt: `cargo fmt --check`
- test-all: `cargo test --locked` (77 pass: 62 otps-core unit + 6 CLI e2e + 3 fixtures + 2 scale + 2 sta-text + 2 sta-json). CI runs these four in `gates` plus an MSRV 1.85 build+test job (`.github/workflows/ci.yml`).
- msrv: `cargo +1.85 build --locked`, `cargo +1.85 test --locked` (both green; `rust-version = "1.85"`).
- test-one (unit): `cargo test --locked -p otps-core --lib matcher::tests::exact_match_rate_is_one`
- test-one (integration): `cargo test --locked -p otps-core --test fixtures perturbation_test_attributes_net_delay`
- test-one (CLI e2e): `cargo test --locked -p otps-cli --test diff diff_json_parses_and_gate_fails`
- test-one (scale): `cargo test --locked -p otps-core --test scale -- --nocapture` (prints ingest 4k/8k + fuzzy 500/1k timings; sub-3x ratio gates)
- demo: `cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt` (expect 6 base / 7 new / 6 matched, 85.7%, worst -0.102 ns, unmatched-new WNS +0.300 ns)
- demo real-text: `... diff fixtures/sta_base.rpt fixtures/sta_route.rpt` (expect 2 base / 3 new / 2 matched, 66.7%, worst -0.100 ns)
- demo native-json: `... diff fixtures/sta_native_base.json fixtures/sta_native_route.json` (same expectations as real-text)
- demo gate: `... diff fixtures/gate_base.rpt fixtures/gate_route.rpt --fail-on-regression-ps 50` (expect exit 2, "unmatched-new WNS" on stderr; exit 0 at 500)

## Oracles
- `fixtures/base.rpt` + `fixtures/route.rpt` (hand-authored, synthetic): confirms net-delay blame, match rate, cluster grouping (`crates/otps-core/tests/fixtures.rs`). Clock-side attribution is covered by `sta_*` fixtures + unit tests.
- `fixtures/sta_base.rpt` + `fixtures/sta_route.rpt`: real-`report_checks`-layout ingest oracle (Time/Delay spans, folded clock spans, hold sign).
- `fixtures/sta_native_base.json` + `fixtures/sta_native_route.json`: native STA JSON oracle (setup-margin negation, `output_delay` routing, skip counting).
- `fixtures/gate_base.rpt` + `fixtures/gate_route.rpt`: unmatched-new gate oracle (10 ps matched regression passes 50 ps; -300 ps added path trips).
- OpenSTA golden reports are a verbatim oracle for text layout, `-fields`, `-format json` (seconds, separate clock/data paths) and hold sign (slack = arrival − required): `gh api -H "Accept: application/vnd.github.raw" repos/The-OpenROAD-Project/OpenSTA/contents/search/test/<name>.ok` with `<name>` in `search_path_delay_output`, `search_crpr_data_checks`, `search_report_fields_formats`, `search_report_json_formats` [V, fetched 2026-10-13]. GPL-3.0: read-only reference, never vendored or copied into fixtures (LEDGER D19); synthesize fixtures in that layout.
- No local OpenSTA binary: `which sta opensta openroad` is empty; subprocess-only per D19.

## Fixtures / harnesses
- 8 synthetic reports: legacy pair, real-text pair, native-JSON pair, gate pair. No real or ORFS-derived corpus.
- `crates/otps-core/tests/scale.rs`: CI sub-3x ratio gates (4k→8k ingest, 500→1k fuzzy) over `tests/common` generator. Research-era 400k-path / 126 MB probes live outside the repo (scratchpad).

## Observability
- `--format json` exposes `match_rate` / `fuzzy_rate` / `combined_rate`, `worst_residual_ns`, `unmatched_new_wns_ns`, `base_run` / `new_run` / `run_mismatches`, per-pair attributions, matches with kind+score, clusters (`crates/otps-cli/src/main.rs`, `DiffOutput`).
- Exit codes: 0 ok, 1 error, 2 gate failed (matched regression or unmatched-new WNS beyond budget).
- Skipped blocks (unconstrained, `path_delay`, nonzero `path_margin`) count on stderr; per-path `unknowns` + `confidence` in JSON.

## Gaps (invariants with no way to observe them today)
- Real-vendor `report_checks` ingest (no vendored sample, no binary; goldens reachable only over the network).
- Memory/time at true multi-GB scale (ratio gates only; no bench harness, no large public corpus).
- Fuzzy bound is documented, not absolute: adversarial uniform-pin workloads degrade toward quadratic.
- Cross-format text-vs-JSON diffs do not match by design (flavor in identity) — covered by docs callout, no test pins the non-match.

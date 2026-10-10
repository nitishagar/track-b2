# Track B2: Timing-Correlation Diff Engine (`otps`)

Explain **why timing slack changed** between two chip-design runs — pre- vs
post-route, corner A vs B, flow v1 vs ported v2 — in seconds, offline.

```sh
cargo run -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt
```

```text
otps diff (schema 0.1.0)
base: 6 paths  new: 7 paths  matched: 6 (unmatched base 0, new 1)
match rate: 85.7%
worst slack delta: -0.102 ns
worst residual: 0.000000 ns
top regressions:
  -0.102 ns  NetDelay  u_core/ff1 -> u_core/ff2  (explained 100%)
  ...
clusters:
  NetDelay x3 [u_core]: total -0.202 ns
  ...
```

No telemetry. No network. Apache-2.0.

**What it is NOT:** a signoff tool. It is signoff-adjacent triage: every report
prints its match rate and unexplained residual, and a large residual means
"uncertain", never "clean".

## Quickstart

```sh
# Build + lint + test (the same gates CI runs)
cargo build --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --locked

# Diff two reports (plain text or .gz, JSON exporter dumps)
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --corner sky130hd

# CI gate: fail with exit 2 if any matched path regresses more than 50 ps
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt \
  --fail-on-regression-ps 50

# Machine-readable output
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --format json
```

## How it works

1. **Parse** — `OpenSTA report_checks` subset (streaming-friendly, gzip-aware)
   or JSON dumps from in-tool exporter scripts into the versioned
   **Open Timing Path Schema** (`Run → Path → Point`).
2. **Match** — exact key (startpoint, endpoint, clocks, check type), then
   through-pin Jaccard fallback for renamed/ECO-touched paths.
3. **Attribute** — `Δslack = Δrequired − Δarrival`, with `Δarrival` split into
   cell vs net per stage; residual always reported.
4. **Cluster** — group by dominant component + hierarchy prefix so 2,000
   miscorrelated endpoints collapse to a handful of root causes.

## Performance

Rust core, hand-rolled parser (no regex), zero-copy-ish line passes. The hot
path is allocation-light so multi-GB reports stay I/O-bound. Run
`cargo test --release` on your own reports to measure; please file issues with
`--format json` timings on large designs.

## Legal / EULA guardrails

- Public fixtures and demos use **only OpenSTA/OpenROAD-style data**.
- OpenSTA is GPL-3.0: this repo **never links it** (subprocess use only).
- Never publish commercial-tool outputs or tool-vs-tool comparisons.
  See `CONTRIBUTING.md` (coming with docs) before contributing samples.

## Repo layout

```text
crates/otps-core  schema, parser, matcher, attribution + unit/integration tests
crates/otps-cli   `otps` binary (parse/diff, text+json, --fail-on gate, .gz)
fixtures/         synthetic pre/post-route pair with injected perturbations
docs/             user guide + GitHub Pages site (in progress)
social/           launch pack + partner outreach (in progress)
```

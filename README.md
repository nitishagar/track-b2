# Track B2: Timing-Correlation Diff Engine (`otps`)

Explain **why timing slack changed** between two chip-design runs — pre- vs
post-route, corner A vs B, flow v1 vs ported v2 — in seconds, offline.

```sh
cargo run -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt
```

```text
otps diff (schema 0.2.0)
base: 6 paths  new: 7 paths  matched: 6 (unmatched base 0, new 1)
match rate: 85.7%
exact: 6  fuzzy: 0  combined rate: 85.7% (renamed/ECO-touched paths stay visible)
worst slack delta: -0.102 ns
worst residual: 0.000000 ns
unmatched-new WNS: +0.300 ns
top regressions:
  -0.102 ns  NetDelay  u_core/ff1 -> u_core/ff2  (explained 100%)
    residual -0.000000 ns  confidence high  unknowns []  stages: u_core/ff2/D +0.100, ...
  ...
clusters:
  NetDelay x4 [clk]: total -0.217 ns
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

# Diff two reports (OpenSTA report_checks text, native JSON, or .gz of either)
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --base-pdk sky130hd

# CI gate: exit 2 if any matched path regresses more than 50 ps,
# or any added path violates beyond 50 ps
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt \
  --fail-on-regression-ps 50

# Corner-vs-corner: label each side; mismatches print on every report
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt \
  --base-corner tt --new-corner ss

# Machine-readable output
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --format json
```

## How it works

1. **Parse** — real `OpenSTA report_checks` text or native STA JSON
   (streaming iterators, gzip-aware, auto-sniffed) into the versioned
   **Open Timing Path Schema** (`Run → Path → Point`). Unrepresentable
   blocks (unconstrained paths, `path_delay`/`path_margin` summaries) are
   counted, never silent.
2. **Match** — exact key (startpoint, endpoint, clocks, edges, group,
   check, flavor), then an inverted-index through-pin Jaccard fallback
   (≥ 0.5, same check) for renamed/ECO-touched paths. `match rate`
   counts exact pairs only; fuzzy pairs get their own
   `fuzzy_rate`/`combined_rate`.
3. **Attribute** — sign-aware `Δslack = s·(Δrequired_parts − Δarrival_parts)`
   over 11 components: cell/net per stage plus launch/capture clock,
   edge, uncertainty, CRPR, check margin, and external delay; derate
   and SI sub-splits, POCV sigma RSS with a 3σ flag, per-path
   confidence, residual always reported.
4. **Cluster** — group by dominant component + path group (hierarchy
   prefix when ungrouped) so 2,000 miscorrelated endpoints collapse to
   a handful of root causes.

## Performance

Rust core, hand-rolled parser (no regex), streaming iterators with bounded
1 MiB blocks — the 400k-path / 126 MB class stays I/O-bound. CI enforces
sub-3x scaling per doubling (`tests/scale.rs`: 4k→8k ingest, 500→1k
fuzzy). Please file issues with `--format json` timings on large designs.

## Legal / EULA guardrails

- Public fixtures and demos use **only OpenSTA/OpenROAD-style data**.
- OpenSTA is GPL-3.0: this repo **never links it** (subprocess use only).
- Never publish commercial-tool outputs or tool-vs-tool comparisons.
  See `CONTRIBUTING.md` before contributing samples.

## Repo layout

```text
crates/otps-core  schema, parser, matcher, attribution + unit/integration tests
crates/otps-cli   `otps` binary (parse/diff, text+json, --fail-on gate, .gz)
fixtures/         synthetic pre/post-route pairs with injected perturbations
docs/             user guide + GitHub Pages site (format, architecture, adopters)
social/           launch pack (HN/Reddit/LinkedIn drafts, 1-pager) + partner outreach
```

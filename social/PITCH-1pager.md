# otps — Timing-Correlation Diff Engine (1-pager pitch)

> Leave-behind for brown-bags, partners, and curious STA engineers.
> Every claim verified against the repo (tool v0.1.0, schema 0.2.0).

---

## The problem

When slack for the "same" path differs between two timing analyses, engineers
spend hours hand-aligning text reports to decide whether the cause is real —
and what it is. The two analyses might be pre- vs post-route, implementation
vs signoff, flow v1 vs ported v2, corner A vs B, or PDK rev 1 vs 2. Possible
causes: parasitics, clock latency, derates, CRPR, SI, library, constraints.
Getting it wrong means chasing fake violations — or missing real ones.

Correlation is still recurring expert work: STA job postings list "timing
correlation between tools" as a standing duty, and no open-source project
matches paths across runs with stage-level delta attribution. (Closest tools
are visualization-only or aggregate worst-slack gates.)

## The product

**otps** aligns the "same" timing paths across two reports and explains why
slack moved, stage by stage, in seconds, offline.

- **Parse** — real OpenSTA `report_checks` text or native STA JSON (plain,
  `.gz`, or stdin; auto-sniffed, streaming) into the versioned Open Timing
  Path Schema (`Run → Path → Point`), with per-side run identity
- **Match** — exact key (startpoint, endpoint, clocks, edges, group, check,
  flavor), then an inverted-index through-pin Jaccard fallback (≥ 0.5, same
  check) for renamed/ECO-touched paths; honest exact/fuzzy/combined rates
- **Attribute** — sign-aware over 11 components (cell/net per stage +
  launch/capture clock, edge, uncertainty, CRPR, check margin, external
  delay); derate/SI sub-splits, POCV sigma RSS, per-path confidence;
  residual always reported
- **Cluster** — group by dominant component + path group, so thousands of
  miscorrelated endpoints collapse to a handful of root causes
- **Gate** — `--fail-on-regression-ps N` exits 2 for CI (matched
  regressions + added-path violations); `--format json` for machines

Rust core, hand-rolled parser (no regex), `unsafe` forbidden. Apache-2.0.
No telemetry. No network. Air-gap friendly.

## Demo scenario (shipped fixtures, runs in seconds)

Synthetic ibex-like design on sky130hd: **pre-route** (estimated parasitics)
vs **post-route** (extracted SPEF), with injected perturbations — net delays
up on long `u_core` routes, a capture-clock latency shift on one IO path, one
added path.

```sh
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --base-pdk sky130hd
```

```
otps diff (schema 0.2.0)
base: 6 paths  new: 7 paths  matched: 6 (unmatched base 0, new 1)
match rate: 85.7%
exact: 6  fuzzy: 0  combined rate: 85.7% (renamed/ECO-touched paths stay visible)
worst slack delta: -0.102 ns
worst residual: 0.000000 ns
unmatched-new WNS: +0.300 ns
top regressions:
  -0.102 ns  NetDelay  u_core/ff1 -> u_core/ff2  (explained 100%)
  -0.050 ns  CaptureClock  u_io/pad1 -> u_core/ff6  (explained 100%)
clusters:
  NetDelay x4 [clk]: total -0.217 ns
  CaptureClock x1 [clk]: total -0.050 ns
```

Read it as: routing parasitics explain the `u_core` regressions; the IO path
moved on the capture-clock side, not the data path; one path is new and
counted, not hidden; 85.7% matched with zero residual.

## What it is NOT

- **Not a signoff tool.** Signoff-adjacent triage only. Every report prints
  its match rate and unexplained residual; a large residual means
  "uncertain", never "clean". It never certifies timing.
- **Not a vendor replacement.** It sits beside your signoff flow and tells
  you where to look — complementing PrimeTime/Tempus-class tools and the
  coming agentic closure suites, not competing with them.
- **Not a data collector.** Nothing phones home; there is no hosted tier
  that ingests your netlist. Evaluation runs fully offline (WASM demo
  planned: files never leave the browser).

## EULA-safe positioning (how commercial users can adopt it)

- Public fixtures and demos use **only OpenSTA/OpenROAD-style data**.
- The repo **never links OpenSTA** (GPL-3.0; subprocess use only) and is
  Apache-2.0 with `unsafe` forbidden.
- We **never publish commercial-tool outputs or tool-vs-tool comparisons**
  — vendor EULAs bar disclosing operation-result comparisons to third
  parties. Internal use by licensed engineers (comparison stays among
  authorized users) is the low-risk posture; public artifacts stay
  OpenROAD/open-PDK.
- PrimeTime/Tempus support is roadmapped strictly as **user-run, in-tool
  Tcl exporter scripts** (documented `get_timing_paths`/`report_timing`-style
  commands, run in your own licensed session). Shipped as source; no vendor
  binaries, no vendor sample outputs in the repo.
- Contributors: samples accepted from OpenSTA/open flows, or under your own
  legal sign-off. (An anonymizer-backed redacted-contribution flow is on the
  roadmap.)

## Ask

1. **Run it** on your worst miscorrelation (OpenSTA reports today) and file
   the failure cases — bad matches, big residuals, large-report timings.
2. **Brown-bag it**: 30-minute "show me your worst miscorrelation" session
   for your STA/PD team (NCR in person, anywhere remote).
3. **Partner**: parser/exporter contributions, co-maintainers from OSS
   silicon, academic collaborators for grant-backed history-store work.

https://github.com/nitishagar/track-b2

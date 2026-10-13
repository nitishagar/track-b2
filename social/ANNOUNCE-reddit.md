# Reddit announce — r/chipdesign + r/FPGA text post

> One draft, post to r/chipdesign first, r/FPGA a few days later with the FPGA
> note swapped in. All tool claims verified against the repo (tool v0.1.0,
> schema 0.2.0).

---

## Title

I built an open-source tool that explains WHY timing slack changed between two
runs (pre/post-route, corner vs corner) — looking for failure cases

## Body

**TL;DR:** `otps` diffs two timing reports, aligns the "same" paths across
runs, and attributes every slack delta to cell vs net vs required time,
clustered by root cause. Rust, offline, Apache-2.0, no telemetry. I want your
nastiest miscorrelation to break it.

**The pain:** post-route slack moves on hundreds of endpoints and you spend
hours hand-aligning text reports to decide whether the cause is parasitics,
clock latency, derate, or just a renamed net. Implementation-vs-signoff
correlation is still literally written into STA job postings as recurring
work. This automates the first 80% of that triage.

**What it does today (tool v0.1.0, schema 0.2.0):**

- Parses real OpenSTA `report_checks` text or native STA JSON (plain,
  `.gz`, or stdin; auto-sniffed) into a versioned schema
  (`Run → Path → Point`), with per-side run identity
- Matches paths: exact key (startpoint, endpoint, clocks, edges, group,
  check, flavor), then an inverted-index through-pin Jaccard fallback
  (≥ 0.5, same check) for renamed/ECO-touched paths; `match_rate` counts
  exact only, fuzzy gets its own rate
- Attributes sign-aware over 11 components (cell/net per stage +
  launch/capture clock, edge, uncertainty, CRPR, check margin, external
  delay), with derate/SI sub-splits, POCV sigma RSS, per-path confidence,
  and the unexplained **residual** on every path
- Clusters by dominant component + path group, so N endpoints collapse to a
  few root causes
- `--fail-on-regression-ps N` CI gate (exit 2; covers matched regressions
  and added-path violations), `--format json` output

**Demo (real output, shipped fixtures — synthetic ibex-like on sky130hd,
pre-route vs post-route with injected perturbations):**

```sh
cargo build --locked
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
  -0.095 ns  NetDelay  u_core/ff3 -> u_core/ff4  (explained 100%)
  -0.050 ns  CaptureClock  u_io/pad1 -> u_core/ff6  (explained 100%)
clusters:
  NetDelay x4 [clk]: total -0.217 ns
  CaptureClock x1 [clk]: total -0.050 ns
```

**Honesty box (read before commenting "table stakes"):**

- The fixtures are 6–7 paths. Toy-sized on purpose — scale is covered by
  CI ratio gates (sub-3x per doubling), but I need real multi-GB reports to
  prove the top end, which is why I'm here.
- Required-time attribution is fully split (launch/capture clock, edge,
  uncertainty, CRPR, check margin, external delay) with derate/SI
  sub-splits and a POCV sigma RSS + 3σ flag — and per-path confidence says
  when unknowns weaken the call.
- Honest rates (`match_rate` exact-only, plus `fuzzy_rate` /
  `combined_rate`), unmatched-new WNS, and residuals print on every report.
  Big residual = "uncertain", never "clean". This is signoff-*adjacent*
  triage, NOT a signoff tool.
- Public fixtures/demos are OpenSTA/OpenROAD-style data only. The repo never
  links OpenSTA (GPL-3.0) and will never publish commercial-tool outputs or
  tool-vs-tool comparisons (EULA). PrimeTime/Tempus support is planned as
  user-run in-tool Tcl exporter scripts — you run them in your own licensed
  session; nothing vendor lands in the repo.

**What I'm asking for — failure cases:**

1. Reports where the matcher gets it wrong (bad fuzzy matches, missed renames)
2. Deltas with large residuals — that means my attribution identity is
   missing something real and I want to know what
3. Big reports — the parser is hand-rolled (no regex), gzip-aware, and
   meant to stay I/O-bound; `--format json` timings on multi-GB reports
   would be gold
4. Your "same design, two flows" stories: ORFS version upgrades, PDK
   revisions (sky130 → GF180/IHP welcome), CTS vs route

Repo: https://github.com/nitishagar/track-b2 (docs site in `docs/`)

*FPGA note (r/FPGA version — swap this paragraph in): Vivado/nextpnr users —
the JSON importer path is your on-ramp today (dump paths/points as JSON and
diff them); a native text parser is contributor-friendly via the planned
per-format plugin points. Tell me what your report format looks like and
I'll prioritise it.*

*Non-goals I will say no to: a signoff stamp, hosted SaaS that ingests your
netlist, anything that phones home. Offline is the whole point.*

# Reddit announce — r/chipdesign + r/FPGA text post

> One draft, post to r/chipdesign first, r/FPGA a few days later with the FPGA
> note swapped in. All tool claims verified against the repo (v0.1.0).

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

**What it does today (v0.1.0):**

- Parses an OpenSTA `report_checks` subset (plain text or `.gz`) plus JSON
  dumps into a versioned schema (`Run → Path → Point`)
- Matches paths: exact key (startpoint, endpoint, clocks, check type), then a
  through-pin Jaccard fallback (≥ 0.5) for renamed/ECO-touched paths
- Attributes `Δslack = Δrequired − Δarrival`, splitting arrival into cell vs
  net per stage, and reports the unexplained **residual** on every path
- Clusters by dominant component + hierarchy prefix, so N endpoints collapse
  to a few root causes
- `--fail-on-regression-ps N` CI gate (exit 2), `--format json` output

**Demo (real output, shipped fixtures — synthetic ibex-like on sky130hd,
pre-route vs post-route with injected perturbations):**

```sh
cargo build --locked
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --corner sky130hd
```

```
otps diff (schema 0.1.0)
base: 6 paths  new: 7 paths  matched: 6 (unmatched base 0, new 1)
match rate: 85.7%
worst slack delta: -0.102 ns
worst residual: 0.000000 ns
top regressions:
  -0.102 ns  NetDelay  u_core/ff1 -> u_core/ff2  (explained 100%)
  -0.095 ns  NetDelay  u_core/ff3 -> u_core/ff4  (explained 100%)
  -0.050 ns  Required  u_io/pad1 -> u_core/ff6  (explained 100%)
clusters:
  NetDelay x3 [u_core]: total -0.202 ns
  Required x1 [u_core]: total -0.050 ns
```

**Honesty box (read before commenting "table stakes"):**

- The fixtures are 6–7 paths. Toy-sized on purpose — I need real reports to
  prove scale, which is why I'm here.
- Required-time attribution is currently one bucket (clock latency +
  uncertainty + CRPR folded together); per-factor splitting is roadmap.
  Same for derate/sigma/SI — the schema has the fields, the attribution
  doesn't split them yet.
- Match rate and residual print on every report. Big residual = "uncertain",
  never "clean". This is signoff-*adjacent* triage, NOT a signoff tool.
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

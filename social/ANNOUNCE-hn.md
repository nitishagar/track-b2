# Show HN draft — otps timing-correlation diff engine

> All tool claims verified against the repo (tool v0.1.0, schema 0.2.0).
> Post timing: weekday morning US time. Reply commitment: author in comments
> all day.

---

## Title

Show HN: otps – explain why timing slack changed between two chip-design runs (offline, OSS)

## Body

Hi HN. I built **otps**, an open-source timing-correlation diff engine for
chip design. Point it at two timing reports — pre- vs post-route, corner A
vs B, flow v1 vs ported v2 — and it tells you *why* slack moved, stage by
stage, instead of making you hand-align text reports.

**Offline, Rust, WASM-roadmapped.** The CLI is Rust (hand-rolled parser, no
regex, gzip-aware, `unsafe` forbidden), Apache-2.0, with zero telemetry and
zero network calls — `--offline` is the only mode, because the target users
sit on air-gapped farms. Next up on the roadmap is a static WASM demo page:
drop two OpenSTA reports in the browser, get the clustered root-cause view,
files never leave the browser. Same offline guarantee, no Rust install to
evaluate.

How it works:

1. **Parse** real OpenSTA `report_checks` text or native STA JSON
   (streaming, gzip-aware, auto-sniffed) into a versioned schema
   (`Run → Path → Point`). Each side gets its own run identity.
2. **Match** the "same" path across runs: exact key first, then an
   inverted-index through-pin Jaccard fallback (≥ 0.5, same check) for
   renamed/ECO-touched paths.
3. **Attribute** sign-aware over 11 components — cell/net per stage plus
   launch/capture clock, edge, uncertainty, CRPR, check margin, external
   delay — with derate/SI sub-splits and a per-path confidence, and report
   the unexplained **residual** on every path.
4. **Cluster** by dominant component + path group, so thousands of
   miscorrelated endpoints collapse to a handful of root causes.

Real output on the shipped fixtures (synthetic ibex-like on sky130hd,
pre- vs post-route):

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
    residual -0.000000 ns  confidence high  unknowns []  stages: ...
clusters:
  NetDelay x4 [clk]: total -0.217 ns
```

**Pre-empting the table-stakes questions:**

- *"Toy demo, 6 paths?"* — Yes, deliberately. The fixtures exist to prove
  the attribution identity; scale is covered by CI ratio gates (sub-3x per
  doubling on 4k→8k ingest and 500→1k fuzzy workloads) over streaming
  iterators. But I have no large public reports to prove the multi-GB story
  yet — that's my ask: run `--format json` on your big OpenSTA reports and
  file the timings.
- *"What about match quality?"* — Every report prints an honesty split:
  `match_rate` counts exact pairs only (`exact / max(base, new)`), with
  separate `fuzzy_rate` / `combined_rate`, per-path residuals, and the
  unmatched-new WNS. A large residual means "uncertain", never "clean".
  This is signoff-*adjacent* triage, never a signoff tool — it narrows
  2,000 endpoints to root causes; it never certifies timing.
- *"Cell/net/required is coarse — what about derate, POCV, SI, CRPR?"* —
  The required side is fully split now: launch/capture clock, edge,
  uncertainty, CRPR, check margin, and external delay are separate
  components, with derate and SI sub-splits and a POCV sigma RSS + 3σ flag.
  Per-path confidence says when unknowns weaken the call. Failure cases
  still wanted — that's what the residual column is for.
- *"Why not just diff in PrimeTime/Tempus?"* — Vendor tools correlate within
  their own families; a vendor-neutral layer across runs, corners, flow
  versions, and PDKs is nobody's product incentive. Also: this will never
  publish commercial-tool outputs or tool-vs-tool comparisons
  (EULA-hostile). Commercial support is planned strictly as user-run in-tool
  Tcl exporter scripts — you run them in your licensed session.
- *"Who is this for?"* — STA/timing-closure engineers, PD/CAD leads gating
  flow upgrades in CI (`--fail-on-regression-ps 50` exits 2), flow-porting
  engineers proving correlation, and OSS-silicon folks diffing ORFS/P DK
  revisions. No IT install: `cargo build`, two files, one command.

Repo + docs: https://github.com/nitishagar/track-b2

I'd especially love: (1) failure cases where matching or attribution is
wrong, (2) large-report timings, (3) your "same design, two flows" stories.
Roast away — the residual column can take it.

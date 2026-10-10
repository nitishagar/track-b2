# Show HN draft — otps timing-correlation diff engine

> All tool claims verified against the repo (v0.1.0). Post timing: weekday
> morning US time. Reply commitment: author in comments all day.

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

1. **Parse** an OpenSTA `report_checks` subset (or JSON dumps) into a
   versioned schema (`Run → Path → Point`).
2. **Match** the "same" path across runs: exact key first, then a
   through-pin Jaccard fallback for renamed/ECO-touched paths.
3. **Attribute** `Δslack = Δrequired − Δarrival`, splitting arrival into cell
   vs net per stage — and report the unexplained **residual** on every path.
4. **Cluster** by dominant component + hierarchy prefix, so thousands of
   miscorrelated endpoints collapse to a handful of root causes.

Real output on the shipped fixtures (synthetic ibex-like on sky130hd,
pre- vs post-route):

```
otps diff (schema 0.1.0)
base: 6 paths  new: 7 paths  matched: 6 (unmatched base 0, new 1)
match rate: 85.7%
worst slack delta: -0.102 ns
worst residual: 0.000000 ns
top regressions:
  -0.102 ns  NetDelay  u_core/ff1 -> u_core/ff2  (explained 100%)
clusters:
  NetDelay x3 [u_core]: total -0.202 ns
```

**Pre-empting the table-stakes questions:**

- *"Toy demo, 6 paths?"* — Yes, deliberately. The fixtures exist to prove
  the attribution identity, not scale. The parser is built to stay I/O-bound
  on multi-GB reports, but I have no large public reports to prove it with
  yet — that's my ask: run `--format json` on your big OpenSTA reports and
  file the timings.
- *"What about match quality?"* — Every report prints `match rate`
  (matched / max(base, new)) and per-path residuals. A large residual means
  "uncertain", never "clean". This is signoff-*adjacent* triage, never a
  signoff tool — it narrows 2,000 endpoints to root causes; it never
  certifies timing.
- *"Cell/net/required is coarse — what about derate, POCV, SI, CRPR?"* —
  Fair. Today required-time is one bucket (clock + uncertainty + CRPR) and
  attribution splits only cell/net/required. The schema already carries
  per-stage derate, sigma, and SI-delta fields; per-factor splitting is
  sequenced roadmap work, driven by whichever failure cases the community
  files first.
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

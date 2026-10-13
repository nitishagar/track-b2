# LinkedIn announce — otps: timing-correlation diff engine (carousel post)

> Copy/paste ready. Carousel = 6 slides (cover + 5 root causes + closer = 7 cards
> total; LinkedIn allows up to 20). Suggested format: one card per section below,
> exported as PDF carousel. All tool claims verified against the repo (tool
> v0.1.0, schema 0.2.0).

---

## Post text

Timing closed on Friday. On Monday, post-route slack moved on 2,000 endpoints —
and nobody can say why. Net? Clock? Derate? A renamed net?

I built **otps**, an open-source timing-correlation diff engine that answers
"why did slack move?" in seconds, offline:

```
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt
```

It aligns the "same" paths across two runs (pre- vs post-route, corner A vs B,
flow v1 vs ported v2), attributes every slack delta over 11 components —
cell/net per stage plus a fully split required side — and clusters thousands
of endpoints into a handful of root causes. Rust, Apache-2.0, zero telemetry,
zero network calls.

Every report prints its honest **match rate** (exact-only, fuzzy split out),
**unexplained residual**, and per-path confidence — a large residual means
"uncertain", never "clean". It is signoff-adjacent triage, never a signoff
tool.

🇮🇳 **Why this matters for India VLSI:** with ChipIN/C-DAC putting EDA in reach
of 500+ organisations, 105 DLI startups doing real tapeouts, and mixed
OpenROAD + commercial flows everywhere — a free, offline, vendor-neutral diff
layer is infrastructure we can all share. No license server. No IT ticket.
`cargo build` and go.

Repo: https://github.com/nitishagar/track-b2

Swipe through for the 5 miscorrelation root-cause patterns it surfaces 👇

#VLSI #PhysicalDesign #STA #TimingClosure #OpenSource #SemiconductorIndia
#ChipIN #OpenROAD #Rust

---

## Slide 1 — Cover

**"Why did my slack move?"**
*5 miscorrelation root causes, auto-detected — offline, in seconds.*

otps · open-source timing-correlation diff engine · Apache-2.0

---

## Slide 2 — Root cause 1: Net (wire) delay blowup

**Post-route parasitics hit long routes first.**

`Δarrival` is split per stage into cell vs net delay. When extracted SPEF
pushes wire delay up, those paths cluster as `NetDelay x N [hierarchy]`
with the total delta — one line instead of 500 endpoints.

Real fixture output (synthetic ibex-like, sky130hd, pre- vs post-route):

```
clusters:
  NetDelay x4 [clk]: total -0.217 ns
```

---

## Slide 3 — Root cause 2: Cell delay shift

**Library, VT-swap, sizing, or derate-driven gate delay.**

Same attribution, other component: per-stage cell-delay deltas summed by pin
pairing, with derate and SI sub-splits and a POCV sigma RSS + 3σ flag. If
ECO sizing or a .lib change moved gate delay, the dominant component reads
`CellDelay`, not `NetDelay` — so you stop blaming the router.

---

## Slide 4 — Root cause 3: Required-time shift (clock / uncertainty / CRPR)

**The capture side moved, not your data path.**

`Δslack = s·(Δrequired_parts − Δarrival_parts)`, sign-aware per check.
Clock-latency shifts land in `CaptureClock` (uncertainty, CRPR, check
margin, and external delay are separate components too) — e.g. the
fixture's IO path:

```
-0.050 ns  CaptureClock  u_io/pad1 -> u_core/ff6  (explained 100%)
```

Classic trap this catches: chasing "data path" violations that are really a
capture-clock shift. (Conceptually the same class as the known
Innovus-vs-signoff insertion-delay miscorrelation mechanism.)

---

## Slide 5 — Root cause 4: Renamed / ECO-touched paths

**Same path, new name — still matched.**

Exact key match (startpoint, endpoint, clocks, edges, group, check,
flavor) first; then an inverted-index through-pin Jaccard fallback (≥ 0.5,
same check type) aligns renamed or ECO-touched paths and labels them fuzzy:

```
exact: 6  fuzzy: 0  combined rate: 85.7% (renamed/ECO-touched paths stay visible)
```

No more "path disappeared" false alarms after an ECO.

---

## Slide 6 — Root cause 5: New / vanished paths + the honesty metrics

**What can't be matched is reported, not hidden.**

Unmatched paths are counted on both sides, and every report prints:

```
match rate: 85.7%
worst residual: 0.000000 ns
unmatched-new WNS: +0.300 ns
```

Match rate = exact / max(base, new); fuzzy pairs get their own
`fuzzy_rate` / `combined_rate`. Residual = unexplained slack per path. Low
match rate or large residual means **"uncertain"** — the tool tells you when
it doesn't know. That is the whole trust model.

---

## Slide 7 — Closer / CTA

**Try it on your worst miscorrelation.**

```sh
cargo build --locked
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --base-pdk sky130hd
# CI gate: exit 2 if any path regresses > 50 ps, or any added path violates > 50 ps
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt --fail-on-regression-ps 50
```

- OpenSTA/OpenROAD-style data only · never links OpenSTA (GPL) · no commercial outputs, ever
- Roadmapped: in-browser WASM demo, PrimeTime/Tempus exporter scripts, history store
- NCR brown-bags starting soon — 30 min, "show me your worst miscorrelation"

⭐ Star + file an issue with your failure case: https://github.com/nitishagar/track-b2

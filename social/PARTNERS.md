# Partner + community outreach shortlist — otps (Track B2)

> Ranked by fit × likelihood of a fast first conversation. Contact channels are
> public surfaces only — no scraped emails. First touch is always "feedback,
> not sales": run it, break it, tell us. Owner actions marked **[TODO]** need
> the maintainer's own accounts/network.

---

## Rank 1 — OpenROAD / ORFS community (distribution + credibility)

**Why-fit:** otps demos entirely on OpenSTA/OpenROAD-style data; ORFS
version-upgrade diffs ("same design, two flows") are the flagship use case.
METRICS2.1 covers design-level scalars — path-level cross-run attribution is
the gap above it, complementary, not competing. A mention or discussion
thread from maintainers is worth more than any ad channel.

**Ask:** feedback on the schema alignment with METRICS2.1 naming; a
"who uses it" pointer if any flow team trials the CI gate; later, an
exporter/attribution contribution path if they build adjacent features.

**Contact channels:**

1. OpenROAD GitHub Discussions / ORFS GitHub Discussions (public thread:
   "path-level diff across ORFS versions — feedback wanted") **[TODO: post
   from maintainer account]**
2. ORConf 2027 CFP (primary FOSSi venue; 2026 missed) — submit talk as soon
   as CFP opens; watch FOSSi Foundation announcements
3. DAC 2027 Open-Source EDA BoF — hallway + BoF slot for vendor/academic
   visibility

---

## Rank 2 — Tiny Tapeout / FOSSi community (users with the exact pain)

**Why-fit:** Post-Efabless multi-PDK reality (SKY130 → IHP SG13G2, GF180MCU
ports) creates exactly the cross-PDK, cross-flow-version diffs otps targets.
OSS-silicon designers on ORFS/LibreLane are the fastest adopters: no
procurement, no legal review, reports they can share publicly.

**Ask:** "Why did my timing change between LibreLane versions / PDK
revisions?" trial runs; redacted failure-case reports; a co-maintainer from
this community by month 6–9 (explicit project goal).

**Contact channels:**

1. FOSSi Discord + Zero to ASIC community (demo post + office-hours offer)
   **[TODO]**
2. Tiny Tapeout Discord (IHP/GF180 port crowd) — short video/GIF of a PDK-rev
   diff once fixtures exist **[TODO]**
3. FOSSi Latch-Up 2027 CFP (usually announced ~January) — submit talk
4. Bluesky/Mastodon OSS-silicon circles — short demo videos

---

## Rank 3 — CaretEDA (commercial OSS-stack ally)

**Why-fit:** CaretEDA ships a *supported* end-to-end OSS stack
(Verilator/Yosys/SymbiYosys/OpenROAD, 2026.08 release) — i.e. they sell
support around exactly the flows otps diffs. A deterministic correlation
layer fits their stack story; partnership/acqui-hire class outcome, not a
sales target.

**Ask:** technical feedback (does path-level diff belong in a supported OSS
flow?); pilot with one customer design; co-marketing (guest post / joint
demo) if the pilot lands.

**Contact channels:**

1. Public contact form / hello@-style address on care teda website **[TODO:
   founder-to-founder intro email from maintainer]**
2. Meet at ORConf / DAC BoF / FOSSi events (in person beats cold email)

---

## Rank 4 — Antmicro (open tooling + services, strong community presence)

**Why-fit:** Leading open-source hardware consultancy; deep in FPGA/ASIC
open tooling, CI for silicon, and community work. They feel multi-flow,
multi-version correlation pain across client projects, and they contribute
back to OSS. Services firms are ideal design partners: many designs, no
product-competition angle.

**Ask:** design-partner feedback; trial the CI gate on an internal flow;
guidance on which report formats (FPGA-side included) to prioritise.

**Contact channels:**

1. Public contact form on antmicro.com **[TODO]**
2. FOSSi events / ORConf hallway track (they attend and speak regularly)
3. Engage with their public GitHub/blog work first, then warm-intro the ask

---

## Rank 5 — ChipIN / C-DAC + DLI startups (India scale channel)

**Why-fit:** ChipIN gives shared EDA access to 1L+ engineers across 500+
organisations; Chips-to-Startup reaches 320 institutions / 68,000+ students;
DLI backs 105 startups with 24 funded design projects and 16 tape-outs.
This is a large, cost-sensitive, often OpenROAD-and-commercial-mixed user
base within reach of Gurugram — free, offline, vendor-neutral tooling is
exactly their shape. DLI-startup inbound is an explicit week-14 success
metric.

**Ask:** brown-bag / webinar for member startups ("gate your flow upgrades
in CI"); a pilot startup that diffs a real pre/post-route pair; academic
collaboration path for grant-backed work (ISM/C2S-linked).

**Contact channels:**

1. ChipIN centre / C-DAC programme contacts via official portals and
   SEMICON India / IESA event networking **[TODO: warm intro via VLSI
   Society contacts preferred over cold outreach]**
2. DLI startup founders via LinkedIn (India VLSI community is very large
   there) — carousel post first, DMs second
3. IESA (India Electronics & Semiconductor Association) events, Noida NCR
   chapter meetups

---

## Rank 6 — Silimate (AI-copilot adjacency, partner-or-acquirer class)

**Why-fit:** On-prem AI copilot for RTL bugs/PPA (YC S23) that maintains an
OpenSTA fork — i.e. they already live in the open-timing world and feel the
correlation problem. otps positions as *the deterministic layer agents call*,
which complements rather than competes with a copilot. Reputation outcome,
not a plan — but the technical conversation is genuinely mutual.

**Ask:** technical exchange (OpenSTA fork learnings ↔ attribution engine);
their view on what a copilot needs from a deterministic diff API (shapes the
roadmapped `otps-mcp` layer).

**Contact channels:**

1. Founder outreach via LinkedIn / YC network **[TODO: maintainer]**
2. Their public GitHub (OpenSTA fork) — substantive issue/PR engagement
   before any pitch

---

## Rank 7 — Precision Innovations (ORFS-adjacent builders)

**Why-fit:** Their engineers are actively building the ORFS QoR-dashboard
upload path — they work daily in the run-compare/regression-gating problem
space one level up (design metrics) from otps (path-level attribution). Natural
technical peers; a path-level drill-down behind their dashboard is an obvious
joint shape.

**Ask:** peer review of schema/JSON output for dashboard interop; trial
linking a QoR regression to an otps cluster; co-contribution norms if scopes
touch (pivot-to-contribute is the stated plan if they ship attribution
first).

**Contact channels:**

1. ORFS GitHub (their QoR-dashboard PRs) — review, test, then introduce otps
   in context **[TODO]**
2. ORFS community calls / discussions

---

## Rank 8 — VLSI Society of India / VLSID (credibility + NCR pipeline)

**Why-fit:** The professional home of the target persona (STA/PD engineers at
Noida/Bengaluru MNC design centres). A VSI talk or VLSID fringe presence
converts directly into brown-bag invitations and Stage-0/Stage-2 interview
pipeline. DVCon India (Bengaluru; ~April 2027 papers deadline) is the
adjacent industry-practitioner venue.

**Ask:** 30-min chapter talk in NCR; VLSID 2027 fringe/workshop slot;
introductions to Noida design-centre STA leads for feedback sessions.

**Contact channels:**

1. VSI Noida/NCR chapter office-bearers via LinkedIn + chapter events
   **[TODO: maintainer — attend first, pitch second]**
2. VLSID 2027 + DVCon India 2027 CFP calendars (watch from late 2026)
3. IESA NCR events (shared audience)

---

## NCR brown-bag plan (Stage 2: weeks 9–14)

**Format:** 30 minutes, on-site or hybrid, titled *"Show me your worst
miscorrelation"*. Not a sales pitch — a live feedback session:

- 5 min: the problem (one slide: 2,000 endpoints moved, why?)
- 10 min: live demo on shipped fixtures + one ORFS-derived pair
  (pre/post-route ibex-like on sky130hd), ending on match-rate + residual
- 15 min: audience miscorrelation show-and-tell — engineer screenshares (or
  describes) a real case; we map it to cell/net/required/renamed/unmatched
  live, or log it as a failure case. No files leave their laptop (offline
  CLI), which is itself the trust demo.

**Leave-behind:** `PITCH-1pager.md` (print or PDF) + repo link.

**Target list (minimum 2 sessions by week 14):**

1. NXP Noida / ST Noida / Qualcomm / MediaTek design centres — STA/PD teams
   via LinkedIn DMs to practicing engineers (10-interview Stage-0 list
   doubles as the invite list) **[TODO]**
2. Cadence/Synopsys Noida R&D centres — methodology-minded audience; frame
   as vendor-neutral academic-style exchange, never competitive **[TODO]**
3. VSI NCR chapter meetup — one session as a chapter talk **[TODO]**
4. DLI startups / IESA NCR — one remote-friendly session for the startup
   cohort **[TODO]**

**Logistics:** maintainer laptop + projector/HDMI; no network needed (offline
CLI is the point — offer to run with machine Wi-Fi off as a trust gesture).
Collect: hours/week on correlation per attendee, last root cause, "would you
run a local CLI on real data?" (Stage-0 gate needs ≥6/10 monthly + ≥3 yes).

**Success bar (ties to week-14 gate):** ≥5 outside engineers reporting runs
on non-toy designs, ≥2 redacted samples or parser PRs, ≥1 inbound from a
semiconductor company or DLI startup. Stars are informational only.

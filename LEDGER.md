# Track B2 — Build Ledger (handoff document)

Date: 2026-10-10. Project: `otps`, an open-source timing-correlation diff
engine. Repo: <https://github.com/nitishagar/track-b2> (public).
Site: <https://nitishagar.github.io/track-b2/>.
Source strategy: "Open-Source Timing-Correlation Diff Engine: Semiconductor
PoC Strategy and Go-With-Conditions Verdict" (verdict: go-with-conditions,
reputation-first OSS). No `thoughts/` material exists for track-b2 (only
unrelated Intercom/fin research); this ledger is the handoff record.

## 1. What was built

```
track-b2/
├── Cargo.toml / Cargo.lock          # workspace (edition 2021, MSRV 1.74, Apache-2.0)
├── crates/otps-core/src/
│   ├── schema.rs        # OTPS v0.1: Run → TimingPath → TimingPoint + validate()
│   ├── parser.rs        # hand-rolled OTPS text parser + JSON importer
│   ├── matcher.rs       # exact-key + Jaccard fallback path matcher
│   ├── attribution.rs   # Δslack decomposition + deterministic clustering
│   └── lib.rs           # re-exports
├── crates/otps-core/tests/fixtures.rs  # pre/post-route diff + perturbation tests
├── crates/otps-cli/
│   ├── src/main.rs      # `otps parse|diff`, text/JSON, .gz, --fail-on gate
│   └── tests/diff.rs    # CLI e2e: output, JSON validity, gate exit codes
├── fixtures/base.rpt + route.rpt  # synthetic pre/post-route pair (6 vs 7 paths)
├── docs/                # GitHub Pages site: index/format/architecture/adopters/roadmap + style.css
├── social/              # ANNOUNCE-{linkedin,reddit,hn}.md, PITCH-1pager.md, PARTNERS.md
├── .github/workflows/ci.yml  # build + clippy(-D warnings) + fmt + test
├── README.md, CONTRIBUTING.md, LICENSE (Apache-2.0)
└── LEDGER.md            # this file
```

Verified state: 19 tests pass (14 unit + 3 fixture + 2 CLI e2e);
`cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check`
clean; GitHub `ci` workflow `success` on main; Pages `built`, index and
subpages return 200. Fixture diff: 6 base / 7 new / 6 matched, 85.7% match
rate, worst Δ −0.102 ns, residual 0.000000.

## 2. Decisions log

### D1. Rust-first implementation (overrode verdict doc's Python core)
The verdict doc specified a Python 3.9+ core with optional Rust later. The
build directive required a "blazing fast" Rust setup for semiconductor
adoption, so the core and CLI are pure Rust (workspace `otps-core` +
`otps-cli`). Consequence: the verdict doc's Pyodide WASM demo does not apply
as specified; a WASM demo is roadmapped via a Rust→WASM build instead.

### D2. OTPS text format v0.1 as an OpenSTA `report_checks` subset
Fixed header order (`Startpoint` … `Uncertainty`), `Points:` block, `End`
terminator; point lines are `pin | cell | net | k=v…`. Comment (`#`) and
blank lines skipped. Rationale: bottom-up "drop two files" adoption without
fragile full-format parsing; JSON import covers in-tool exporter dumps.

### D3. Hand-rolled parser, no regex dependency
Line-oriented parsing over `&str` keeps multi-GB reports I/O-bound and the
dependency tree small (`serde`, `serde_json`, `clap`, `flate2` only).

### D4. Matcher: exact key → Jaccard ≥ 0.5 fallback, same check type only
Exact key = startpoint, endpoint, launch/capture clocks, check. Leftovers
fuzzy-match greedily on through-pin Jaccard (threshold 0.5) within the same
setup/hold class. Match rate = matched / max(base, new); always printed.

### D5. Attribution identity with honest residual
Per pair: `slack_delta − (required_delta − data_delta)` = residual, with
data split into cell/net per stage paired by pin. Dominant = largest
|component|; `Balanced` when nothing exceeds 1e-12.

### D6. `explained_ratio` bounded in [0, 1] (verifier bug B2)
Original `1 − |res|/max(|Δslack|, 1e-12)` exploded to ≈−100000 on
zero-delta + sub-tolerance noise. Now: |Δslack| < 1e-9 judges the residual
on an absolute scale (1.0/0.0), else clamped. Regression test:
`explained_ratio_stays_bounded_near_zero_delta`.

### D7. Deterministic cluster ordering (verifier bug B3)
`HashMap` iteration + count/|total| sort flipped tied clusters across runs.
Added total tiebreak (dominant, prefix); `Component` derives `Ord`.
Regression test runs 50 iterations asserting identical order.

### D8. Non-finite floats rejected (verifier bug B1)
`NaN` parsed, passed `validate` (`NaN > tol` is false), and diffed "clean".
Fix at two layers: parser rejects non-finite numbers; `validate()` flags
non-finite fields first (covers JSON/API-constructed paths). `inf` was
already caught; behavior is now symmetric.

### D9. Strict point lines; empty pin sets don't fuzzy-match (O4, O5)
Trailing `| garbage` columns are now a parse error. Jaccard returns 0.0 when
either pin set is empty (exact keys still match identical empty paths).

### D10. Negative `--fail-on-regression-ps` rejected (O3)
A negative budget false-failed clean diffs; now exits 1 with an error.
Exit codes: 0 ok, 1 error, 2 gate failed. Left as documented: clap usage
errors also exit 2 (O2, distinguishable by output), JSON import ignores
incoming `schema_version` (O1, output schema is what `0.1.0` claims).

### D11. Cluster prefix = parent hierarchy, max two segments
First attempt (first two endpoint segments) never grouped two-segment names.
Now the leaf is dropped (`u_core/ff2` → `u_core`; `a/b/c/d` → `a/b`).

### D12. Hardened workspace lints, MSRV 1.74, committed lockfile
`unsafe_code = forbid`; clippy `all` + `pedantic` as warnings, CI denies
them. Two portability fixes resulted: `is_none_or` (1.82+) replaced for
MSRV, and `assert!(x.is_empty())` rewritten as `assert_eq/ne` for clippy
1.99's `assert_is_empty` (CI toolchain is newer than local 1.97.1).
`Cargo.lock` committed (binary workspace, `--locked` builds).

### D13. Private-first GitHub flow under `nitishagar/track-b2`
Created private, pushed only after local gates passed, flipped to public
(`visibility: PUBLIC` verified by API). Pages required public on this plan
(422 while private), so Pages was enabled after the flip: legacy build from
`main` + `/docs`.

### D14. Static zero-dependency docs site
Five pages + one stylesheet, no CDN, no build step — consistent with the
offline/air-gapped story. Internal links machine-checked; all numeric claims
cross-checked against live CLI output.

### D15. pi.dev-inspired lively theme (kept offline)
Tokens sampled from pi.dev's real stylesheet: warm-dark canvas
(#0d1116/#161d27), moonstone text, tidal blue + sunkissed amber accents,
mono display type (Departure/Commit Mono named first with system fallback),
serif prose. Original CSS, no copied assets; light mode, focus rings, and
`prefers-reduced-motion` included.

### D16. Landing page rewritten for professional SoC engineers
Sober structure: capability list, flow fit, v0.1.0 scope table with explicit
non-goals, accuracy basis, verified quickstart/output, repo + contributing
links. No marketing claims beyond demonstrated behavior.

### D17. Agent organization (adapted mid-build)
A 12-agent research→plan→implement workflow stalled on one research agent
(2/12 across four turns) and was stopped; completed research was already
covered inline. Replaced with three focused subagents on disjoint paths:
independent verifier (25 probes; found B1–B3/O3–O5, repo untouched),
docs builder (`docs/`), social/partner builder (`social/`). Git worktrees
were authorized but unnecessary — no lane touched another's paths.

### D18. No co-author trailers
All commits are single-author (`Nitish Agarwal <…@users.noreply.github.com>`);
history audited, zero `Co-authored-by` trailers.

### D19. EULA guardrails enforced throughout
Only synthetic OpenSTA-style fixtures in public; never link OpenSTA
(GPL-3.0); no commercial-tool outputs or tool-vs-tool comparisons in code,
docs, fixtures, or social pack. CONTRIBUTING.md makes acceptance rules
explicit (OpenSTA-only samples).

## 3. Deferred / open items

### Closed in the completion phase (2026-10-13)
- Real `report_checks` text ingest (streaming `PathIter`, auto-sniffed),
  native STA JSON ingest (streaming `JsonPathIter`), CLI streams
  files/gzip/stdin with error budget and skip counting.
- Schema 0.2.0 (clock segments, per-check slack sign, `Option` unknowns,
  per-point transition/arrival) with sign-aware 11-component attribution,
  stage lists, confidence, sigma RSS + 3σ, derate/SI sub-splits.
- Matcher rewrite: rich `PathKey`, FIFO dupes, inverted-index fuzzy
  (top-128, flavor/check blocks, clock preference), honesty rate split
  (`match_rate` exact-only + `fuzzy_rate`/`combined_rate`).
- `tests/scale.rs` sub-3x CI ratio gates (4k→8k ingest, 500→1k fuzzy);
  77 tests green on stable and on MSRV 1.85 (new CI job).
- Unmatched-new gate (absolute WNS, exit 2), per-side `--base-*`/`--new-*`
  runs with visible mismatches, full README/docs/social re-verification.
- Known remaining edges: cross-format text-vs-JSON diffs do not match by
  design; cap/slew/variation columns, generated-clock source paths, and
  summary aggregates are rejected with reasons; fuzzy bound is documented,
  not absolute, on adversarial uniform-pin workloads.

### Still open
- Roadmap page: WASM demo, in-tool Tcl exporter scripts, history store +
  trend charts, MCP server (`otps-mcp`, local-only), Liberty-diff module.
- No local OpenSTA Toolchain here: parser tests use synthetic fixtures; a
  live-OpenSTA demo and ORFS-derived corpus pairs are follow-ups.
- Outreach TODOs in `social/PARTNERS.md` (maintainer accounts/network):
  OpenROAD/ORFS discussions, FOSSi/Tiny Tapeout posts, CaretEDA/Antmicro/
  Silimate/Precision intros, ChipIN/DLI + VSI/VLSID/DVCon pipeline, two NCR
  brown-bags by week 14 with the ≥5-external-runs gate.
- Stage gates from the verdict doc still apply: kill/pivot if week-14
  external-use bar or vendor-legal issues arise; watch
  `timing-report-explorer`/OpenROAD for overlapping path-level attribution.

## 4. Operator commands

```sh
cd /home/nitish/track-b2
cargo build --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --locked
cargo run -q -p otps-cli -- diff fixtures/base.rpt fixtures/route.rpt
gh run list --repo nitishagar/track-b2 --limit 3   # CI + pages deploys
```

Commits: `a4471ad` core+CLI → `65a20ff` verifier fixes+docs+social →
`86c6c09` clippy-1.99 CI fix → `bde7cc0` theme+landing+CONTRIBUTING.

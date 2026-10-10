# Contributing to otps

## Legal guardrails (read first)

- Contribute fixtures and samples from **OpenSTA / OpenROAD-style data only**.
- **Never submit commercial-tool outputs** (PrimeTime, Tempus, or any licensed
  EDA report text) or tool-vs-tool comparisons. Even anonymized commercial
  outputs may count as vendor "results" under EULA benchmark clauses.
- This project never links OpenSTA (GPL-3.0). Do not add OpenSTA as a build or
  link dependency; subprocess invocation for local demos is the only contact.
- By contributing you agree your contribution is licensed Apache-2.0.

## How to contribute

- **Bug reports:** include the failing input (redacted is fine), the command,
  expected vs actual output, and `rustc --version`.
- **Parser support:** add a module under `crates/otps-core/src/`, fixtures
  under `fixtures/`, and unit + integration tests. Numbers must be finite
  (`NaN`/`inf` rejected); behavior on malformed input must be a clean
  `ParseError` with a line number, never a panic.
- **Attribution/matcher changes:** keep the identity
  `Δslack = Δrequired − Δarrival` exact, keep cluster ordering deterministic,
  and extend the perturbation tests in `crates/otps-core/tests/fixtures.rs`.

## Gates (must pass before every push)

```sh
cargo build --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --locked
```

CI runs the same four gates on every push and PR.

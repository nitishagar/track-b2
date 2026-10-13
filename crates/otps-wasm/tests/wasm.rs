//! Boundary tests that only run under `wasm-pack test --node` (empty natively).

#![cfg(target_arch = "wasm32")]

use otps_wasm::{diff_reports, schema_version};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn schema_version_matches_core() {
    assert_eq!(schema_version(), otps_core::SCHEMA_VERSION);
    assert_eq!(schema_version(), "0.2.0");
}

#[wasm_bindgen_test]
fn garbage_input_returns_error_shape() {
    let out = diff_reports("@@@ not a report @@@\n", "also garbage\n", "{}");
    assert!(out.contains("\"error\""), "missing error key: {out}");
}

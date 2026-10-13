// Functional drift check for the committed WASM demo bundle.
//
// Drives the COMMITTED docs/pkg/ build on the three fixture pairs and
// byte-compares each result against the current CLI `--format json` output.
// If core changed without a docs/pkg/ rebuild, this fails.
//
// Usage: node scripts/wasm-parity.mjs   (exit 0 = identical, 1 = drift)
// Requires: docs/pkg/ built, otps binary built (cargo build -p otps-cli).
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const { default: init, diff_reports, schema_version } = await import(
  join(root, "docs/pkg/otps_wasm.js")
);
await init(readFileSync(join(root, "docs/pkg/otps_wasm_bg.wasm")));

const pairs = [
  ["base.rpt", "route.rpt"],
  ["sta_base.rpt", "sta_route.rpt"],
  ["sta_native_base.json", "sta_native_route.json"],
];

let failed = false;
console.log(`schema_version(): ${schema_version()}`);
for (const [b, n] of pairs) {
  const base = readFileSync(join(root, "fixtures", b), "utf8");
  const nw = readFileSync(join(root, "fixtures", n), "utf8");
  const wasmOut = diff_reports(base, nw, "{}") + "\n"; // CLI println! newline
  const cliOut = execFileSync(
    join(root, "target/debug/otps"),
    ["diff", join(root, "fixtures", b), join(root, "fixtures", n), "--format", "json"],
    { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
  );
  if (wasmOut === cliOut) {
    console.log(`IDENTICAL ${b} x ${n} (${wasmOut.length} bytes)`);
  } else {
    console.log(`DRIFT ${b} x ${n}: committed docs/pkg/ does not match current core`);
    failed = true;
  }
}
const err = diff_reports("@@@ not a report @@@\n", "x", "{}");
if (!err.startsWith('{"error":')) {
  console.log(`DRIFT error shape broken: ${err.slice(0, 80)}`);
  failed = true;
}
process.exit(failed ? 1 : 0);

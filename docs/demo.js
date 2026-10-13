// otps demo — runs the real diff engine (docs/pkg/, wasm-pack --target web)
// 100% locally in the browser. No fetch/XHR/WebSocket anywhere on this page:
// all input comes from the textareas/file pickers below (FileReader only).
//
// ES-module/file:// honesty: `import ... from './pkg/otps_wasm.js'` is an ES
// module import, which browsers CORS-block on file://. Serve this page over
// http(s) — e.g. `python3 -m http.server` in docs/ — or use the Pages URL.
// There is deliberately no file:// workaround (no bundling, no inline blob).
import init, { diff_reports, schema_version } from './pkg/otps_wasm.js';

// ---------------------------------------------------------------------------
// Inlined sample pair: fixtures/sta_base.rpt + fixtures/sta_route.rpt
// (OpenSTA report_checks layout, synthesized content; ~5KB total). Inlined
// because Pages serves docs/ only, so ../fixtures/ is unreachable.
// Regenerate check: node -e extract SAMPLE_BASE/SAMPLE_NEW and cmp fixtures.
// ---------------------------------------------------------------------------
const SAMPLE_BASE = `# OTPS fixture: real \`report_checks\`-layout pair (SYNTHESIZED content).
# Every pin name and value below is invented for these tests in the genuine
# OpenSTA layout. No golden content. Units: ns.
Startpoint: u_core/ff1 (rising edge-triggered flip-flop clocked by clk)
Endpoint: u_core/ff2 (rising edge-triggered flip-flop clocked by clk)
Path Group: clk
Path Type: max

  Delay    Time   Description
---------------------------------------------------
   0.00    0.00   clock clk (rise edge)
   0.00    0.00   clock source latency
   0.01    0.01 ^ clkbuf0/Z (CLKBUF_X1)
   0.00    0.01 ^ u_core/ff1/CK (DFF_X1)
   0.64    0.65 ^ u_core/ff1/Q (DFF_X1)
   0.20    0.85 ^ u_core/ff2/D (DFF_X1)
           0.85   data arrival time

   1.00    1.00   clock clk (rise edge)
   0.00    1.00   clock network delay (ideal)
  -0.05    0.95   clock uncertainty
  -0.05    0.90   library setup time
           0.90   data required time
---------------------------------------------------
           0.90   data required time
          -0.85   data arrival time
---------------------------------------------------
           0.05   slack (MET)

Startpoint: u_core/ff7 (rising edge-triggered flip-flop clocked by clk)
Endpoint: u_core/ff8 (rising edge-triggered flip-flop clocked by clk)
Path Group: clk
Path Type: min

  Delay    Time   Description
---------------------------------------------------
   0.00    0.00   clock clk (rise edge)
   0.00    0.00   clock network delay (ideal)
   0.00    0.00 v u_core/ff7/CK (DFF_X1)
   0.05    0.05 v u_core/ff7/Q (DFF_X1)
   0.02    0.07 v u_core/ff8/D (DFF_X1)
           0.07   data arrival time

   0.00    0.00   clock clk (rise edge)
   0.00    0.00   clock network delay (ideal)
   0.00    0.00   clock reconvergence pessimism
   0.01    0.01   library hold time
           0.01   data required time
---------------------------------------------------
           0.01   data required time
          -0.07   data arrival time
---------------------------------------------------
           0.06   slack (MET)
`;

const SAMPLE_NEW = `# OTPS fixture: post-route counterpart to sta_base.rpt (SYNTHESIZED).
# Perturbations: net growth on the long route (A), hold regression (B),
# one added reg-to-output path with external delay (C).
Startpoint: u_core/ff1 (rising edge-triggered flip-flop clocked by clk)
Endpoint: u_core/ff2 (rising edge-triggered flip-flop clocked by clk)
Path Group: clk
Path Type: max

  Delay    Time   Description
---------------------------------------------------
   0.00    0.00   clock clk (rise edge)
   0.00    0.00   clock source latency
   0.01    0.01 ^ clkbuf0/Z (CLKBUF_X1)
   0.00    0.01 ^ u_core/ff1/CK (DFF_X1)
   0.64    0.65 ^ u_core/ff1/Q (DFF_X1)
   0.30    0.95 ^ u_core/ff2/D (DFF_X1)
           0.95   data arrival time

   1.00    1.00   clock clk (rise edge)
   0.00    1.00   clock network delay (ideal)
  -0.05    0.95   clock uncertainty
  -0.05    0.90   library setup time
           0.90   data required time
---------------------------------------------------
           0.90   data required time
          -0.95   data arrival time
---------------------------------------------------
          -0.05   slack (VIOLATED)

Startpoint: u_core/ff7 (rising edge-triggered flip-flop clocked by clk)
Endpoint: u_core/ff8 (rising edge-triggered flip-flop clocked by clk)
Path Group: clk
Path Type: min

  Delay    Time   Description
---------------------------------------------------
   0.00    0.00   clock clk (rise edge)
   0.00    0.00   clock network delay (ideal)
   0.00    0.00 v u_core/ff7/CK (DFF_X1)
   0.04    0.04 v u_core/ff7/Q (DFF_X1)
   0.02    0.06 v u_core/ff8/D (DFF_X1)
           0.06   data arrival time

   0.00    0.00   clock clk (rise edge)
   0.00    0.00   clock network delay (ideal)
   0.00    0.00   clock reconvergence pessimism
   0.01    0.01   library hold time
           0.01   data required time
---------------------------------------------------
           0.01   data required time
          -0.06   data arrival time
---------------------------------------------------
           0.05   slack (MET)

Startpoint: u_core/ff10 (rising edge-triggered flip-flop clocked by clk)
Endpoint: u_io/pad5 (output port clocked by clk)
Path Group: clk
Path Type: max

  Delay    Time   Description
---------------------------------------------------
   0.00    0.00   clock clk (rise edge)
   0.00    0.00   clock network delay (ideal)
   0.00    0.00 ^ u_core/ff10/CK (DFF_X1)
   0.30    0.30 ^ u_core/ff10/Q (DFF_X1)
   0.10    0.40 ^ u_io/pad5 (out)
           0.40   data arrival time

   1.00    1.00   clock clk (rise edge)
   0.00    1.00   clock network delay (ideal)
  -0.20    0.80   output external delay
           0.80   data required time
---------------------------------------------------
           0.80   data required time
          -0.40   data arrival time
---------------------------------------------------
           0.40   slack (MET)
`;

// Input caps (bytes of UTF-8 text per side; checked before diffing).
const WARN_BYTES = 50 * 1024 * 1024;
const MAX_BYTES = 200 * 1024 * 1024;

const $ = (id) => document.getElementById(id);

const els = {
  status: $("demo-status"),
  base: $("demo-base"),
  new: $("demo-new"),
  baseFile: $("demo-base-file"),
  newFile: $("demo-new-file"),
  diff: $("demo-diff"),
  sample: $("demo-sample"),
  error: $("demo-error"),
  warn: $("demo-warn"),
  results: $("demo-results"),
  summary: $("demo-summary"),
  regressions: $("demo-regressions"),
  clusters: $("demo-clusters"),
  raw: $("demo-raw"),
  schema: $("demo-schema"),
};

let wasmReady = false;

// ---------------------------------------------------------------------------
// boot
// ---------------------------------------------------------------------------
try {
  await init();
  wasmReady = true;
  els.schema.textContent = `engine: otps-wasm (schema ${schema_version()})`;
  els.status.textContent = "Engine ready. Paste reports, pick files, or try the sample pair.";
  els.diff.disabled = false;
  els.sample.disabled = false;
} catch (err) {
  els.status.textContent = "";
  showError(
    `Could not load the WASM engine (${err}). ` +
      "This page must be served over http(s) — ES module imports are blocked on " +
      "file://. Try `python3 -m http.server` in docs/, or use the Pages URL."
  );
}

// ---------------------------------------------------------------------------
// input handling
// ---------------------------------------------------------------------------
function showError(msg) {
  els.error.textContent = msg;
  els.error.hidden = false;
  els.results.hidden = true;
}

function clearMessages() {
  els.error.hidden = true;
  els.error.textContent = "";
  els.warn.hidden = true;
  els.warn.textContent = "";
}

function warn(msg) {
  els.warn.textContent = msg;
  els.warn.hidden = false;
}

function isGzipName(name) {
  return name.toLowerCase().endsWith(".gz");
}

function isGzipMagic(buf) {
  const b = new Uint8Array(buf);
  return b.length >= 2 && b[0] === 0x1f && b[1] === 0x8b;
}

async function readPickedFile(file, target) {
  if (isGzipName(file.name)) {
    showError(`"${file.name}": v1 reads plain text only — decompress .gz first.`);
    return;
  }
  if (file.size > MAX_BYTES) {
    showError(`"${file.name}" is ${(file.size / 1048576).toFixed(0)} MB: over the 200 MB cap.`);
    return;
  }
  const head = await file.slice(0, 2).arrayBuffer();
  if (isGzipMagic(head)) {
    showError(`"${file.name}": looks like gzip (magic bytes 1f 8b) — v1: decompress first.`);
    return;
  }
  target.value = await file.text();
  clearMessages();
  if (file.size > WARN_BYTES) {
    warn(`"${file.name}" is ${(file.size / 1048576).toFixed(0)} MB: over 50 MB, the diff may take a while.`);
  }
}

els.baseFile.addEventListener("change", () => {
  if (els.baseFile.files.length > 0) void readPickedFile(els.baseFile.files[0], els.base);
});
els.newFile.addEventListener("change", () => {
  if (els.newFile.files.length > 0) void readPickedFile(els.newFile.files[0], els.new);
});

els.sample.addEventListener("click", () => {
  clearMessages();
  els.base.value = SAMPLE_BASE;
  els.new.value = SAMPLE_NEW;
});

// ---------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------
function runIdentity(prefix) {
  // Empty fields are omitted so the engine falls back to CLI defaults
  // (tool opensta, corner/mode/pdk "unknown"; new side falls back to base).
  const obj = {};
  for (const field of ["corner", "mode", "pdk"]) {
    const v = $(`${prefix}-${field}`).value.trim();
    if (v !== "") obj[field] = v;
  }
  return obj;
}

function byteLen(s) {
  return new TextEncoder().encode(s).length;
}

function fmtSigned(x, digits) {
  const s = Number(x).toFixed(digits);
  return (Number(x) >= 0 ? "+" : "") + s;
}

function fmtPct(x) {
  return `${(Number(x) * 100).toFixed(1)}%`;
}

function esc(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function renderSummary(d) {
  const lines = [
    `base: ${d.base_paths} paths  new: ${d.new_paths} paths  matched: ${d.matched} (unmatched base ${d.unmatched_base}, new ${d.unmatched_new})`,
    `match rate: ${fmtPct(d.match_rate)}  fuzzy: ${fmtPct(d.fuzzy_rate)}  combined: ${fmtPct(d.combined_rate)} (exact ${d.exact_matches}, fuzzy ${d.fuzzy_matches})`,
    `worst slack delta: ${fmtSigned(d.worst_slack_delta_ns, 3)} ns`,
    `worst residual: ${Number(d.worst_residual_ns).toFixed(6)} ns`,
    `unmatched-new WNS: ${d.unmatched_new_wns_ns === null || d.unmatched_new_wns_ns === undefined ? "n/a" : fmtSigned(d.unmatched_new_wns_ns, 3) + " ns"}`,
  ];
  if (Array.isArray(d.run_mismatches) && d.run_mismatches.length > 0) {
    lines.push(`run mismatches: ${d.run_mismatches.join("; ")}`);
  } else {
    lines.push("run mismatches: none");
  }
  els.summary.textContent = lines.join("\n");
}

function renderRegressions(d) {
  const rows = [...d.attributions]
    .sort((a, b) => a.slack_delta - b.slack_delta)
    .filter((a) => a.slack_delta < 0)
    .slice(0, 5);
  if (rows.length === 0) {
    els.regressions.innerHTML = "<p>No matched-path regressions.</p>";
    return;
  }
  const trs = rows
    .map(
      (a) =>
        `<tr><td>${esc(a.endpoint)}</td><td>${esc(a.dominant)}</td>` +
        `<td>${fmtSigned(a.slack_delta, 3)} ns</td>` +
        `<td>${fmtPct(a.explained_ratio)}</td>` +
        `<td>${Number(a.residual).toFixed(6)} ns</td><td>${esc(a.confidence)}</td></tr>`
    )
    .join("");
  els.regressions.innerHTML =
    "<table><tr><th>Endpoint</th><th>Dominant</th><th>Δslack</th>" +
    "<th>Explained</th><th>Residual</th><th>Confidence</th></tr>" +
    trs +
    "</table>";
}

function renderClusters(d) {
  if (!Array.isArray(d.clusters) || d.clusters.length === 0) {
    els.clusters.innerHTML = "<p>No clusters.</p>";
    return;
  }
  const items = d.clusters
    .map(
      (c) =>
        `<li><code>${esc(c.dominant)}</code> ×${c.count} [${esc(c.prefix)}]: ` +
        `total ${fmtSigned(c.total_slack_delta, 3)} ns</li>`
    )
    .join("");
  els.clusters.innerHTML = `<ul>${items}</ul>`;
}

els.diff.addEventListener("click", () => {
  if (!wasmReady) return;
  clearMessages();
  const baseText = els.base.value;
  const newText = els.new.value;
  if (baseText.trim() === "" || newText.trim() === "") {
    showError("Both report inputs are required — paste text, pick files, or use the sample pair.");
    return;
  }
  const sizes = [byteLen(baseText), byteLen(newText)];
  if (sizes[0] > MAX_BYTES || sizes[1] > MAX_BYTES) {
    showError("Input over the 200 MB cap — split the report and retry.");
    return;
  }
  if (sizes[0] > WARN_BYTES || sizes[1] > WARN_BYTES) {
    warn("Input over 50 MB — the diff may take a while.");
  }
  const opts = JSON.stringify({ base_run: runIdentity("demo-base"), new_run: runIdentity("demo-new") });
  let out;
  try {
    out = diff_reports(baseText, newText, opts);
  } catch (err) {
    showError(`Engine threw (unexpected): ${err}`);
    return;
  }
  let d;
  try {
    d = JSON.parse(out);
  } catch {
    showError(`Engine returned non-JSON (unexpected): ${out.slice(0, 400)}`);
    return;
  }
  if (d && typeof d.error === "string") {
    showError(d.error);
    return;
  }
  renderSummary(d);
  renderRegressions(d);
  renderClusters(d);
  els.raw.textContent = out;
  els.results.hidden = false;
});

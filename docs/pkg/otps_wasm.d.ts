/* tslint:disable */
/* eslint-disable */

/**
 * Diff two timing reports given as UTF-8 text. `opts_json` is the
 * `{"base_run": {...}, "new_run": {...}}` object (all fields optional;
 * `"{}"` selects CLI defaults). Returns the pretty diff JSON, or
 * `{"error":"..."}` on any failure — never throws.
 */
export function diff_reports(base: string, _new: string, opts_json: string): string;

/**
 * The OTPS schema version this build diffs against (== core `SCHEMA_VERSION`).
 */
export function schema_version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly diff_reports: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly schema_version: () => [number, number];
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

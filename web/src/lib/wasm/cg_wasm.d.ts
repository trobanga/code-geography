/* tslint:disable */
/* eslint-disable */

/**
 * The JavaScript class `Explorer`: only forwards to the plain type above.
 */
export class Explorer {
    free(): void;
    [Symbol.dispose](): void;
    focusView(expanded: string[], include_tests: boolean, merge_imports: boolean, focus_nodes: string[], focus_changes: boolean, neighbors: boolean, unfolded: string[], fold_mixed: boolean): string;
    constructor(graph_json: string);
    view(expanded: string[], include_tests: boolean, merge_imports: boolean): string;
}

/**
 * `diffSources(basePaths, baseTexts, headPaths, headTexts, rootName, baseName, headName,
 * changedPaths, changedKinds)`: JSON of `{graph, report}` with the overlay graph; throws
 * when two parallel arrays differ in length or a kind is not `added`, `removed` or
 * `modified`.
 */
export function diffSources(base_paths: string[], base_texts: string[], head_paths: string[], head_texts: string[], root_name: string, base_name: string, head_name: string, changed_paths: string[], changed_kinds: string[]): string;

/**
 * `indexSources(paths, texts, rootName)`: JSON of `{graph, report}`; throws on an error.
 */
export function indexSources(paths: string[], texts: string[], root_name: string): string;

/**
 * `isInput(path)`: true for a `.java` file, a `pom.xml`, a `.rs` file or a `Cargo.toml`
 * that an indexer wants.
 */
export function isInput(path: string): boolean;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly memcmp: (a: number, b: number, c: number) => number;
    readonly __wbg_explorer_free: (a: number, b: number) => void;
    readonly diffSources: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number, n: number, o: number, p: number, q: number, r: number) => [number, number, number, number];
    readonly explorer_focusView: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number) => [number, number];
    readonly explorer_new: (a: number, b: number) => [number, number, number];
    readonly explorer_view: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly indexSources: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly isInput: (a: number, b: number) => number;
    readonly calloc: (a: number, b: number) => number;
    readonly free: (a: number) => void;
    readonly iswspace: (a: number) => number;
    readonly iswdigit: (a: number) => number;
    readonly iswalpha: (a: number) => number;
    readonly strlen: (a: number) => number;
    readonly abort: () => void;
    readonly malloc: (a: number) => number;
    readonly realloc: (a: number, b: number) => number;
    readonly strncmp: (a: number, b: number, c: number) => number;
    readonly memchr: (a: number, b: number, c: number) => number;
    readonly memcpy: (a: number, b: number, c: number) => number;
    readonly memmove: (a: number, b: number, c: number) => number;
    readonly memset: (a: number, b: number, c: number) => number;
    readonly strchr: (a: number, b: number) => number;
    readonly strcmp: (a: number, b: number) => number;
    readonly strncat: (a: number, b: number, c: number) => number;
    readonly strncpy: (a: number, b: number, c: number) => number;
    readonly iswalnum: (a: number) => number;
    readonly iswblank: (a: number) => number;
    readonly iswlower: (a: number) => number;
    readonly towupper: (a: number) => number;
    readonly iswpunct: (a: number) => number;
    readonly iswupper: (a: number) => number;
    readonly towlower: (a: number) => number;
    readonly iswxdigit: (a: number) => number;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_alloc: () => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
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

/* tslint:disable */
/* eslint-disable */

/**
 * The JavaScript class `Explorer`: only forwards to the plain type above.
 */
export class Explorer {
    free(): void;
    [Symbol.dispose](): void;
    edgesBetween(from: string, to: string, include_tests: boolean, merge_imports: boolean): string;
    focusView(expanded: string[], include_tests: boolean, merge_imports: boolean, focus_nodes: string[], focus_changes: boolean, neighbors: boolean, unfolded: string[], fold_mixed: boolean): string;
    constructor(graph_json: string);
    view(expanded: string[], include_tests: boolean, merge_imports: boolean): string;
}

/**
 * The JavaScript class `GitRepo`: a repository built from the files of its `.git`
 * (ADR 0039). See `cg_git::Repository`.
 */
export class GitRepo {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * `addLoose(id, bytes)`: the file `.git/objects/xx/yyyy…` with `id` = `xxyyyy…`.
     */
    addLoose(id: string, bytes: Uint8Array): void;
    /**
     * `addPack(index, data)`: a `.idx` file and its `.pack` file; throws on a broken pack.
     */
    addPack(index: Uint8Array, data: Uint8Array): void;
    /**
     * `addRef(name, text)`: a file below `.git/refs`, `name` from `.git` (`refs/heads/main`).
     */
    addRef(name: string, text: string): void;
    /**
     * `new GitRepo(head)`: `head` is the text of `.git/HEAD`.
     */
    constructor(head: string);
    /**
     * `setPackedRefs(text)`: the text of `.git/packed-refs`.
     */
    setPackedRefs(text: string): void;
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

/**
 * `worktreeOverlay(repo, paths, texts, rootName)`: JSON of `{graph, report, base}` with the
 * overlay of the working tree against `HEAD`; throws when git cannot give `HEAD`.
 */
export function worktreeOverlay(repo: GitRepo, paths: string[], texts: string[], root_name: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly memcmp: (a: number, b: number, c: number) => number;
    readonly __wbg_explorer_free: (a: number, b: number) => void;
    readonly __wbg_gitrepo_free: (a: number, b: number) => void;
    readonly diffSources: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number, n: number, o: number, p: number, q: number, r: number, s: number) => void;
    readonly explorer_edgesBetween: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => void;
    readonly explorer_focusView: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number) => void;
    readonly explorer_new: (a: number, b: number, c: number) => void;
    readonly explorer_view: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly gitrepo_addLoose: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly gitrepo_addPack: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly gitrepo_addRef: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly gitrepo_new: (a: number, b: number) => number;
    readonly gitrepo_setPackedRefs: (a: number, b: number, c: number) => void;
    readonly indexSources: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly isInput: (a: number, b: number) => number;
    readonly worktreeOverlay: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => void;
    readonly strlen: (a: number) => number;
    readonly free: (a: number) => void;
    readonly malloc: (a: number) => number;
    readonly iswspace: (a: number) => number;
    readonly iswalpha: (a: number) => number;
    readonly iswdigit: (a: number) => number;
    readonly calloc: (a: number, b: number) => number;
    readonly abort: () => void;
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
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number) => void;
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

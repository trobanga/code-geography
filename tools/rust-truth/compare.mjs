// Compares the Rust `calls` edges of a cg graph with the rust-analyzer ground truth (SCIP).
//
//   node compare.mjs <graph.json> <index.scip> <repo> <sites.txt> [--list <dir>]
//
// <sites.txt>: the stderr of a cg built with print-sites.patch (run.sh does it), lines
//   DECL  kind id file name-start name-end item-start item-end parent test
//   SITE  file start end form outcome target caller        (form: call, method, value, call-expr)
//   MACRO file start end caller                            (a macro call that our walker skipped)
// All positions are byte offsets. The join is by file and byte range of the name token:
// a truth function is mapped onto our node by the position of its name in its definition,
// so the comparison does not depend on how either side spells ids.
import fs from "node:fs";
import path from "node:path";
import { readScip, range4 } from "./scip.mjs";

const args = process.argv.slice(2);
const listAt = args.indexOf("--list");
const listDir = listAt >= 0 ? args.splice(listAt, 2)[1] : null;
const [graphFile, scipFile, repo, sitesFile] = args;

const graph = JSON.parse(fs.readFileSync(graphFile, "utf8"));
const nodes = new Map(graph.nodes.map((n) => [n.id, n]));
const out = [];
const log = (...a) => out.push(a.join(" "));
const pct = (a, b) => (b ? ((100 * a) / b).toFixed(1) + " %" : "-");
const bump = (m, k, n = 1) => m.set(k, (m.get(k) ?? 0) + n);
const push = (m, k, v) => (m.has(k) ? m.get(k).push(v) : m.set(k, [v]));
const top = (m, n = 99) => [...m].sort((a, b) => (b[1].length ?? b[1]) - (a[1].length ?? a[1])).slice(0, n);

// ---- our side
const decls = [];
const sites = [];
const macros = [];
for (const line of fs.readFileSync(sitesFile, "utf8").split("\n")) {
  const f = line.split("\t");
  if (f[0] === "DECL") decls.push({ kind: f[1], id: f[2], file: f[3], ns: +f[4], ne: +f[5], is: +f[6], ie: +f[7], parent: f[8], test: f[9] === "true" });
  else if (f[0] === "SITE") sites.push({ file: f[1], start: +f[2], end: +f[3], form: f[4], outcome: f[5], target: f[6], caller: f[7] });
  else if (f[0] === "MACRO") macros.push({ file: f[1], start: +f[2], end: +f[3], caller: f[4] });
}
const declAt = new Map(); // file|ns|ne -> [decl]
for (const d of decls) push(declAt, `${d.file}|${d.ns}|${d.ne}`, d);
const declById = new Map(decls.map((d) => [d.id, d]));
const siteAt = new Map(sites.map((s) => [`${s.file}|${s.start}|${s.end}`, s]));
const macrosIn = new Map();
for (const m of macros) push(macrosIn, m.file, m);

const ours = { fn: new Set(), type: new Set() };
for (const e of graph.edges) {
  if (e.kind !== "calls") continue;
  ours[nodes.get(e.to)?.kind === "function" ? "fn" : "type"].add(`${e.from} -> ${e.to}`);
}

// ---- the truth
const { documents } = readScip(fs.readFileSync(scipFile));
const texts = new Map();
const lineStarts = new Map();
for (const d of documents) {
  const buf = fs.readFileSync(path.join(repo, d.path));
  texts.set(d.path, buf);
  const starts = [0];
  for (let i = 0; i < buf.length; i++) if (buf[i] === 10) starts.push(i + 1);
  lineStarts.set(d.path, starts);
}
// SCIP columns are UTF-8 code units here (position_encoding 1); checked below.
const encodings = new Set(documents.map((d) => d.encoding));
const bytes = (file, r) => {
  const [l0, c0, l1, c1] = range4(r);
  const s = lineStarts.get(file);
  return [s[l0] + c0, s[l1] + c1];
};
const isFnSym = (sym) => !sym.startsWith("local ") && sym.endsWith(").");
const isTypeSym = (sym) => !sym.startsWith("local ") && sym.endsWith("#") && !sym.includes("().");

// Definitions of functions and types in the repository.
const defsOf = new Map(); // symbol -> [{file, start, end, es, ee}]
const fnDefsIn = new Map(); // file -> [def], for the caller of a reference
let occTotal = 0;
for (const d of documents) {
  for (const o of d.occurrences) {
    occTotal++;
    if (!(o.roles & 1)) continue;
    if (!isFnSym(o.symbol) && !isTypeSym(o.symbol)) continue;
    const [start, end] = bytes(d.path, o.range);
    const [es, ee] = o.enclosing.length ? bytes(d.path, o.enclosing) : [start, end];
    const def = { symbol: o.symbol, file: d.path, start, end, es, ee, fn: isFnSym(o.symbol) };
    def.decl = declAt.get(`${d.path}|${start}|${end}`) ?? [];
    push(defsOf, o.symbol, def);
    if (def.fn) push(fnDefsIn, d.path, def);
  }
}
const truthFnDefs = [...defsOf.values()].flat().filter((d) => d.fn);
const truthTypeDefs = [...defsOf.values()].flat().filter((d) => !d.fn);

// The function whose body holds a position: the innermost function definition with a node.
const callerAt = (file, pos) => {
  const around = (fnDefsIn.get(file) ?? []).filter((d) => d.es <= pos && pos < d.ee && d.decl.length === 1);
  around.sort((a, b) => b.es - a.es);
  return around[0];
};
const text = (file, s, e) => texts.get(file).toString("utf8", s, e);
const followedByParen = (file, end) => {
  const t = texts.get(file).toString("utf8", end, end + 200);
  return /^\s*(::\s*<[^()]*>\s*)?\(/.test(t);
};
const inMacro = (file, pos) => (macrosIn.get(file) ?? []).find((m) => m.start <= pos && pos < m.end);
const lineOf = (file, pos) => {
  const s = lineStarts.get(file);
  let lo = 0, hi = s.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (s[mid] <= pos) lo = mid; else hi = mid - 1;
  }
  return lo + 1;
};
const inUse = (file, pos) => {
  // The statement that holds the position starts with `use` or `pub use`.
  const b = texts.get(file);
  let i = pos;
  while (i > 0 && b[i - 1] !== 59 && b[i - 1] !== 125 && b[i - 1] !== 123) i--; // ; } {
  let head = b.toString("utf8", i, pos);
  if (/\buse\b[^;]*$/.test(head) && /^\s*(#\[[^\]]*\]\s*)*(pub(\([^)]*\))?\s+)?use\b/.test(head)) return true;
  // inside a `use a::{b, c}` group: look further back.
  const back = b.toString("utf8", Math.max(0, pos - 2000), pos);
  const m = back.match(/(?:^|[;}\n])\s*(pub(\([^)]*\))?\s+)?use\s+[^;]*$/);
  return !!m;
};

// ---- truth reference sites of functions
const truth = [];
const unmappedCallee = new Map();
for (const d of documents) {
  for (const o of d.occurrences) {
    if (o.roles & 1 || !isFnSym(o.symbol)) continue;
    const [start, end] = bytes(d.path, o.range);
    const t = { file: d.path, start, end, symbol: o.symbol, line: lineOf(d.path, start), token: text(d.path, start, end) };
    // callee
    const defs = defsOf.get(o.symbol);
    if (!defs) t.scope = "external";
    else {
      let def = defs.length === 1 ? defs[0] : defs.find((x) => x.file === d.path);
      if (!def) t.scope = "ambiguous";
      else {
        t.scope = "internal";
        t.def = def;
        if (def.decl.length === 1) t.callee = def.decl[0].id;
        else {
          t.callee = null;
          bump(unmappedCallee, `${def.decl.length} nodes at the definition: ${o.symbol}`);
        }
      }
    }
    // caller
    const c = callerAt(d.path, start);
    t.caller = c ? c.decl[0].id : null;
    // form
    const s = siteAt.get(`${d.path}|${start}|${end}`);
    t.site = s;
    if (s) {
      s.truth = t;
      t.form = s.form === "value" ? "value" : "call";
    } else if (!/^[A-Za-z_]/.test(t.token)) t.form = "implicit"; // `?`, `.await`, operators
    else if (inMacro(d.path, start)) t.form = followedByParen(d.path, end) ? "macro-call" : "macro-value";
    else if (followedByParen(d.path, end)) t.form = "call-unwalked";
    else if (inUse(d.path, start)) t.form = "import";
    else t.form = "value-unwalked";
    truth.push(t);
  }
}
const isCallForm = (f) => f === "call" || f === "macro-call" || f === "call-unwalked" || f === "implicit";
const isValueForm = (f) => f === "value" || f === "macro-value" || f === "value-unwalked";

// ---- 1. coverage
log(`## 1 coverage`);
log(`SCIP: ${documents.length} documents (position encoding ${[...encodings]}), ${occTotal} occurrences; truth fn defs ${truthFnDefs.length}, mapped onto exactly one node ${truthFnDefs.filter((d) => d.decl.length === 1).length}; truth type defs ${truthTypeDefs.length}, mapped ${truthTypeDefs.filter((d) => d.decl.length === 1).length}`);
const fnDecls = decls.filter((d) => d.kind === "Function");
const mappedDecl = new Set(truthFnDefs.flatMap((d) => d.decl.map((x) => x.id)));
const notInTruth = fnDecls.filter((d) => !mappedDecl.has(d.id));
log(`our fn nodes ${fnDecls.length}, with a truth definition at the name ${fnDecls.length - notInTruth.length}; without: ${notInTruth.length} ${notInTruth.slice(0, 5).map((d) => d.id).join(", ")}`);
const unmappedFnDefs = truthFnDefs.filter((d) => d.decl.length !== 1);
log(`truth fn defs without a node: ${unmappedFnDefs.length} ${unmappedFnDefs.slice(0, 5).map((d) => `${d.symbol.split(" ").pop()}@${d.file}:${lineOf(d.file, d.start)}`).join(", ")}`);
const formTally = new Map();
for (const t of truth) bump(formTally, `${t.form}/${t.scope}`);
log(`truth fn references ${truth.length} by form/scope: ${[...formTally].sort().map(([k, n]) => `${k} ${n}`).join(", ")}`);
log(`truth references without a caller node (outside fns, e.g. const initializers): ${truth.filter((t) => !t.caller).length}`);

// ---- truth edges
const key = (a, b) => `${a} -> ${b}`;
const truthCallEdges = new Map(); // from call forms
const truthAnyEdges = new Map(); // call or value forms
for (const t of truth) {
  if (t.scope !== "internal" || !t.callee || !t.caller || t.callee === t.caller) continue;
  const k = key(t.caller, t.callee);
  if (isCallForm(t.form)) push(truthCallEdges, k, t);
  if (isCallForm(t.form) || isValueForm(t.form)) push(truthAnyEdges, k, t);
}

// ---- M1
log(`## M1 precision of our fn -> fn edges`);
const falseFn = [...ours.fn].filter((k) => !truthAnyEdges.has(k));
log(`ours ${ours.fn.size}, in truth ${ours.fn.size - falseFn.length} (${pct(ours.fn.size - falseFn.length, ours.fn.size)}), not in truth ${falseFn.length}`);
const nameOf = (id) => id.split(/::|#/).pop();
const falseDetail = [];
const falseClass = new Map();
for (const e of falseFn) {
  const [from, to] = e.split(" -> ");
  const ss = sites.filter((s) => s.caller === from && s.target === to);
  for (const s of ss) {
    const t = s.truth;
    let cls;
    if (!t) cls = "no truth fn reference at our site";
    else if (t.scope !== "internal") cls = `truth callee ${t.scope}`;
    else if (!t.callee) cls = "truth callee has no node (mapping)";
    else if (t.caller !== from) cls = "caller differs (mapping)";
    else if (t.callee !== to) {
      const trait = /#[^#]*::[^:]+$/.test(to) || /\/[A-Z][A-Za-z0-9]*#[a-z_0-9]+\(\)\.$/.test(t.symbol);
      cls = nameOf(t.callee) === nameOf(to) && trait ? "trait method vs impl method" : "other function";
    } else cls = "?";
    const what = t ? `truth: ${t.callee ?? t.symbol}` : `truth at site: ${JSON.stringify(sitesTruthAt(s))}`;
    push(falseClass, cls, `${e}  @ ${s.file}:${lineOf(s.file, s.start)} ${s.form} ${what}`);
  }
  if (!ss.length) push(falseClass, "no site of ours (?)", e);
}
function sitesTruthAt(s) {
  const d = documents.find((x) => x.path === s.file);
  return (d?.occurrences ?? []).filter((o) => { const [a, b] = bytes(s.file, o.range); return a === s.start && b === s.end; }).map((o) => o.symbol.split(" ").pop());
}
for (const [c, l] of top(falseClass)) {
  log(`   ${l.length}\t${c}`);
  for (const x of l.slice(0, 30)) log(`      ${x}`);
}

// ---- M2
log(`## M2 fn -> type edges: ours ${ours.type.size}`);
for (const e of ours.type) {
  const [from, to] = e.split(" -> ");
  const ok = truth.some((t) => t.caller === from && t.scope === "internal" && t.callee && declById.get(t.callee)?.parent === to);
  log(`   ${ok ? "correct" : "not in truth"}\t${e}`);
}

// ---- M3
log(`## M3 recall over truth call edges (call forms; callee and caller with a node; self-calls dropped)`);
const recall = { exact: [], typeOnly: [], none: [] };
for (const [k, ts] of truthCallEdges) {
  const [from, to] = k.split(" -> ");
  if (ours.fn.has(k)) recall.exact.push(k);
  else if (ours.type.has(key(from, declById.get(to)?.parent))) recall.typeOnly.push(k);
  else recall.none.push([k, ts]);
}
const nT = truthCallEdges.size;
log(`truth call edges ${nT}: exact ${recall.exact.length} (${pct(recall.exact.length, nT)}), only as type edge ${recall.typeOnly.length}, missing ${recall.none.length} (${pct(recall.none.length, nT)})`);
for (const part of [false, true]) {
  const all = [...truthCallEdges.keys()].filter((k) => declById.get(k.split(" -> ")[0])?.test === part);
  const hit = all.filter((k) => ours.fn.has(k)).length;
  log(`   ${part ? "test" : "main"}: ${all.length} edges, exact ${hit} (${pct(hit, all.length)})`);
}
const valueOnly = [...truthAnyEdges.keys()].filter((k) => !truthCallEdges.has(k));
log(`   value-only truth edges (fn named, not called): ${valueOnly.length}, ours have ${valueOnly.filter((k) => ours.fn.has(k)).length}`);
const siteLevel = truth.filter((t) => t.scope === "internal" && t.callee && t.caller && t.callee !== t.caller && isCallForm(t.form));
const siteHit = siteLevel.filter((t) => t.site?.outcome === "function" && t.site.target === t.callee).length;
log(`   site level: truth internal call sites ${siteLevel.length}, our site gives that function ${siteHit} (${pct(siteHit, siteLevel.length)})`);

// ---- M4: the cause of each missed site (all sites of the missed edges)
log(`## M4 missed internal call sites by cause (sites of missing edges / all missed sites)`);
const causeOf = (t) => {
  if (t.form === "macro-call") return "call inside macro arguments";
  if (t.form === "implicit") return `implicit call (${t.token})`;
  if (t.form === "call-unwalked") return "call outside the walked code";
  const s = t.site;
  if (!s) return "no site of ours";
  if (s.caller !== t.caller) return `caller differs (ours ${s.caller})`;
  if (s.outcome === "function") return s.target === t.callee ? "hit" : "ours: other function";
  return `ours ${s.outcome} (${s.form})`;
};
const m4 = new Map();
const m4Sites = new Map();
const missedEdge = new Set(recall.none.map(([k]) => k));
for (const t of siteLevel) {
  const c = causeOf(t);
  if (c === "hit") continue;
  push(m4Sites, c, t);
  if (missedEdge.has(key(t.caller, t.callee))) push(m4, c, t);
}
for (const [c, l] of top(m4Sites)) log(`   ${m4.get(c)?.length ?? 0} / ${l.length}\t${c}`);
// Per missed edge: macro only if every site of the edge is in macro arguments.
const edgeCause = new Map();
for (const [k, ts] of recall.none) {
  const causes = ts.map(causeOf).filter((c) => c !== "hit");
  const c = causes.every((x) => x === "call inside macro arguments") ? "call inside macro arguments" : causes.find((x) => x !== "call inside macro arguments") ?? "?";
  push(edgeCause, c, k);
}
log(`   missed edges by cause: ${top(edgeCause).map(([c, l]) => `${l.length} ${c}`).join("; ")}`);
const macroName = (t) => {
  const m = (macrosIn.get(t.file) ?? []).filter((x) => x.start <= t.start && t.start < x.end).sort((a, b) => b.start - a.start)[0];
  return m ? text(t.file, m.start, m.end).match(/^[\w:]+!?/)?.[0] : "?";
};
const byMacro = new Map();
for (const t of m4Sites.get("call inside macro arguments") ?? []) bump(byMacro, macroName(t));
log(`   internal calls in macro arguments by macro: ${[...byMacro].sort((a, b) => b[1] - a[1]).slice(0, 8).map(([k, n]) => `${k} ${n}`).join(", ")}`);
const macroTest = (m4Sites.get("call inside macro arguments") ?? []).filter((t) => declById.get(t.caller)?.test).length;
log(`   of those in test code: ${macroTest}`);
// Site-level precision: each of our function sites against the truth at the same token.
const fnSites = sites.filter((s) => s.outcome === "function");
const fnSitesOk = fnSites.filter((s) => s.truth?.callee === s.target && s.truth?.caller === s.caller);
log(`   site-level precision: our function sites ${fnSites.length}, same callee and caller in truth ${fnSitesOk.length}`);
for (const s of fnSites.filter((s) => !fnSitesOk.includes(s)).slice(0, 10)) log(`      ${s.file}:${lineOf(s.file, s.start)} ours ${s.caller} -> ${s.target}; truth ${s.truth?.caller} -> ${s.truth?.callee ?? s.truth?.symbol}`);

// ---- M5
log(`## M5 the indexer's own classification against the truth`);
const tally = new Map();
const truthAtSite = new Map();
for (const d of documents)
  for (const o of d.occurrences) {
    if (o.roles & 1) continue;
    const [a, b] = bytes(d.path, o.range);
    push(truthAtSite, `${d.path}|${a}|${b}`, o.symbol);
  }
for (const s of sites) {
  let tk;
  if (s.truth) tk = s.truth.scope === "internal" ? (s.truth.callee ? "internal" : "internal-no-node") : s.truth.scope;
  else {
    const syms = truthAtSite.get(`${s.file}|${s.start}|${s.end}`) ?? [];
    tk = syms.length ? (syms.some((x) => x.startsWith("local ")) ? "truth: local (closure var)" : "truth: non-fn " + (syms.some((x) => x.endsWith("#")) ? "type" : "symbol")) : "truth: nothing";
  }
  bump(tally, `${s.outcome.padEnd(10)} -> ${tk}`);
}
for (const [k, n] of [...tally].sort()) log(`   ${n}\t${k}`);
const macroInternal = truth.filter((t) => t.form === "macro-call" && t.scope === "internal").length;
const macroAll = truth.filter((t) => t.form === "macro-call").length;
log(`   calls in macro arguments (truth, followed by '('): ${macroAll}, internal ${macroInternal}`);

if (listDir) {
  fs.mkdirSync(listDir, { recursive: true });
  const row = (t) => [t.file, t.line, t.token, t.form, t.scope, t.caller, t.callee ?? t.symbol, t.site?.outcome ?? "-", t.site?.target ?? "-", t.site?.form ?? "-"].join("\t");
  fs.writeFileSync(path.join(listDir, "false-fn-edges.txt"), top(falseClass).map(([c, l]) => `## ${c} (${l.length})\n${l.join("\n")}`).join("\n\n") + "\n");
  fs.writeFileSync(path.join(listDir, "missed-sites-by-cause.tsv"), top(m4Sites).flatMap(([c, l]) => l.map((t) => `${c}\t${row(t)}`)).join("\n") + "\n");
  fs.writeFileSync(path.join(listDir, "truth-sites.tsv"), truth.map(row).join("\n") + "\n");
  fs.writeFileSync(path.join(listDir, "unmapped.txt"), [...unmappedCallee].map(([k, n]) => `${n}\t${k}`).concat(unmappedFnDefs.map((d) => `def\t${d.symbol}\t${d.file}:${lineOf(d.file, d.start)}\tnodes ${d.decl.length}`)).join("\n") + "\n");
}
console.log(out.join("\n"));

// Compares the `calls` edges of a cg graph with the javac ground truth of CallTruth.java.
//
//   node compare.mjs <graph.json> <truth.tsv> [<sites.tsv>] [--list <dir>]
//
// <sites.tsv> (optional) holds our per-site outcomes: lines
//   SITE file start end node-kind outcome target-or-reason caller
// from a build of the indexer patched with print-sites.patch (run.sh does it). Without it, M4 and
// M5 are skipped. --list writes the detail lists (false edges, misses) into <dir>.
import fs from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const listAt = args.indexOf("--list");
const listDir = listAt >= 0 ? args.splice(listAt, 2)[1] : null;
const [graphFile, truthFile, sitesFile] = args;

const graph = JSON.parse(fs.readFileSync(graphFile, "utf8"));
const nodes = new Map(graph.nodes.map((n) => [n.id, n]));
const isTest = (file) => !/\/src\/main\//.test(file);

// ---- our edges
const ours = { fn: new Set(), type: new Set() };
for (const e of graph.edges) {
  if (e.kind !== "calls") continue;
  const k = nodes.get(e.to)?.kind === "function" ? "fn" : "type";
  ours[k].add(`${e.from} -> ${e.to}`);
}

// ---- truth sites
const [header, ...rows] = fs.readFileSync(truthFile, "utf8").trimEnd().split("\n");
const cols = header.split("\t");
const truth = rows.map((r) => Object.fromEntries(r.split("\t").map((v, i) => [cols[i], v])));
for (const t of truth) {
  t.start = +t.start;
  t.end = +t.end;
  // Our target for the site: the function if it has a node, else its type.
  t.ownerId = `type:${t.owner}`;
  t.target = t.scope !== "internal" ? null : t.origin === "src" ? t.callee : t.origin === "local" ? null : t.ownerId;
}
const parentOf = (id) => nodes.get(id)?.parent;
// Our indexer drops edges to the caller itself and from a function to its own type.
const dropped = (from, to) => to === from || (nodes.get(from)?.kind === "function" && parentOf(from) === to);

// ---- mapping check
const unmapped = { caller: new Map(), callee: new Map() };
const note = (m, k, ex) => m.set(k, [(m.get(k)?.[0] ?? 0) + 1, m.get(k)?.[1] ?? ex]);
for (const t of truth) {
  if (!nodes.has(t.caller)) note(unmapped.caller, t.caller, `${t.file}:${t.line}`);
  if (t.scope === "internal" && t.origin === "src" && !nodes.has(t.callee)) note(unmapped.callee, t.callee, `${t.file}:${t.line}`);
}

// ---- truth edges (internal only)
const truthEdges = new Map(); // "from -> to" -> [sites]
const truthByCallerOwner = new Set(); // caller|type:owner for any internal callee (also generated)
const truthByCallerAnyOwner = new Set();
for (const t of truth) {
  if (t.scope !== "internal") continue;
  truthByCallerOwner.add(`${t.caller}|${t.ownerId}`);
  if (!t.target || dropped(t.caller, t.target)) continue;
  const key = `${t.caller} -> ${t.target}`;
  if (!truthEdges.has(key)) truthEdges.set(key, []);
  truthEdges.get(key).push(t);
}
const truthFnEdges = [...truthEdges.keys()].filter((k) => nodes.get(k.split(" -> ")[1])?.kind === "function");

// ---- our sites and the join
let sites = null;
const truthAt = new Map(); // file|start|end -> truth site
const truthAtStart = new Map(); // file|start -> [truth sites]
for (const t of truth) {
  truthAt.set(`${t.file}|${t.start}|${t.end}`, t);
  const k = `${t.file}|${t.start}`;
  if (!truthAtStart.has(k)) truthAtStart.set(k, []);
  truthAtStart.get(k).push(t);
}
const kindOf = { method_invocation: ["call"], object_creation_expression: ["new", "new-anon"], method_reference: ["ref"], explicit_constructor_invocation: ["super", "this"] };
if (sitesFile) {
  sites = fs
    .readFileSync(sitesFile, "utf8")
    .split("\n")
    .filter((l) => l.startsWith("SITE\t"))
    .map((l) => {
      const [, file, start, end, nodeKind, outcome, target, caller] = l.split("\t");
      return { file, start: +start, end: +end, nodeKind, outcome, target, caller };
    });
  for (const s of sites) {
    s.truth = truthAt.get(`${s.file}|${s.start}|${s.end}`);
    if (!s.truth) {
      const c = (truthAtStart.get(`${s.file}|${s.start}`) ?? []).filter((t) => kindOf[s.nodeKind].includes(t.kind) && !t.joined);
      if (c.length === 1) s.truth = c[0];
    }
    if (s.truth) s.truth.joined = s;
  }
}
const siteKey = (s) => `${s.file}:${s.truth?.line ?? "?"}`;

const out = [];
const log = (...a) => out.push(a.join(" "));
const pct = (a, b) => (b ? ((100 * a) / b).toFixed(1) + " %" : "-");

// ---- M1
const falseFn = [...ours.fn].filter((k) => !truthEdges.has(k));
log(`M1 fn->fn edges: ${ours.fn.size}, in truth ${ours.fn.size - falseFn.length} (${pct(ours.fn.size - falseFn.length, ours.fn.size)}), not in truth ${falseFn.length}`);
const explainFalse = (edge) => {
  const [from, to] = edge.split(" -> ");
  if (!sites) return "";
  const ss = sites.filter((s) => s.caller === from && s.target === to);
  return ss
    .map((s) => {
      const t = s.truth;
      if (!t) return `${s.file}@${s.start} no javac site`;
      return `${siteKey(s)} javac: caller=${t.caller} callee=${t.scope === "internal" ? t.callee : t.owner + "." + "(external)"} origin=${t.origin}`;
    })
    .join(" | ");
};
const falseFnList = falseFn.map((e) => `${e}\n    ${explainFalse(e)}`);

// ---- M2
const typeRes = { correct: [], wrong: [] };
for (const e of ours.type) {
  const [from, to] = e.split(" -> ");
  (truthByCallerOwner.has(`${from}|${to}`) ? typeRes.correct : typeRes.wrong).push(e);
}
log(`M2 fn->type edges: ${ours.type.size}, correct ${typeRes.correct.length} (${pct(typeRes.correct.length, ours.type.size)}), not in truth ${typeRes.wrong.length}`);
const falseTypeList = typeRes.wrong.map((e) => `${e}\n    ${explainFalse(e)}`);

// ---- M3
const recall = { fn: 0, typeOnly: 0, none: [] };
const typeHit = (from, to) => {
  const owner = nodes.get(to)?.kind === "function" ? parentOf(to) : to;
  return ours.type.has(`${from} -> ${owner}`);
};
for (const [k, ss] of truthEdges) {
  const [from, to] = k.split(" -> ");
  if (ours.fn.has(k) || (nodes.get(to)?.kind !== "function" && ours.type.has(k))) recall.fn++;
  else if (typeHit(from, to)) recall.typeOnly++;
  else recall.none.push([k, ss]);
}
const nTruth = truthEdges.size;
log(`M3 truth internal edges: ${nTruth} (to a function node ${truthFnEdges.length}, to a type because the callee has no node ${nTruth - truthFnEdges.length})`);
log(`   exact ${recall.fn} (${pct(recall.fn, nTruth)}), only as type edge ${recall.typeOnly} (${pct(recall.typeOnly, nTruth)}), missing ${recall.none.length} (${pct(recall.none.length, nTruth)})`);
for (const part of ["main", "test"]) {
  const inPart = (k) => (isTest(truthEdges.get(k)[0].file) ? "test" : "main") === part;
  const all = [...truthEdges.keys()].filter(inPart);
  const miss = recall.none.filter(([k]) => inPart(k)).length;
  log(`   ${part}: ${all.length} truth edges, missing ${miss} (${pct(miss, all.length)})`);
}

// ---- M4: the cause of each missed edge, from the first site of the edge
const cause = (t) => {
  const tag = t.origin === "src" || t.origin === "-" ? "" : ` [callee ${t.origin}]`;
  if (t.kind === "enumconst") return "enum constant (not a call node in tree-sitter)";
  const s = t.joined;
  if (!s) return "no site of ours at the position" + tag;
  if (s.caller !== t.caller) return `caller attribution differs (ours ${s.outcome})` + tag;
  if (s.outcome === "external") return `ours external` + tag;
  if (s.outcome === "unresolved") return `unresolved: ${s.target}` + tag;
  if (s.outcome === "type") {
    if (s.target === parentOf(t.caller) || s.target === t.caller) return "type edge dropped: callee in the caller's own type (overload)" + tag;
    if (s.target !== t.ownerId) return "type edge to the receiver's type, callee declared in a supertype" + tag;
    return "type edge, other" + tag;
  }
  if (s.outcome === "function" && s.target !== t.target) return `ours: other function` + tag;
  return `ours: ${s.outcome}` + tag;
};
const m4 = new Map();
for (const [k, ss] of recall.none) {
  const c = cause(ss[0]);
  if (!m4.has(c)) m4.set(c, []);
  m4.get(c).push(`${k}  @ ${ss[0].file}:${ss[0].line}`);
}
log(`M4 missing edges by cause (first site of the edge):`);
for (const [c, l] of [...m4].sort((a, b) => b[1].length - a[1].length)) log(`   ${l.length}\t${c}`);

// site-level detail of the unresolved internal sites: our reason x javac receiver
if (sites) {
  const det = new Map();
  for (const s of sites) {
    const t = s.truth;
    if (!t || t.scope !== "internal" || s.outcome !== "unresolved") continue;
    const k = `${s.target} | recv ${t.recvKind}/${t.recvSym}/${t.recvDecl} | ${t.origin}`;
    det.set(k, (det.get(k) ?? 0) + 1);
  }
  log(`M4b unresolved internal SITES by (our reason | javac receiver | callee origin), top 25:`);
  for (const [k, n] of [...det].sort((a, b) => b[1] - a[1]).slice(0, 25)) log(`   ${n}\t${k}`);
}

// ---- M5
if (sites) {
  const tally = new Map();
  let unjoined = 0;
  for (const s of sites) {
    const t = s.truth;
    if (!t) { unjoined++; continue; }
    const tk = t.scope === "internal" ? `internal/${t.origin}` : t.scope;
    const k = `${s.outcome} -> ${tk}`;
    tally.set(k, (tally.get(k) ?? 0) + 1);
  }
  const truthOnly = truth.filter((t) => !t.joined);
  const toTally = new Map();
  for (const t of truthOnly) toTally.set(`${t.kind}/${t.scope}`, (toTally.get(`${t.kind}/${t.scope}`) ?? 0) + 1);
  log(`M5 our sites ${sites.length}, joined to a javac site ${sites.length - unjoined}, unjoined ${unjoined}; javac sites ${truth.length}, without a site of ours ${truthOnly.length}: ${JSON.stringify(Object.fromEntries(toTally))}`);
  log(`   javac: internal ${truth.filter((t) => t.scope === "internal").length}, external ${truth.filter((t) => t.scope === "external").length}, unattributed ${truth.filter((t) => t.scope === "unattributed").length}`);
  for (const [k, n] of [...tally].sort()) log(`   ${n}\t${k}`);
  if (listDir) {
    fs.writeFileSync(path.join(listDir, "unjoined-ours.txt"), sites.filter((s) => !s.truth).map((s) => `${s.file}@${s.start}-${s.end} ${s.nodeKind} ${s.outcome} ${s.target}`).join("\n") + "\n");
    fs.writeFileSync(path.join(listDir, "external-but-internal.txt"), sites.filter((s) => s.truth && s.outcome === "external" && s.truth.scope === "internal").map((s) => `${siteKey(s)} ${s.truth.kind} callee=${s.truth.callee} origin=${s.truth.origin} recv=${s.truth.recvKind}/${s.truth.recvSym} ${s.truth.recvType}`).join("\n") + "\n");
    fs.writeFileSync(path.join(listDir, "sites-joined.tsv"), sites.filter((s) => s.truth).map((s) => [s.file, s.truth.line, s.nodeKind, s.outcome, s.target, s.caller, s.truth.kind, s.truth.caller, s.truth.callee, s.truth.owner, s.truth.scope, s.truth.origin, s.truth.recvKind, s.truth.recvSym, s.truth.recvDecl, s.truth.recvScope, s.truth.recvType, s.truth.static].join("\t")).join("\n") + "\n");
  }
}

log(`Mapping: truth callers without a node ${[...unmapped.caller.values()].reduce((a, [n]) => a + n, 0)} sites / ${unmapped.caller.size} ids; internal src callees without a node ${[...unmapped.callee.values()].reduce((a, [n]) => a + n, 0)} sites / ${unmapped.callee.size} ids`);
for (const [id, [n, ex]] of [...unmapped.caller, ...unmapped.callee].slice(0, 15)) log(`   ${n}\t${id}\t${ex}`);

if (listDir) {
  fs.writeFileSync(path.join(listDir, "false-fn-edges.txt"), falseFnList.join("\n") + "\n");
  fs.writeFileSync(path.join(listDir, "false-type-edges.txt"), falseTypeList.join("\n") + "\n");
  fs.writeFileSync(path.join(listDir, "missing-by-cause.txt"), [...m4].map(([c, l]) => `## ${c} (${l.length})\n${l.join("\n")}`).join("\n\n") + "\n");
  fs.writeFileSync(path.join(listDir, "unmapped.txt"), [...unmapped.caller, ...unmapped.callee].map(([id, [n, ex]]) => `${n}\t${id}\t${ex}`).join("\n") + "\n");
}
console.log(out.join("\n"));

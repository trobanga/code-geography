// Decodes a SCIP index (protobuf, https://github.com/sourcegraph/scip/blob/main/scip.proto)
// without a protobuf library. Only the fields that compare.mjs needs are read.
//
//   import { readScip } from "./scip.mjs";
//   const { documents } = readScip(buffer);
//   // documents: [{ path, encoding, occurrences: [{ range, symbol, roles, enclosing }],
//   //               symbols: [{ symbol, kind, displayName, enclosing, relationships }] }]

class Reader {
  constructor(buf, start = 0, end = buf.length) {
    this.buf = buf;
    this.pos = start;
    this.end = end;
  }
  done() {
    return this.pos >= this.end;
  }
  varint() {
    let result = 0n;
    let shift = 0n;
    for (;;) {
      const b = this.buf[this.pos++];
      result |= BigInt(b & 0x7f) << shift;
      if (!(b & 0x80)) break;
      shift += 7n;
    }
    return result;
  }
  int() {
    // int32 is sign-extended to 64 bits on the wire.
    return Number(BigInt.asIntN(32, this.varint()));
  }
  bytes() {
    const n = Number(this.varint());
    const start = this.pos;
    this.pos += n;
    return [start, this.pos];
  }
  string() {
    const [s, e] = this.bytes();
    return this.buf.toString("utf8", s, e);
  }
  skip(wire) {
    if (wire === 0) this.varint();
    else if (wire === 1) this.pos += 8;
    else if (wire === 2) this.bytes();
    else if (wire === 5) this.pos += 4;
    else throw new Error(`wire type ${wire} at ${this.pos}`);
  }
  *fields() {
    while (!this.done()) {
      const key = Number(this.varint());
      yield [key >>> 3, key & 7];
    }
  }
  sub() {
    const [s, e] = this.bytes();
    return new Reader(this.buf, s, e);
  }
  packed(wire) {
    if (wire !== 2) return [this.int()];
    const r = this.sub();
    const out = [];
    while (!r.done()) out.push(r.int());
    return out;
  }
}

function occurrence(r) {
  const o = { range: [], symbol: "", roles: 0, enclosing: [] };
  for (const [f, w] of r.fields()) {
    if (f === 1) o.range.push(...r.packed(w));
    else if (f === 2) o.symbol = r.string();
    else if (f === 3) o.roles = r.int();
    else if (f === 7) o.enclosing.push(...r.packed(w));
    else r.skip(w);
  }
  return o;
}

function relationship(r) {
  const o = { symbol: "", isReference: false, isImplementation: false, isTypeDefinition: false, isDefinition: false };
  for (const [f, w] of r.fields()) {
    if (f === 1) o.symbol = r.string();
    else if (f === 2) o.isReference = r.int() !== 0;
    else if (f === 3) o.isImplementation = r.int() !== 0;
    else if (f === 4) o.isTypeDefinition = r.int() !== 0;
    else if (f === 5) o.isDefinition = r.int() !== 0;
    else r.skip(w);
  }
  return o;
}

function symbolInformation(r) {
  const o = { symbol: "", kind: 0, displayName: "", enclosing: "", relationships: [] };
  for (const [f, w] of r.fields()) {
    if (f === 1) o.symbol = r.string();
    else if (f === 4) o.relationships.push(relationship(r.sub()));
    else if (f === 5) o.kind = r.int();
    else if (f === 6) o.displayName = r.string();
    else if (f === 8) o.enclosing = r.string();
    else r.skip(w);
  }
  return o;
}

function document(r) {
  const d = { path: "", encoding: 0, occurrences: [], symbols: [] };
  for (const [f, w] of r.fields()) {
    if (f === 1) d.path = r.string();
    else if (f === 2) d.occurrences.push(occurrence(r.sub()));
    else if (f === 3) d.symbols.push(symbolInformation(r.sub()));
    else if (f === 6) d.encoding = r.int();
    else r.skip(w);
  }
  return d;
}

export function readScip(buf) {
  const r = new Reader(buf);
  const documents = [];
  const external = [];
  for (const [f, w] of r.fields()) {
    if (f === 2) documents.push(document(r.sub()));
    else if (f === 3) external.push(symbolInformation(r.sub()));
    else r.skip(w);
  }
  return { documents, external };
}

/** [startLine, startChar, endLine, endChar] from the 3- or 4-element SCIP form. */
export function range4(range) {
  return range.length === 3 ? [range[0], range[1], range[0], range[2]] : range;
}

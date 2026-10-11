// SPDX-License-Identifier: AGPL-3.0-or-later
//
// CErXes's JavaScript output (src/emit.rs) run by Node: every runtime
// conformance probe (Ranger's tests/runtime-conformance.test.ts, plain
// JavaScript) read as TSX, written out as JavaScript by CErXes compiled to
// JS by rgrc (bin/Cerxes.cjs), and run in a fresh Node context after
// `runtime()`. It must answer what Node answers for the probe itself.
//
// Where CErXes itself answers differently from Node, its reading of the
// source is what differs (BigInt literals, a comment ended by U+2028, an
// escaped "use strict", a function's source text): the output can only
// mean what CErXes read. Those probes are counted, not failed; a probe
// CErXes gets right and its output gets wrong is a failure.
//
//   node cerxes/bench/emit.mjs            # needs bin/Cerxes.cjs (cases.mjs --build)
//   node cerxes/bench/emit.mjs --build    # rebuild it first
//   node cerxes/bench/emit.mjs --list     # list the probes that differ
//
// Exits 1 when any probe answers differently, or its output does not parse.
import fs from "fs";
import path from "path";
import vm from "vm";
import { fileURLToPath } from "url";
import { createRequire } from "module";
import { RANGER } from "../../cer/bench/common.mjs";
import { compile } from "../../scripts/compiler.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const OUT = path.resolve(HERE, "../bin/Cerxes.cjs");
const args = process.argv.slice(2);
if (args.includes("--build") || !fs.existsSync(OUT)) compile(["-es6", "-nodemodule", "cerxes/src/lib.rs", "-d=cerxes/bin", "-o=Cerxes.cjs"]);
const { Emit, Syntax, Engine } = createRequire(import.meta.url)(OUT);

/** The array literal after `marker` in the test source (as conformance.mjs reads it). */
function literalAfter(src, marker) {
  let i = src.indexOf("[", src.indexOf("=", src.indexOf(marker) + marker.length));
  const start = i;
  let depth = 0;
  let quote = null;
  for (; i < src.length; i++) {
    const c = src[i];
    if (quote) {
      if (c === "\\") i++;
      else if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") quote = c;
    else if (c === "/" && src[i + 1] === "/") i = src.indexOf("\n", i);
    else if (c === "[") depth++;
    else if (c === "]" && --depth === 0) return src.slice(start, i + 1);
  }
  throw new Error("unbalanced: " + marker);
}

function tag(v) {
  if (v === undefined) return "u";
  if (v === null) return "l";
  if (typeof v === "boolean") return "b:" + v;
  if (typeof v === "number") return Object.is(v, -0) ? "n:-0" : "n:" + String(v);
  if (typeof v === "string") return "s:" + v;
  return "o:" + typeof v;
}

// a probe runs in a context of its own, as in a fresh engine
function run(code, runtime) {
  const ctx = vm.createContext({});
  try {
    if (runtime) vm.runInContext(runtime, ctx);
    return tag(vm.runInContext(code, ctx, { timeout: 5000 }));
  } catch (e) {
    return "t:" + (e && e.name);
  }
}

const PROBES = new Function("return " + literalAfter(fs.readFileSync(path.join(RANGER, "tests/runtime-conformance.test.ts"), "utf8"), "const PROBES"))();
const runtime = Emit.runtime();
const script = (body) => "(function () { " + body + "\n})();";
const differ = [];
let unread = 0;
let cerxesDiffers = 0;
for (const [name, body, group] of PROBES) {
  const src = script(body);
  const want = run(src, "");
  const r = Emit.emit_js(src, Syntax.tsx());
  // a probe CErXes cannot read (a syntax it lacks) has no output to check
  if (r.error) {
    unread++;
    continue;
  }
  try {
    new vm.Script(r.js);
  } catch (e) {
    differ.push([group, name, want, "does not parse: " + e.message, r.js]);
    continue;
  }
  const got = run(r.js, runtime);
  if (got === want) continue;
  // what CErXes answers for the probe (as conformance.mjs compares: any
  // two thrown values agree)
  let own;
  try {
    own = Engine.new_().eval_typed(src);
  } catch (e) {
    own = "t:host " + e;
  }
  if (!(own === want || (own.startsWith("t:") && want.startsWith("t:")))) {
    cerxesDiffers++;
    continue;
  }
  differ.push([group, name, want, got, r.js]);
}
console.log(`probes: ${PROBES.length}, not read by CErXes: ${unread}, emitted and run by Node: ${PROBES.length - unread}`);
console.log(`answer differently from Node: ${differ.length} (and ${cerxesDiffers} that CErXes itself answers differently)`);
if (args.includes("--list") || differ.length) {
  for (const [g, n, want, got, js] of differ) console.log(`  ${g}/${n}: want ${JSON.stringify(want).slice(0, 80)} got ${JSON.stringify(got).slice(0, 120)}\n    ${js.replace(/\s+/g, " ").slice(0, 300)}`);
}
if (differ.length) process.exit(1);

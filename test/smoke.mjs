// SPDX-License-Identifier: AGPL-3.0-or-later
// Runs a few scripts through bin/engine_module.cjs and compares what they
// print. `npm test` builds the module first.
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);
const mod = require(path.join(ROOT, "bin/engine_module.cjs"));

function run(src) {
  const lines = [];
  const saved = console.log;
  console.log = (...a) => lines.push(a.map(String).join(" ").replace(/^\[tsx\]\s*/, ""));
  try {
    const e = new mod.ComponentEngine();
    e.quiet = true;
    e.loadScript(src + "\nfunction __done__() { return 1; }\n");
    e.callFunction("__done__", (mod.EvHandle || mod.EvalValue).null());
  } finally {
    console.log = saved;
  }
  return lines.join("\n");
}

const cases = [
  ["arithmetic", "console.log(1 + 2 * 3, 7 / 2, 2 ** 10);", "7 3.5 1024"],
  ["closures", "function c(){let n=0;return()=>++n}const f=c();f();console.log(f());", "2"],
  ["classes", "class A{constructor(x){this.x=x}get d(){return this.x*2}}class B extends A{}console.log(new B(4).d);", "8"],
  ["array methods", "console.log([3,1,2].sort().map(x=>x*10).join(','));", "10,20,30"],
  ["destructuring", "const {a, b:[c]} = {a: 1, b: [2]}; console.log(a + c);", "3"],
  ["template + JSON", "const o={k:[1,'x']};console.log(`${JSON.stringify(o)}`);", '{"k":[1,"x"]}'],
  ["Map", "const m=new Map([[1,'a']]);m.set(2,'b');console.log(m.size, m.get(2));", "2 b"],
  ["regex", "console.log('a1b22'.replace(/\\d+/g, n => '<' + n + '>'));", "a<1>b<22>"],
  ["try/catch", "try { null.x } catch (e) { console.log(e instanceof TypeError) }", "true"],
];

let failed = 0;
for (const [name, src, want] of cases) {
  let got;
  try {
    got = run(src);
  } catch (e) {
    got = "threw " + e;
  }
  if (got === want) {
    console.log("  PASS " + name);
  } else {
    failed++;
    console.log("  FAIL " + name + "\n    want " + JSON.stringify(want) + "\n    got  " + JSON.stringify(got));
  }
}
if (failed) {
  console.log(failed + " failed");
  process.exit(1);
}
console.log("ALL PASS");

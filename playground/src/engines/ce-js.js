// SPDX-License-Identifier: AGPL-3.0-or-later
// ComponentEngine compiled by rgrc to JavaScript (-es6 -nodemodule), the
// same build Ranger's runtime-conformance suite drives.
import mod from "../../../bin/engine_module.cjs";

export default {
  id: "ce-js",
  name: "CE · Ranger → JS",
  about: "ComponentEngine's Ranger sources compiled by rgrc to JavaScript: an interpreter running inside the browser's JIT.",
  async load() {
    const Null = (mod.EvHandle || mod.EvalValue).null();
    return {
      run(src) {
        const out = [];
        const saved = console.log;
        console.log = (...a) => out.push(a.map(String).join(" ").replace(/^\[tsx\]\s*/, ""));
        try {
          const e = new mod.ComponentEngine();
          e.quiet = true;
          e.liveClock = true;
          e.maxLoopIterations = 1000000000;
          e.loadScript(src + "\nfunction __done__() { return 1; }\n");
          e.callFunction("__done__", Null);
        } catch (err) {
          out.push("Uncaught (engine): " + err);
        } finally {
          console.log = saved;
        }
        return { out, error: out.some((l) => l.startsWith("Uncaught")) };
      },
    };
  },
};

// SPDX-License-Identifier: AGPL-3.0-or-later
// The browser's own JavaScript engine (V8, SpiderMonkey, JavaScriptCore):
// the script compiled with `new Function` and run in the worker.

export default {
  id: "native",
  name: "Browser JS",
  about: "The browser's own engine (JIT), running the same script through new Function.",
  async load() {
    return {
      run(src) {
        const out = [];
        const log = (...a) => out.push(a.map(String).join(" "));
        try {
          new Function("print", "console", src)(log, { log, warn: log, error: log });
          return { out, error: false };
        } catch (e) {
          out.push("Uncaught " + (e && e.name ? e.name + ": " + e.message : String(e)));
          return { out, error: true };
        }
      },
    };
  },
};

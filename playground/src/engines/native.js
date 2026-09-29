// SPDX-License-Identifier: AGPL-3.0-or-later
// The browser's own JavaScript engine (V8, SpiderMonkey, JavaScriptCore):
// the script run as a global script (indirect eval) in the worker, with
// print and console.log captured.

export default {
  id: "native",
  name: "Browser JS",
  about: "The browser's own engine (JIT), running the same script as a global script in a worker.",
  async load() {
    let out = [];
    const log = (...a) => out.push(a.map(String).join(" "));
    const con = { log, info: log, warn: log, error: log, debug: log };
    const geval = eval;
    // A script may declare its own setTimeout (the compat-table tests do).
    const timeout = self.setTimeout.bind(self);
    // An error a promise job or timer throws is the script's output, not the
    // worker's failure.
    self.addEventListener("error", (ev) => {
      ev.preventDefault();
      out.push("Uncaught " + ev.message);
    });
    self.addEventListener("unhandledrejection", (ev) => ev.preventDefault());
    return {
      run(src) {
        out = [];
        const saved = self.console;
        self.print = log;
        self.console = con;
        try {
          geval(src);
          return { out, error: false };
        } catch (e) {
          out.push("Uncaught " + (e && e.name ? e.name + ": " + e.message : String(e)));
          return { out, error: true };
        } finally {
          self.console = saved;
        }
      },
      /** Promise jobs run before the next task; a test's own timer queue
       * drains through them. */
      async settle() {
        const saved = self.console;
        self.console = con;
        await new Promise((r) => timeout(r, 0));
        self.console = saved;
      },
    };
  },
};

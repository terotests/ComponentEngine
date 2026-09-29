// SPDX-License-Identifier: AGPL-3.0-or-later
// JS-Interpreter (Neil Fraser, Apache-2.0): a sandboxed, step-by-step ES5
// interpreter written in JavaScript. ES5 only: arrow functions, let/const
// and classes are syntax errors to it.
import Interpreter from "js-interpreter";

// RegExp in its sandboxed mode (2) runs in Node's vm module or a worker of
// its own; in this worker the native RegExp (1) is what it can use.
Interpreter.prototype.REGEXP_MODE = 1;

export default {
  id: "js-interpreter",
  name: "JS-Interpreter · ES5",
  about: "Neil Fraser's JS-Interpreter, a step-by-step ES5 interpreter written in JavaScript. ES5 only, so later syntax fails.",
  async load() {
    return {
      run(src) {
        const out = [];
        const init = (interp, global) => {
          const log = interp.createNativeFunction((...a) => {
            out.push(a.map((v) => String(interp.pseudoToNative(v))).join(" "));
          });
          interp.setProperty(global, "print", log);
          const con = interp.nativeToPseudo({});
          interp.setProperty(con, "log", log);
          interp.setProperty(global, "console", con);
        };
        try {
          const interp = new Interpreter(src, init);
          interp.run();
          return { out, error: false };
        } catch (e) {
          out.push("Uncaught " + (e && e.name ? e.name + ": " + e.message : String(e)));
          return { out, error: true };
        }
      },
    };
  },
};

// SPDX-License-Identifier: AGPL-3.0-or-later
// Sval (MIT): a JavaScript interpreter written in JavaScript, walking the
// Acorn syntax tree.
import Sval from "sval";

export default {
  id: "sval",
  name: "Sval · JS interpreter",
  about: "Sval, an AST-walking JavaScript interpreter written in JavaScript (ES2019+ syntax).",
  async load() {
    let out = [];
    const log = (...a) => out.push(a.map(String).join(" "));
    return {
      run(src) {
        out = [];
        const interp = new Sval({ ecmaVer: "latest", sandBox: true });
        interp.import({ print: log, console: { log, warn: log, error: log } });
        try {
          interp.run(src);
          return { out, error: false };
        } catch (e) {
          out.push("Uncaught " + (e && e.name ? e.name + ": " + e.message : String(e)));
          return { out, error: true };
        }
      },
    };
  },
};

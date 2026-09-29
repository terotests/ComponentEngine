// SPDX-License-Identifier: AGPL-3.0-or-later
// QuickJS (Fabrice Bellard, MIT) compiled to WebAssembly by emscripten, from
// the quickjs-emscripten packages.
import { newQuickJSWASMModuleFromVariant } from "quickjs-emscripten-core";
import variant from "@jitl/quickjs-singlefile-browser-release-sync";

export default {
  id: "quickjs",
  name: "QuickJS · C → WASM",
  about: "Fabrice Bellard's QuickJS, a small bytecode interpreter in C, compiled by emscripten (quickjs-emscripten).",
  async load() {
    const QuickJS = await newQuickJSWASMModuleFromVariant(variant);
    const runtime = QuickJS.newRuntime();
    runtime.setMaxStackSize(4 * 1024 * 1024);
    const vm = runtime.newContext();
    let out = [];
    const log = vm.newFunction("log", (...args) => {
      out.push(args.map((h) => String(vm.dump(h))).join(" "));
    });
    vm.setProp(vm.global, "print", log);
    const con = vm.newObject();
    vm.setProp(con, "log", log);
    vm.setProp(vm.global, "console", con);
    con.dispose();
    log.dispose();
    return {
      run(src) {
        out = [];
        const r = vm.evalCode(src);
        let error = false;
        if (r.error) {
          const e = vm.dump(r.error);
          out.push("Uncaught " + (e && e.name ? e.name + ": " + e.message : String(e)));
          r.error.dispose();
          error = true;
        } else {
          r.value.dispose();
        }
        runtime.executePendingJobs();
        return { out, error };
      },
    };
  },
};

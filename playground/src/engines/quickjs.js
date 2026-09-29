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
    let out = [];
    // print, console.log and performance.now (Octane's timer) on a context
    const context = () => {
      const vm = runtime.newContext();
      const log = vm.newFunction("log", (...args) => {
        out.push(args.map((h) => String(vm.dump(h))).join(" "));
      });
      vm.setProp(vm.global, "print", log);
      const con = vm.newObject();
      vm.setProp(con, "log", log);
      vm.setProp(vm.global, "console", con);
      const now = vm.newFunction("now", () => vm.newNumber(performance.now()));
      const perf = vm.newObject();
      vm.setProp(perf, "now", now);
      vm.setProp(vm.global, "performance", perf);
      for (const h of [con, log, now, perf]) h.dispose();
      return vm;
    };
    let vm = context();
    return {
      reset() {
        vm.dispose();
        vm = context();
      },
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

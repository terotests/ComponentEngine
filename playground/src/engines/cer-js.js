// SPDX-License-Identifier: AGPL-3.0-or-later
// CEr (the Rust sources in cer/) compiled by rgrc to JavaScript.
import mod from "../../../cer/bin/Cer.cjs";

export default {
  id: "cer-js",
  name: "CEr · Rust → Ranger → JS",
  about: "The same Rust source as CEr · WASM, read by rgrc as a strict Rust module and written out as JavaScript.",
  async load() {
    const e = mod.Engine.new_();
    return {
      run(src) {
        e.clear_output();
        const r = e.eval(src);
        const out = [];
        for (let i = 0; i < e.output_count(); i++) out.push(e.output_at(i));
        const error = e.error !== "";
        if (error) out.push(r);
        return { out, error };
      },
    };
  },
};

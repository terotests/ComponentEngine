// SPDX-License-Identifier: AGPL-3.0-or-later
import { cerxes } from "./cerxes.js";

export default cerxes({
  id: "cerxes-jit",
  name: "CErXes · JIT",
  about: "CErXes built with the baseline JIT (wjit): register code that runs 64 times is compiled to WebAssembly, which the browser compiles again to machine code.",
  wasm: "cerxes-jit.wasm",
  flags: 0,
  jit: 64,
  optional: true,
});

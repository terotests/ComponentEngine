// SPDX-License-Identifier: AGPL-3.0-or-later
import { cerxes } from "./cerxes.js";

export default cerxes({
  id: "cerxes-wasm",
  name: "CErXes · register VM",
  about: "CErXes (TypeScript + JSX on CEr's VM) by cargo to WebAssembly, opt-level 3, on the register machine: the default build.",
  wasm: "cerxes.wasm",
  flags: 0,
});

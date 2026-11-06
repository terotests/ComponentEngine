// SPDX-License-Identifier: AGPL-3.0-or-later
import { cerxes } from "./cerxes.js";

export default cerxes({
  id: "cerxes-stack",
  name: "CErXes · stack VM",
  about: "The same cerxes.wasm on the older stack machine (vm.rs), for comparing the two interpreters.",
  wasm: "cerxes.wasm",
  flags: 1,
});

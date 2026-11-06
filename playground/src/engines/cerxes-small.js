// SPDX-License-Identifier: AGPL-3.0-or-later
import { cerxes } from "./cerxes.js";

export default cerxes({
  id: "cerxes-small",
  name: "CErXes · small build",
  about: "CErXes built for size (opt-level s) on the register machine: a smaller download for some speed.",
  wasm: "cerxes-small.wasm",
  flags: 0,
});

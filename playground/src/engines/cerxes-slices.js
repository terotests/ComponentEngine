// SPDX-License-Identifier: AGPL-3.0-or-later
import { cerxes } from "./cerxes.js";

export default cerxes({
  id: "cerxes-slices",
  name: "CErXes · detachable",
  about: "cerxes.wasm on the register VM in 4 ms time slices: the running engine can be stopped and written out as a snapshot at any branch, call or return. What it costs to be able to move.",
  wasm: "cerxes.wasm",
  flags: 0,
  slice: 4,
});

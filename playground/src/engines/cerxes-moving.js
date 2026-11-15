// SPDX-License-Identifier: AGPL-3.0-or-later
import { cerxes } from "./cerxes.js";

export default cerxes({
  id: "cerxes-moving",
  name: "CErXes · moving",
  about: "As CErXes · detachable, and every 100 ms of the run the engine is written out as a snapshot and replaced by a new engine made from it, mid-script, as an engine travelling between hosts would be. What moving costs.",
  wasm: "cerxes.wasm",
  flags: 0,
  slice: 4,
  move: 100,
});

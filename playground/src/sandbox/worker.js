// SPDX-License-Identifier: AGPL-3.0-or-later
// One sandbox host in a Web Worker: core.js on its own thread.
import { loadCerxes, serve, messageYield } from "./core.js";

const handle = serve(loadCerxes(new URL("../wasm/cerxes.wasm", import.meta.url)), (m) => self.postMessage(m), messageYield());
self.onmessage = (ev) => handle(ev.data);

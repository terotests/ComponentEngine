// SPDX-License-Identifier: AGPL-3.0-or-later
// One sandbox host in an iframe: core.js in a document of its own (its own
// WebAssembly instance and globals), talking to the page by postMessage. A
// same-origin iframe shares the page's thread, so its slices matter as much
// as the page's own; it yields with setTimeout, as the page does, so the
// page still gets its frames painted.
import { loadCerxes, serve, timeoutYield } from "./core.js";

const handle = serve(loadCerxes(new URL("../wasm/cerxes.wasm", import.meta.url)), (m) => parent.postMessage(m, location.origin), timeoutYield);
addEventListener("message", (ev) => {
  if (ev.source === parent && ev.origin === location.origin) handle(ev.data);
});
parent.postMessage({ type: "ready" }, location.origin);

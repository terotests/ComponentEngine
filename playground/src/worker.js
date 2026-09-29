// SPDX-License-Identifier: AGPL-3.0-or-later
//
// One engine per worker. build.mjs bundles this with one adapter from
// engines/ into workers/<id>.js. Messages:
//   { cmd: "run", id, src, reps }  ->  { id, ok, out, error, times: [ms…] }
// A run that never ends is ended by the page, which terminates the worker.

export function serve(engine) {
  let ready = null;
  const load = () => (ready ||= engine.load(new URL("../", self.location.href).href));

  self.onmessage = async (ev) => {
    const { cmd, id, src, reps } = ev.data;
    if (cmd !== "run") return;
    try {
      const t0 = performance.now();
      const inst = await load();
      const loadMs = performance.now() - t0;
      const times = [];
      let res = { out: [], error: false };
      for (let i = 0; i < (reps || 1); i++) {
        const t = performance.now();
        res = inst.run(src);
        times.push(performance.now() - t);
        if (res.error) break;
      }
      self.postMessage({ id, ok: true, out: res.out, error: res.error, times, loadMs });
    } catch (e) {
      self.postMessage({ id, ok: false, out: [String(e && e.stack ? e.stack : e)], error: true, times: [] });
    }
  };
  self.postMessage({ ready: engine.id });
}

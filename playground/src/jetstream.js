// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The "JetStream 3" tab: the JavaScript benchmarks of WebKit's JetStream 3
// (the ones it runs by default), run in every ticked engine as JetStream's
// shell driver runs them (jetstream-core.js), scored as JetStream scores
// them. The material is fetched at build time (jetstream/update.mjs) into
// jetstream/: manifest.json and the workloads' files.
//
// One benchmark at a time, in every ticked engine in turn (the browser's
// own engine first), each in a fresh realm: for the browser, a fresh worker.

import { script, parse, subScores, groupScores, scoreOf, geomean, plan } from "./jetstream-core.js";

const $ = (id) => document.getElementById(id);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const fmt = (v) => (v == null ? "" : v >= 100 ? String(Math.round(v)) : v >= 10 ? v.toFixed(1) : v >= 1 ? v.toFixed(2) : v.toPrecision(2));
// benchmarks whose output differs from run to run in the same engine
// (timestamps, ids): their digest is not compared
const VARIES = new Set(["first-inspector-code-load", "multi-inspector-code-load", "proxy-mobx", "web-ssr"]);
/** "same", "differs" or null (nothing to compare) for an engine's result
 * against the browser's. */
const sameAs = (r, nat, b) => (VARIES.has(b) || !r || !nat || !r.digest || !nat.digest ? null : r.digest === nat.digest ? "same" : "differs");
const fmtMs = (v) => (v >= 1000 ? (v / 1000).toFixed(1) + " s" : v.toFixed(v >= 10 ? 0 : 1) + " ms");

/**
 * ctx: { chosen() -> engines, runnerFor(e), store, rev, setEngineState }
 */
export function setupJetStream(ctx) {
  let manifest = null;
  let stop = false;
  let running = null; // { b, id }: the cell being run now
  let results = ctx.store.get("js3-results", null);
  if (!results || results.rev !== ctx.rev) results = { rev: ctx.rev, byCap: {} };
  const save = () => ctx.store.set("js3-results", results);
  const cap = () => Number($("js3-iter").value);
  const current = () => (results.byCap[cap()] ||= { engines: {} });
  let selected = new Set(ctx.store.get("js3-selected", []) || []);

  const status = (s) => ($("js3-status").textContent = s);
  const progress = (done, total) => {
    const p = $("js3-progress");
    p.hidden = !total;
    p.max = total || 1;
    p.value = done;
  };

  async function loadManifest() {
    if (manifest) return manifest;
    const r = await fetch("jetstream/manifest.json");
    if (!r.ok) throw new Error("this build has no JetStream material (jetstream/manifest.json)");
    manifest = await r.json();
    if (!ctx.store.get("js3-selected", null)) selected = new Set(manifest.benchmarks.map((b) => b.name));
    return manifest;
  }

  // ---- running ---------------------------------------------------------

  const fileCache = new Map();
  async function load(f, binary) {
    const key = (binary ? "b:" : "t:") + f;
    if (!fileCache.has(key)) {
      const r = await fetch("jetstream/" + f);
      if (!r.ok) throw new Error(`jetstream/${f}: ${r.status}`);
      fileCache.set(key, binary ? new Uint8Array(await r.arrayBuffer()) : await r.text());
    }
    return fileCache.get(key);
  }

  /** One benchmark (not a group) in one engine: its iteration times or an
   * error. */
  async function runOne(e, s, limit) {
    const runner = ctx.runnerFor(e);
    const deadline = performance.now() + limit;
    const left = () => Math.max(1, deadline - performance.now());
    const late = { error: `no result in ${Math.round(limit / 1000)} s` };
    let m = await runner.run(s.src, 1, left(), { fresh: true, settle: s.async });
    for (;;) {
      if (stop || m.cancelled) return { error: "stopped" };
      if (m.timeout) return late;
      const got = parse(m.out);
      if (got) return got;
      if (m.error || !m.ok) return { error: (m.out || []).filter((l) => l.trim()).slice(-1)[0] || "failed" };
      // an Async benchmark answers from promise jobs or timers: let the
      // engine's queue drain until it prints, as long as time is left
      if (!s.async) return { error: "the runner did not answer" };
      if (performance.now() > deadline) return late;
      m = await runner.run("", 1, left(), { settle: true });
    }
  }

  async function run() {
    await loadManifest();
    const engines = ctx.chosen();
    const limit = Number($("js3-limit").value);
    const c = cap();
    const list = manifest.benchmarks.filter((b) => selected.has(b.name));
    stop = false;
    $("js3-run").disabled = true;
    $("js3-stop").disabled = false;
    const res = current();
    for (const e of engines) {
      res.engines[e.id] ||= { name: e.name, b: {} };
      res.engines[e.id].name = e.name;
      ctx.setEngineState(e.id, "JetStream…");
    }
    let i = 0;
    for (const b of list) {
      if (stop) break;
      progress(i++, list.length);
      const members = b.subs || [b];
      // the scripts once per benchmark, for every engine
      const built = [];
      try {
        for (const m of members) built.push(await script(m, c, load));
      } catch (err) {
        for (const e of engines) res.engines[e.id].b[b.name] = { error: String(err.message || err) };
        save();
        render();
        continue;
      }
      for (const e of engines) {
        if (stop) break;
        const per = [];
        let error = null;
        running = { b: b.name, id: e.id };
        render();
        for (let k = 0; k < members.length; k++) {
          status(`${b.name}${members.length > 1 ? " · " + members[k].name : ""} · ${e.name} (${i}/${list.length})`);
          // a runner that throws (a worker that died, an engine that
          // aborted) fails this cell; the run goes on with the next one
          let r;
          try {
            r = await runOne(e, built[k], limit);
          } catch (err) {
            r = { error: String((err && err.message) || err) || "the engine stopped" };
          }
          if (r.error) {
            error = (members.length > 1 ? members[k].name + ": " : "") + r.error;
            break;
          }
          per.push({ name: members[k].name, times: r.times, digest: r.digest, subs: subScores(members[k], r.times, c) });
        }
        if (stop && !error) error = "stopped";
        if (error) res.engines[e.id].b[b.name] = { error: error.length > 300 ? error.slice(0, 300) + "…" : error };
        else {
          const subs = b.subs ? groupScores(per.map((p) => p.subs)) : per[0].subs;
          res.engines[e.id].b[b.name] = {
            score: scoreOf(subs),
            subs,
            members: b.subs ? Object.fromEntries(per.map((p) => [p.name, scoreOf(p.subs)])) : undefined,
            ms: per.reduce((a, p) => a + p.times.reduce((x, y) => x + y, 0), 0),
            digest: per.every((p) => p.digest) ? per.map((p) => p.digest).join(",") : undefined,
          };
        }
        running = null;
        save();
        render();
      }
      built.length = 0;
      fileCache.clear();
    }
    running = null;
    render();
    for (const e of engines) ctx.setEngineState(e.id, stop ? "stopped" : "done", stop ? "" : "ok");
    progress(0, 0);
    status(stop ? "stopped" : "done");
    $("js3-run").disabled = false;
    $("js3-stop").disabled = true;
  }

  // ---- table -----------------------------------------------------------

  function render() {
    if (!manifest) return;
    const res = current();
    const ids = Object.keys(res.engines).sort((a, b) => (a === "native" ? -1 : b === "native" ? 1 : 0));
    const nat = res.engines.native;
    const head =
      "<thead><tr><th>Benchmark</th>" +
      ids.map((id) => `<th>${esc(res.engines[id].name)}</th>`).join("") +
      "</tr></thead>";
    const cell = (id, b, r) => {
      if (running && running.b === b && running.id === id) return `<td class="pending">running…</td>`;
      if (!r) return "<td></td>";
      // the error in the cell itself (a tooltip alone is easy to miss)
      if (r.error) return `<td class="err" title="${esc(r.error)}">✗ <span class="errmsg">${esc(r.error.length > 80 ? r.error.slice(0, 80) + "…" : r.error)}</span></td>`;
      const tip = Object.entries(r.subs).map(([k, v]) => `${k} ${fmt(v)}`).join(", ") + ` · ${fmtMs(r.ms)} in all`;
      const n = nat && nat.b[b] && nat.b[b].score;
      const ratio = id !== "native" && n ? ` <span class="ratio">${fmt(n / r.score)}×</span>` : "";
      if (id !== "native" && sameAs(r, nat && nat.b[b], b) === "differs")
        return `<td class="err" title="${esc(tip + " · output differs from the browser: " + r.digest + " vs " + nat.b[b].digest)}">${fmt(r.score)}${ratio}<span class="errmsg">≠ browser output</span></td>`;
      return `<td title="${esc(tip)}">${fmt(r.score)}${ratio}</td>`;
    };
    // total: geometric mean over the benchmarks the engine finished; and
    // against the browser over those both finished
    const totals = ids.map((id) => {
      const e = res.engines[id];
      const done = manifest.benchmarks.filter((b) => e.b[b.name] && e.b[b.name].score);
      const score = geomean(done.map((b) => e.b[b.name].score));
      let ratio = null;
      if (id !== "native" && nat) {
        const both = done.filter((b) => nat.b[b.name] && nat.b[b.name].score);
        if (both.length) ratio = geomean(both.map((b) => nat.b[b.name].score / e.b[b.name].score));
      }
      const cmp = id === "native" || !nat ? [] : done.map((b) => sameAs(e.b[b.name], nat.b[b.name], b.name)).filter(Boolean);
      return { id, score, n: done.length, ratio, same: cmp.filter((x) => x === "same").length, compared: cmp.length };
    });
    const total =
      `<tr class="total"><td><strong>Score</strong> <span class="ratio">(geometric mean)</span></td>` +
      totals
        .map(
          (t) =>
            `<td><strong>${fmt(t.score)}</strong> <span class="ratio">${t.n}/${manifest.benchmarks.length}` +
            (t.ratio ? ` · ${fmt(t.ratio)}× slower` : "") +
            (t.compared ? ` · output = browser ${t.same}/${t.compared}` : "") +
            `</span></td>`,
        )
        .join("") +
      "</tr>";
    const rows = [];
    for (const b of manifest.benchmarks) {
      rows.push(
        `<tr><td title="${esc(b.tags.join(", "))}">${esc(b.name)}</td>` + ids.map((id) => cell(id, b.name, res.engines[id].b[b.name])).join("") + "</tr>",
      );
      if (b.subs)
        for (const s of b.subs)
          rows.push(
            `<tr class="member"><td>${esc(s.name)}</td>` +
              ids
                .map((id) => {
                  const r = res.engines[id].b[b.name];
                  const v = r && r.members && r.members[s.name];
                  return `<td>${fmt(v)}</td>`;
                })
                .join("") +
              "</tr>",
          );
    }
    $("js3-table").innerHTML = head + "<tbody>" + total + rows.join("") + "</tbody>";
    $("js3-gaps").textContent = gapReport();
  }

  // ---- slowest benchmarks, as text for development work ----------------

  /** The benchmarks where each of our engines (CEr*, CErXes*) is furthest
   * behind the browser and Hermes, its failures, totals, the build and the
   * browser: plain text to paste into a chat. */
  function gapReport() {
    const res = current();
    const n = Number($("js3-gap-n").value) || 10;
    const ids = Object.keys(res.engines);
    const ours = ids.filter((id) => /^cer/.test(id));
    const refs = ["native", "hermes"].filter((id) => res.engines[id]);
    const ok = (e, b) => e && e.b[b] && e.b[b].score;
    const L = [];
    const nav = typeof navigator !== "undefined" ? navigator : {};
    L.push(`JetStream 3 gaps · playground build ${ctx.rev || "local"} · JetStream ${manifest.rev.slice(0, 10)}`);
    L.push(`iterations: ${cap() || "JetStream's own"} · time limit ${Math.round(Number($("js3-limit").value) / 1000)} s`);
    L.push(`browser: ${nav.userAgent || "?"}`);
    L.push(
      `cores ${nav.hardwareConcurrency || "?"}` +
        (nav.deviceMemory ? ` · memory ${nav.deviceMemory} GB` : "") +
        ` · crossOriginIsolated ${typeof crossOriginIsolated !== "undefined" ? crossOriginIsolated : "?"}`,
    );
    L.push("");
    L.push("Totals (geometric mean over finished benchmarks):");
    for (const id of ids) {
      const e = res.engines[id];
      const done = manifest.benchmarks.filter((b) => ok(e, b.name));
      const failed = manifest.benchmarks.filter((b) => e.b[b.name] && e.b[b.name].error).length;
      L.push(`  ${e.name} [${id}]: ${fmt(geomean(done.map((b) => e.b[b.name].score)))} (${done.length} done, ${failed} failed)`);
    }
    if (!ours.length) {
      L.push("", "No CEr / CErXes results yet: tick one, run, then copy.");
      return L.join("\n");
    }
    for (const id of ours) {
      const e = res.engines[id];
      L.push("", `== ${e.name} [${id}] ==`);
      for (const r of refs) {
        const ref = res.engines[r];
        const both = manifest.benchmarks.filter((b) => ok(e, b.name) && ok(ref, b.name));
        if (!both.length) continue;
        const rows = both
          .map((b) => ({ b, x: ref.b[b.name].score / e.b[b.name].score }))
          .sort((a, b) => b.x - a.x);
        L.push(`vs ${ref.name}: ${fmt(geomean(rows.map((r) => r.x)))}× slower overall (${both.length} benchmarks); slowest ${Math.min(n, rows.length)}:`);
        rows.slice(0, n).forEach(({ b, x }, k) => {
          const me = e.b[b.name];
          const subs = Object.entries(me.subs).map(([s, v]) => `${s} ${fmt(v)}`).join(", ");
          L.push(
            `  ${String(k + 1).padStart(2)}. ${b.name}: ${fmt(x)}× slower · ours ${fmt(me.score)} vs ${fmt(ref.b[b.name].score)}` +
              ` · ${subs} · ${fmtMs(me.ms)}${b.tags.length ? " · " + b.tags.join(",") : ""}`,
          );
        });
      }
      const nat = res.engines.native;
      if (nat && id !== "native") {
        const cmp = manifest.benchmarks.map((b) => [b, sameAs(e.b[b.name], nat.b[b.name], b.name)]).filter(([, x]) => x);
        const diff = cmp.filter(([, x]) => x === "differs");
        L.push(`output vs browser: ${cmp.length - diff.length}/${cmp.length} same` + (diff.length ? `; differs: ${diff.map(([b]) => `${b.name} (${e.b[b.name].digest} vs ${nat.b[b.name].digest})`).join(", ")}` : ""));
      }
      const errs = manifest.benchmarks.filter((b) => e.b[b.name] && e.b[b.name].error);
      if (errs.length) {
        L.push(`failed (${errs.length}):`);
        for (const b of errs) {
          const others = refs.map((r) => `${r} ${ok(res.engines[r], b.name) ? "ok" : res.engines[r].b[b.name] ? "failed" : "not run"}`).join(", ");
          L.push(`  ${b.name}: ${e.b[b.name].error}${others ? " (" + others + ")" : ""}`);
        }
      }
      const notRun = manifest.benchmarks.filter((b) => selected.has(b.name) && !e.b[b.name]);
      if (notRun.length) L.push(`not run yet: ${notRun.map((b) => b.name).join(", ")}`);
    }
    return L.join("\n");
  }

  function renderLists() {
    const c = cap();
    $("js3-list").innerHTML = manifest.benchmarks
      .map((b) => {
        const p = plan(b.subs ? b.subs[0] : b, c);
        const what = b.subs ? `${b.subs.length} tests` : `${p.iterations} iteration${p.iterations === 1 ? "" : "s"}${p.worst ? `, worst ${p.worst}` : ""}`;
        return `<label title="${esc(b.tags.join(", "))}"><input type="checkbox" data-b="${esc(b.name)}"${selected.has(b.name) ? " checked" : ""}> ${esc(b.name)} <span class="ratio">${what}</span></label>`;
      })
      .join("");
    for (const box of $("js3-list").querySelectorAll("input")) {
      box.onchange = () => {
        if (box.checked) selected.add(box.dataset.b);
        else selected.delete(box.dataset.b);
        ctx.store.set("js3-selected", [...selected]);
        $("js3-count").textContent = `${selected.size}/${manifest.benchmarks.length}`;
      };
    }
    $("js3-count").textContent = `${selected.size}/${manifest.benchmarks.length}`;
    $("js3-skipped").innerHTML = manifest.skipped.map((s) => `<li><strong>${esc(s.name)}</strong>: ${esc(s.reason)}</li>`).join("");
    $("js3-source").innerHTML =
      `<a href="${esc(manifest.source)}" target="_blank" rel="noopener">WebKit/JetStream ${esc(manifest.rev.slice(0, 10))}</a> (${esc(manifest.date)}), ` +
      `${manifest.benchmarks.length} JavaScript benchmarks of its default set; ${manifest.skipped.length} left out (below).`;
  }

  $("js3-run").onclick = () => run().catch((err) => {
    running = null;
    render();
    status(String(err.message || err));
    $("js3-run").disabled = false;
    $("js3-stop").disabled = true;
  });
  $("js3-stop").onclick = () => {
    stop = true;
    for (const e of ctx.chosen()) ctx.runnerFor(e).cancel();
  };
  $("js3-clear").onclick = () => {
    results.byCap[cap()] = { engines: {} };
    save();
    render();
  };
  $("js3-gap-n").value = String(ctx.store.get("js3-gap-n", 10));
  $("js3-gap-n").onchange = () => {
    ctx.store.set("js3-gap-n", Number($("js3-gap-n").value));
    render();
  };
  $("js3-gap-copy").onclick = async () => {
    const text = manifest ? gapReport() : "";
    const say = (m) => {
      $("js3-gap-status").textContent = m;
      setTimeout(() => ($("js3-gap-status").textContent = ""), 3000);
    };
    try {
      await navigator.clipboard.writeText(text);
      say("copied");
    } catch {
      // no clipboard permission (e.g. not a secure context): select the text
      const r = document.createRange();
      r.selectNodeContents($("js3-gaps"));
      const sel = getSelection();
      sel.removeAllRanges();
      sel.addRange(r);
      say("selected: press Ctrl/⌘+C");
    }
  };
  $("js3-iter").value = String(ctx.store.get("js3-iter", 5));
  $("js3-iter").onchange = () => {
    ctx.store.set("js3-iter", cap());
    renderLists();
    render();
  };
  $("js3-all").onclick = () => {
    selected = new Set(manifest.benchmarks.map((b) => b.name));
    ctx.store.set("js3-selected", [...selected]);
    renderLists();
  };
  $("js3-none").onclick = () => {
    selected = new Set();
    ctx.store.set("js3-selected", []);
    renderLists();
  };

  loadManifest()
    .then(() => {
      renderLists();
      render();
    })
    .catch((err) => {
      status(String(err.message || err));
      $("js3-run").disabled = true;
    });
}

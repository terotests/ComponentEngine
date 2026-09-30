#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Serves playground/dist, opens it in Chromium (playwright-core), runs the
// benchmark at scale 1 once, and checks that every engine in the build
// loaded and answered what the browser answers (but for a workload listed
// for it in EXPECTED_GAPS).
//
//   node playground/smoke.mjs [--shot=out.png]
//
// --zoo=conf|speed|all runs the Zoo ranking tab instead: conformance, Octane
// or both on every engine, prints the engines' rows and fails when an engine
// got no result or the browser's own conformance is off (the harness is).
//
// --evg opens the CErXes + EVG demo (evg/) instead: each example game has to
// compile, paint frames and answer its input (Breakout: a key press and a
// click on its HUD button; Scaffold Scramble: start, walk, climb), and log
// no error.

import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const DIST = path.join(path.dirname(fileURLToPath(import.meta.url)), "dist");
const shot = (process.argv.find((a) => a.startsWith("--shot=")) || "").slice(7);
const EXPECTED_GAPS = {};
const zoo = (process.argv.find((a) => a.startsWith("--zoo=")) || "").slice(6);
const evgDemo = process.argv.includes("--evg");

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".wasm": "application/wasm" };
const server = http.createServer((req, res) => {
  const rel = decodeURIComponent(new URL(req.url, "http://x").pathname).replace(/^\/+/, "") || "index.html";
  let file = path.join(DIST, rel);
  if (fs.existsSync(file) && fs.statSync(file).isDirectory()) file = path.join(file, "index.html");
  if (!file.startsWith(DIST) || !fs.existsSync(file)) {
    res.writeHead(404);
    res.end();
    return;
  }
  res.writeHead(200, { "content-type": TYPES[path.extname(file)] || "application/octet-stream" });
  fs.createReadStream(file).pipe(res);
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const url = `http://127.0.0.1:${server.address().port}/`;

const { chromium } = await import("playwright-core");
const exe = process.env.CHROMIUM || ["/opt/pw-browsers/chromium", "/usr/bin/chromium"].find((p) => fs.existsSync(p));
const browser = await chromium.launch(exe && !fs.statSync(exe).isDirectory() ? { executablePath: exe } : {});
const page = await browser.newPage({ viewport: { width: 1280, height: 1400 } });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

if (evgDemo) await runEvg();
await page.goto(url);
await page.waitForSelector(".engine");
if (zoo) await runZoo();
await page.selectOption("#scale", process.env.SCALE || "1");
await page.selectOption("#reps", "1");
await page.click("#run-bench");
await page.waitForFunction(() => document.getElementById("bench-status").textContent === "done", null, { timeout: 15 * 60 * 1000 });

const table = await page.evaluate(() => {
  const t = document.getElementById("results");
  const heads = [...t.querySelectorAll("thead th")].map((th) => th.textContent);
  return [...t.querySelectorAll("tbody tr")].map((tr) => {
    const cells = [...tr.children].map((td) => ({ text: td.textContent, cls: td.className, title: td.title }));
    return { workload: cells[0].text, cells: cells.slice(1).map((c, i) => ({ engine: heads[i + 1], ...c })) };
  });
});
const manifest = JSON.parse(fs.readFileSync(path.join(DIST, "engines.json"), "utf8"));
const idOf = Object.fromEntries(manifest.engines.map((e) => [e.name, e.id]));

let bad = 0;
for (const row of table) {
  const line = [row.workload.padEnd(10)];
  for (const c of row.cells) {
    const failed = c.cls === "err" || c.cls === "wrong";
    const gap = (EXPECTED_GAPS[idOf[c.engine]] || []).includes(row.workload);
    if (failed && !gap) {
      bad++;
      console.log(`FAIL ${row.workload} · ${c.engine}: ${c.text} ${c.title}`);
    }
    line.push(c.text.padStart(18));
  }
  console.log(line.join(""));
}
if (shot) await page.screenshot({ path: shot, fullPage: true });
if (errors.length) {
  console.log("page errors:\n  " + errors.join("\n  "));
  bad += errors.length;
}
await browser.close();
server.close();
console.log(bad ? `${bad} problem(s)` : "ALL PASS");
process.exit(bad ? 1 : 0);

async function runZoo() {
  if (process.env.ENGINES) {
    // only these engines (ids, comma-separated); the browser's always runs
    const want = process.env.ENGINES.split(",");
    const m = JSON.parse(fs.readFileSync(path.join(DIST, "engines.json"), "utf8"));
    const boxes = await page.$$(".engine input");
    for (let i = 0; i < boxes.length; i++) if (!(await boxes[i].isDisabled())) await boxes[i].setChecked(want.includes(m.engines[i].id));
  }
  await page.click("#tab-zoo");
  await page.click("#zoo-clear");
  await page.setChecked("#zoo-conf", zoo !== "speed");
  await page.setChecked("#zoo-speed", zoo !== "conf");
  if (process.env.ZOO_LIMIT) await page.selectOption("#zoo-limit", process.env.ZOO_LIMIT);
  const t0 = Date.now();
  await page.click("#zoo-run");
  let last = "";
  while (true) {
    const st = await page.textContent("#zoo-status");
    if (st === "done" || st === "stopped") break;
    if (st !== last && !/conformance \d*[1-9]0\//.test(st)) console.log(`[${((Date.now() - t0) / 1000).toFixed(0)} s] ${st}`);
    last = st;
    await page.waitForTimeout(2000);
  }
  const rows = await page.evaluate(() =>
    [...document.querySelectorAll("#zoo-table tbody tr.ours")].map((tr) => [...tr.children].map((td) => td.textContent)),
  );
  const heads = await page.evaluate(() => [...document.querySelectorAll("#zoo-table thead th")].map((th) => th.textContent.replace(/[▾▴]/, "").trim()));
  console.log(heads.slice(1).map((h, i) => (i === 0 ? h.padEnd(40) : h.padStart(12))).join(""));
  for (const r of rows) console.log(r.slice(1).map((c, i) => (i === 0 ? c.padEnd(40) : c.padStart(12))).join(""));
  const details = await page.evaluate(() => [...document.querySelectorAll("#zoo-details summary")].map((s) => s.textContent));
  for (const d of details) console.log(d);
  if (process.env.ZOO_DUMP) {
    const fails = await page.evaluate(() => [...document.querySelectorAll("#zoo-details details")].map((d) => d.querySelector("summary").textContent.split(":")[0] + "\n  " + [...d.querySelectorAll(".failing li")].map((li) => li.textContent).join("\n  ")));
    fs.writeFileSync(process.env.ZOO_DUMP, fails.join("\n"));
  }
  let bad = 0;
  const col = (name) => heads.indexOf(name);
  for (const r of rows) {
    if (zoo !== "speed" && !r[col("ES6")]) (bad++, console.log("FAIL no conformance: " + r[1]));
    if (zoo !== "conf" && !r[col("Score")]) (bad++, console.log("FAIL no Octane score: " + r[1]));
  }
  const native = rows.find((r) => r[1].startsWith("Browser JS"));
  if (zoo !== "speed" && native && parseFloat(native[col("ES1–5")]) < 99) (bad++, console.log("FAIL the browser's ES1–5 is " + native[col("ES1–5")]));
  if (shot) await page.screenshot({ path: shot, fullPage: true });
  if (errors.length) (bad += errors.length, console.log("page errors:\n  " + errors.join("\n  ")));
  await browser.close();
  server.close();
  console.log(bad ? `${bad} problem(s)` : "ALL PASS");
  process.exit(bad ? 1 : 0);
}

async function runEvg() {
  await runScaffold();
  await page.goto(url + "evg/?game=breakout");
  const frames = () => page.evaluate(() => (window.__evgDemo ? window.__evgDemo.frames : 0));
  await page.waitForFunction(() => window.__evgDemo && window.__evgDemo.frames > 20, null, { timeout: 60000 });
  const before = await page.evaluate(() => document.querySelector("#stage svg").innerHTML.length);
  // launch the ball with the keyboard, then click the HUD button (EVG's hit test)
  await page.focus("#stage");
  await page.keyboard.press("Space");
  await page.waitForTimeout(600);
  const moved = await page.evaluate(() => document.getElementById("status").textContent);
  const box = await page.locator("#stage").boundingBox();
  await page.mouse.click(box.x + box.width * 0.93, box.y + box.height * 0.04);
  await page.waitForTimeout(300);
  const label = await page.evaluate(() => [...document.querySelectorAll("#stage svg text")].map((t) => t.textContent).join(" | "));
  const f = await frames();
  const info = await page.evaluate(() => ({ ...window.__evgDemo, stats: document.getElementById("stats").textContent }));
  if (shot) await page.screenshot({ path: shot.replace(/(\.png)?$/, "-evg.png"), fullPage: false });
  const consoleErrors = await page.evaluate(() => [...document.querySelectorAll("#console .err")].map((n) => n.textContent));
  console.log("evg demo:", f, "frames;", info.stats);
  console.log("status:", moved, "| svg text:", label, "| svg size before", before);
  const bad = [...errors, ...consoleErrors];
  if (bad.length) {
    console.log("errors:\n" + bad.join("\n"));
    process.exitCode = 1;
  }
  if (!/Pause|Resume|New game/.test(label)) {
    console.log("the HUD did not paint");
    process.exitCode = 1;
  }
  if (!/Resume/.test(label)) {
    console.log("the click on the HUD button did not reach its handler");
    process.exitCode = 1;
  }
  await browser.close();
  server.close();
  process.exit(process.exitCode || 0);
}

async function runScaffold() {
  await page.goto(url + "evg/?game=scaffold");
  await page.waitForFunction(() => window.__evgDemo && window.__evgDemo.frames > 20, null, { timeout: 60000 });
  await page.focus("#stage");
  await page.keyboard.press("Space");
  // walk right to the first ladder and climb it
  await page.keyboard.down("ArrowRight");
  await page.waitForTimeout(5200);
  await page.keyboard.up("ArrowRight");
  await page.keyboard.down("ArrowUp");
  await page.waitForTimeout(1500);
  await page.keyboard.up("ArrowUp");
  const text = await page.evaluate(() => [...document.querySelectorAll("#stage svg text")].map((t) => t.textContent).join(" | "));
  const info = await page.evaluate(() => ({ ...window.__evgDemo, stats: document.getElementById("stats").textContent }));
  if (shot) await page.screenshot({ path: shot.replace(/(\.png)?$/, "-scaffold.png"), fullPage: false });
  console.log("scaffold:", info.stats);
  console.log("scaffold text:", text);
  const consoleErrors = await page.evaluate(() => [...document.querySelectorAll("#console .err")].map((n) => n.textContent));
  if (consoleErrors.length || !/1UP/.test(text) || /SCAFFOLD SCRAMBLE/.test(text)) {
    console.log("scaffold failed:", consoleErrors.join("\n") || "the game did not start");
    process.exitCode = 1;
  }
}

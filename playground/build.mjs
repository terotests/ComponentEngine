#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Builds the playground into playground/dist:
//
//   wasm/cer.wasm      CEr (cer/) by cargo, --target wasm32-wasip1
//   workers/<id>.js    one worker per engine (esbuild): the browser's own
//                      engine, CEr, QuickJS, and CEr compiled by rgrc to
//                      JavaScript (cer/bin/Cer.cjs) as a curiosity
//   engines.json       which engines this build has, and why one is missing
//   evg/               the CErXes + EVG game demo: CErXes by cargo for
//                      wasm32-wasip1 (evg/cerxes.wasm), EvgGameHost.rgr (EVG's
//                      layout engine) by rgrc to JavaScript, EVG's SVG painter
//                      and text measurer, the page and its worker
//
// A step whose toolchain is missing is skipped and its engine marked
// unavailable, unless --strict (CI) makes that a failure.
//
//   node playground/build.mjs [--strict] [--skip=cer-wasm,cer-js]

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { REPO, rgrc } from "../scripts/compiler.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(HERE, "src");
const DIST = path.join(HERE, "dist");
const args = process.argv.slice(2);
const strict = args.includes("--strict");
const skip = new Set((args.find((a) => a.startsWith("--skip=")) || "--skip=").slice(7).split(",").filter(Boolean));

const ENGINES = ["native", "cer-wasm", "quickjs", "cer-js"];
const status = {}; // id -> { available, reason, size }

function log(s) {
  process.stdout.write(s + "\n");
}

function run(cmd, argv, opts = {}) {
  const r = spawnSync(cmd, argv, { encoding: "utf8", maxBuffer: 512 * 1024 * 1024, ...opts });
  const out = (r.stdout || "") + (r.stderr || "");
  if (r.error) throw new Error(`${cmd}: ${r.error.message}`);
  if (r.status !== 0 || /\[FAIL\]|Compilation FAILED/.test(out)) {
    throw new Error(`${cmd} ${argv.join(" ")}\n${out.slice(-4000)}`);
  }
  return out;
}

function step(id, fn) {
  if (skip.has(id)) {
    status[id] = { available: false, reason: "skipped in this build" };
    log(`-- ${id}: skipped`);
    return;
  }
  const t = Date.now();
  try {
    fn();
    log(`ok ${id} (${((Date.now() - t) / 1000).toFixed(1)} s)`);
  } catch (e) {
    const reason = String(e.message || e).split("\n")[0];
    status[id] = { available: false, reason };
    log(`!! ${id}: ${e.message || e}`);
    if (strict) process.exit(1);
  }
}

fs.rmSync(DIST, { recursive: true, force: true });
fs.mkdirSync(path.join(DIST, "wasm"), { recursive: true });
fs.mkdirSync(path.join(DIST, "workers"), { recursive: true });
fs.mkdirSync(path.join(DIST, "evg"), { recursive: true });

// ---- CEr as WebAssembly ----------------------------------------------------

step("cer-wasm", () => {
  const crate = path.join(HERE, "cer-wasm");
  run("cargo", ["build", "--release", "--quiet", "--target", "wasm32-wasip1", "--manifest-path", path.join(crate, "Cargo.toml")]);
  fs.copyFileSync(path.join(crate, "target/wasm32-wasip1/release/cer_wasm.wasm"), path.join(DIST, "wasm/cer.wasm"));
});

// ---- CEr as JavaScript, through Ranger -------------------------------------
// rgrc reads cer/src/lib.rs as a strict Rust module and writes JavaScript.
// Needs ranger-compiler 4.0.1 or later (the Rust-syntax fixes CEr uses).

step("cer-js", () => {
  fs.mkdirSync(path.join(REPO, "cer/bin"), { recursive: true });
  fs.rmSync(path.join(REPO, "cer/bin/Cer.cjs"), { force: true });
  run(process.execPath, ["--stack-size=8000", rgrc(), "-es6", "-nodemodule", "cer/src/lib.rs", "-d=cer/bin", "-o=Cer.cjs"], { cwd: REPO });
  if (!fs.existsSync(path.join(REPO, "cer/bin/Cer.cjs"))) throw new Error("rgrc wrote no cer/bin/Cer.cjs");
});

// ---- the CErXes + EVG demo: CErXes as WebAssembly, EVG as JavaScript --------

const EVG_SRC = path.join(SRC, "evg");
const EVG_DIST = path.join(DIST, "evg");

step("cerxes-wasm", () => {
  const crate = path.join(HERE, "cerxes-wasm");
  run("cargo", ["build", "--release", "--quiet", "--target", "wasm32-wasip1", "--manifest-path", path.join(crate, "Cargo.toml")]);
  fs.copyFileSync(path.join(crate, "target/wasm32-wasip1/release/cerxes_wasm.wasm"), path.join(EVG_DIST, "cerxes.wasm"));
});

step("evg-host", () => {
  // EVG (terotests/evg, storm/) comes through `rgrc install`, pinned in
  // playground/evg/ranger.json; the painter and the measurer are its files.
  const dir = path.join(HERE, "evg");
  const log = run(process.execPath, [rgrc(), "install"], { cwd: dir });
  const lock = JSON.parse(fs.readFileSync(path.join(dir, "ranger.lock"), "utf8"));
  const hit = /^evg\s+\S+\s+->\s+(\S+)/m.exec(log);
  const pkg = hit ? hit[1].trim() : path.join(os.homedir(), ".cache/ranger/packages", lock.packages.evg.sha256);
  if (!fs.existsSync(path.join(pkg, "EVGElement.rgr"))) throw new Error("EVG package not found at " + pkg);
  const stage = fs.mkdtempSync(path.join(os.tmpdir(), "evg-host-"));
  run(process.execPath, ["--stack-size=8000", rgrc(), "-es6", "EvgGameHost.rgr", "-d=" + stage, "-o=evg_game_host.js", "-nodecli"], { cwd: dir });
  const bundle = fs.readFileSync(path.join(stage, "evg_game_host.js"), "utf8").replace(/^#![^\n]*\n/, "");
  fs.rmSync(stage, { recursive: true, force: true });
  // Loaded here with `require` hidden, as the browser will load it.
  const previous = globalThis.require;
  globalThis.require = undefined;
  const found = (0, eval)("(function(){" + bundle + ";return typeof EvgGameHost + '|' + typeof EVGHostTextMeasurer})()");
  globalThis.require = previous;
  if (found !== "function|function") throw new Error("evg_game_host.js is missing its exports (" + found + ")");
  fs.writeFileSync(
    path.join(EVG_DIST, "evg_game_host.js"),
    "// GENERATED from playground/evg/EvgGameHost.rgr (EVG, MIT) by rgrc; do not edit.\n(function () {\n" +
      bundle +
      "\n;globalThis.EvgGameHost = EvgGameHost;" +
      "\n;globalThis.EvgGameModule = { EVGHostTextMeasurer: EVGHostTextMeasurer, EVGDefaultMeasurer: EVGDefaultMeasurer };" +
      "\n})();\n"
  );
  const vendor = path.join(EVG_SRC, "vendor");
  fs.mkdirSync(vendor, { recursive: true });
  fs.copyFileSync(path.join(pkg, "html/evg-html.js"), path.join(vendor, "evg-html.js"));
  fs.copyFileSync(path.join(pkg, "gl/evg-measure.js"), path.join(vendor, "evg-measure.js"));
});

// ---- the page and one worker per engine -----------------------------------

const esbuild = await import("esbuild");
const stub = path.join(SRC, "stubs/node.js");
fs.mkdirSync(path.dirname(stub), { recursive: true });
fs.writeFileSync(stub, "// The Node modules the JS builds mention; nothing calls them in a page.\nexport default {};\n");

for (const id of ENGINES) {
  if (status[id] && !status[id].available) continue;
  step("bundle " + id, () => {
    const r = esbuild.buildSync({
      stdin: {
        contents: `import engine from "./engines/${id}.js";\nimport { serve } from "./worker.js";\nserve(engine);\n`,
        resolveDir: SRC,
        sourcefile: `worker-${id}.js`,
      },
      bundle: true,
      format: "esm",
      platform: "browser",
      target: "es2022",
      minify: true,
      legalComments: "eof",
      alias: { fs: stub, path: stub, child_process: stub, os: stub, crypto: stub },
      outfile: path.join(DIST, "workers", id + ".js"),
      logLevel: "silent",
    });
    if (r.errors.length) throw new Error(r.errors.map((e) => e.text).join("\n"));
  });
  if (status["bundle " + id]) status[id] = status["bundle " + id];
}

esbuild.buildSync({
  entryPoints: [path.join(SRC, "main.js")],
  bundle: true,
  format: "esm",
  target: "es2022",
  minify: true,
  outfile: path.join(DIST, "main.js"),
  logLevel: "silent",
});
for (const f of ["index.html", "style.css"]) fs.copyFileSync(path.join(SRC, f), path.join(DIST, f));

// The EVG demo's page, its worker and its example game (read as text).
if (!status["cerxes-wasm"] && !status["evg-host"]) {
  step("bundle evg", () => {
    for (const [entry, out] of [["main.js", "main.js"], ["worker.js", "worker.js"]]) {
      const r = esbuild.buildSync({
        entryPoints: [path.join(EVG_SRC, entry)],
        bundle: true,
        format: "esm",
        platform: "browser",
        target: "es2022",
        minify: true,
        legalComments: "eof",
        loader: { ".tsx": "text", ".css": "text" },
        outfile: path.join(EVG_DIST, out),
        logLevel: "silent",
      });
      if (r.errors.length) throw new Error(r.errors.map((e) => e.text).join("\n"));
    }
    for (const f of ["index.html", "evg.css"]) fs.copyFileSync(path.join(EVG_SRC, f), path.join(EVG_DIST, f));
  });
} else {
  log("-- evg demo: not built (" + (status["cerxes-wasm"] || status["evg-host"]).reason + ")");
}

// The zoo ranking's material (zoo/update.mjs refreshes it): Octane suites,
// conformance tests, zoo.js.org's published results.
fs.cpSync(path.join(HERE, "zoo"), path.join(DIST, "zoo"), { recursive: true, filter: (f) => !f.endsWith(".mjs") });

// ---- the manifest ---------------------------------------------------------

// name and about, read from each adapter's source (importing QuickJS's
// browser build under Node is not worth it for two strings).
const about = {};
for (const id of ENGINES) {
  const text = fs.readFileSync(path.join(SRC, "engines", id + ".js"), "utf8");
  const field = (k) => (new RegExp(k + ':\\s*"([^"]*)"').exec(text) || [])[1] || "";
  about[id] = { name: field("name") || id, about: field("about") };
}
const sizeOf = (id) => {
  let n = 0;
  for (const f of [`workers/${id}.js`, id === "cer-wasm" ? "wasm/cer.wasm" : ""]) {
    if (f && fs.existsSync(path.join(DIST, f))) n += fs.statSync(path.join(DIST, f)).size;
  }
  return n;
};
const rev = spawnSync("git", ["rev-parse", "--short", "HEAD"], { cwd: REPO, encoding: "utf8" }).stdout.trim();
const manifest = {
  rev,
  built: new Date().toISOString(),
  engines: ENGINES.map((id) => ({
    id,
    ...(about[id] || { name: id, about: "" }),
    available: !status[id] || status[id].available !== false,
    reason: status[id] ? status[id].reason : undefined,
    size: sizeOf(id),
  })),
};
fs.writeFileSync(path.join(DIST, "engines.json"), JSON.stringify(manifest, null, 2) + "\n");
for (const e of manifest.engines) log(`${e.available ? "  " : "- "}${e.id.padEnd(15)} ${e.available ? (e.size / 1048576).toFixed(2) + " MB" : e.reason}`);

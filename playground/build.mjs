#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Builds the playground into playground/dist:
//
//   wasm/cer.wasm      CEr (cer/) by cargo, --target wasm32-wasip1
//   wasm/ce.wasm       ComponentEngine (engine/) by rgrc to C++, then clang
//                      from wasi-sdk (WASI_SDK_PATH, default /opt/wasi-sdk)
//   workers/<id>.js    one worker per engine (esbuild), with the JS builds of
//                      ComponentEngine (bin/engine_module.cjs) and CEr
//                      (cer/bin/Cer.cjs), QuickJS, Sval and JS-Interpreter
//   engines.json       which engines this build has, and why one is missing
//
// A step whose toolchain is missing is skipped and its engine marked
// unavailable, unless --strict (CI) makes that a failure.
//
//   node playground/build.mjs [--strict] [--skip=ce-wasm,cer-js,…]

import fs from "node:fs";
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

const ENGINES = ["native", "cer-wasm", "ce-wasm", "ce-js", "cer-js", "quickjs", "sval", "js-interpreter"];
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

const rgr = (argv, cwd = REPO) => run(process.execPath, ["--stack-size=8000", rgrc(), ...argv], { cwd });

fs.rmSync(DIST, { recursive: true, force: true });
fs.mkdirSync(path.join(DIST, "wasm"), { recursive: true });
fs.mkdirSync(path.join(DIST, "workers"), { recursive: true });

// ---- the WebAssembly builds -----------------------------------------------

step("cer-wasm", () => {
  const crate = path.join(HERE, "cer-wasm");
  run("cargo", ["build", "--release", "--quiet", "--target", "wasm32-wasip1", "--manifest-path", path.join(crate, "Cargo.toml")]);
  fs.copyFileSync(path.join(crate, "target/wasm32-wasip1/release/cer_wasm.wasm"), path.join(DIST, "wasm/cer.wasm"));
});

step("ce-wasm", () => {
  const sdk = process.env.WASI_SDK_PATH || "/opt/wasi-sdk";
  const clang = path.join(sdk, "bin/clang++");
  if (!fs.existsSync(clang)) throw new Error(`no wasi-sdk at ${sdk} (set WASI_SDK_PATH)`);
  const dir = path.join(HERE, "ce-wasm");
  const tmp = path.join(dir, "build");
  fs.mkdirSync(tmp, { recursive: true });
  rgr(["install"], dir);
  rgr(["-l=cpp", "ce_wasm.rgr", "-d=" + tmp, "-o=ce_wasm.cpp"], dir);
  run(process.execPath, [path.join(dir, "patch-cpp.mjs"), path.join(tmp, "ce_wasm.cpp"), path.join(dir, "glue.cpp"), path.join(tmp, "all.cpp")]);
  const sys = ["--target=wasm32-wasip1", "--sysroot=" + path.join(sdk, "share/wasi-sysroot")];
  run(clang, [...sys, "-std=c++17", "-O2", "-fno-exceptions", "-w", "-c", path.join(tmp, "all.cpp"), "-o", path.join(tmp, "all.o")]);
  run(clang, [...sys, "-mexec-model=reactor", path.join(tmp, "all.o"), "-o", path.join(DIST, "wasm/ce.wasm"), "-Wl,-z,stack-size=8388608", "-Wl,--strip-all"]);
});

// ---- the JavaScript builds of the two engines ------------------------------

step("ce-js", () => {
  if (!fs.existsSync(path.join(REPO, "bin/engine_module.cjs"))) run(process.execPath, [path.join(REPO, "scripts/build.mjs")]);
});

step("cer-js", () => {
  fs.mkdirSync(path.join(REPO, "cer/bin"), { recursive: true });
  rgr(["-es6", "-nodemodule", "cer/src/lib.rs", "-d=cer/bin", "-o=Cer.cjs"]);
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
      alias: { fs: stub, path: stub, child_process: stub, os: stub, crypto: stub, vm: stub },
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
  for (const f of [`workers/${id}.js`, id === "cer-wasm" ? "wasm/cer.wasm" : "", id === "ce-wasm" ? "wasm/ce.wasm" : ""]) {
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

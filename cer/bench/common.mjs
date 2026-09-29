// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Running one script through each engine the benchmarks compare. Every
// engine gets the same text and prints through `print`; a script reports
// its own timings, so no engine's parse or setup is counted.
import fs from "fs";
import os from "os";
import path from "path";
import vm from "vm";
import { spawnSync } from "child_process";
import { fileURLToPath } from "url";

export const CER = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// Two homes: terotests/componentengine (cer/ beside engine/) and a Ranger
// checkout, where npm run deps puts this directory at gallery/cer.
export const IN_RANGER = fs.existsSync(path.join(CER, "../../dist/rgrc.js")) && fs.existsSync(path.join(CER, "../../compiler"));
/** The componentengine checkout (only when not in Ranger). */
export const REPO = path.resolve(CER, "..");
/** A Ranger checkout, for what lives there: the Octane suites
 * (gallery/game_engine/v2/interp/bench/zoo_octane) and the conformance
 * probes (tests/runtime-conformance.test.ts). The one this directory is in,
 * else RANGER_DIR, else ../Ranger beside the componentengine checkout. */
export const RANGER = IN_RANGER
  ? path.resolve(CER, "../..")
  : path.resolve(process.env.RANGER_DIR || path.join(REPO, "../Ranger"));
/** ComponentEngine's es6 build, the `ce-js` engine. */
export const CE_MODULE = IN_RANGER
  ? path.join(RANGER, "gallery/game_engine/v2/interp/bin/engine_module.cjs")
  : path.join(REPO, "bin/engine_module.cjs");

/** Cargo's arguments before the command's own: in Ranger, the `ranger`
 * prelude crate is the checkout's runtime/rust/ranger, not the revision
 * Cargo.toml names, so a change there is tested with CEr at once. */
export function cargoConfig() {
  if (!IN_RANGER) return [];
  const crate = path.join(RANGER, "runtime/rust/ranger");
  return ["--config", `patch."https://github.com/terotests/Ranger.git".ranger.path=${JSON.stringify(crate)}`];
}

/** The compiler: RANGER_ROOT/dist/rgrc.js, the ranger-compiler package of
 * the componentengine checkout, else the Ranger checkout's. */
export function rgrc() {
  const cands = [
    process.env.RANGER_ROOT && path.join(process.env.RANGER_ROOT, "dist/rgrc.js"),
    !IN_RANGER && path.join(REPO, "node_modules/ranger-compiler/dist/rgrc.js"),
    path.join(RANGER, "dist/rgrc.js"),
  ].filter(Boolean);
  const hit = cands.find((f) => fs.existsSync(f));
  if (!hit) throw new Error("no Ranger compiler: npm ci, or set RANGER_ROOT / RANGER_DIR");
  return hit;
}

export const PRINT_PRELUDE = `
function print() {
  var s = "";
  for (var i = 0; i < arguments.length; i++) {
    if (i) s += " ";
    s += String(arguments[i]);
  }
  console.log(s);
}
`;

export function has(cmd) {
  const r = spawnSync("sh", ["-c", cmd + " >/dev/null 2>&1"]);
  return r.status === 0;
}

function tmpFile(text) {
  const f = path.join(os.tmpdir(), "cer-bench-" + process.pid + "-" + Math.floor(Math.random() * 1e9) + ".js");
  fs.writeFileSync(f, text);
  return f;
}

function lines(text) {
  return String(text || "").split(/\r?\n/).filter((l) => l.length);
}

/** Node itself, in a fresh context. */
export function runNode(src) {
  const out = [];
  const ctx = { console: { log: (...a) => out.push(a.map(String).join(" ")) }, performance };
  vm.createContext(ctx);
  vm.runInContext(PRINT_PRELUDE + src, ctx);
  return out;
}

/** ComponentEngine compiled to JavaScript (CE_MODULE),
 * in a process of its own with a time limit (CE_TIMEOUT_MS, default 300 s). */
export function runComponentEngine(src) {
  const f = tmpFile(PRINT_PRELUDE + src);
  const limit = Number(process.env.CE_TIMEOUT_MS || 300000);
  const r = spawnSync(process.execPath, ["--stack-size=8000", path.join(CER, "bench/ce_runner.cjs"), CE_MODULE, f], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: limit,
  });
  fs.unlinkSync(f);
  const out = lines(r.stdout);
  if (r.error) out.push("timeout: no result in " + limit / 1000 + " s");
  return out;
}

/** CEr built by cargo. */
export function runCerNative(src) {
  const bin = path.join(CER, "target/release/cer");
  const f = tmpFile(src);
  const r = spawnSync(bin, [f], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: 600000 });
  fs.unlinkSync(f);
  return lines(r.stdout).concat(lines(r.stderr));
}

/** CEr compiled to JavaScript by rgrc (bin/Cer.cjs), in a process of its
 * own with a time limit (CE_TIMEOUT_MS) and a 4 GB heap. */
export function runCerJs(src) {
  const f = tmpFile(src);
  const limit = Number(process.env.CE_TIMEOUT_MS || 300000);
  const r = spawnSync(process.execPath, ["--max-old-space-size=4096", path.join(CER, "bench/cer_js_runner.cjs"), f], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: limit,
  });
  fs.unlinkSync(f);
  const out = lines(r.stdout);
  if (r.error) out.push("timeout: no result in " + limit / 1000 + " s");
  else if (r.status !== 0) out.push("exited with " + (r.status ?? r.signal) + (/heap out of memory/.test(r.stderr) ? " (out of memory)" : ""));
  return out;
}

/** QuickJS (`qjs`, or the binary QJS names), for comparison. */
export function runQuickJs(src) {
  const f = tmpFile(PRINT_PRELUDE + src);
  const r = spawnSync(process.env.QJS || "qjs", [f], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: 600000 });
  fs.unlinkSync(f);
  const out = lines(r.stdout).concat(lines(r.stderr));
  if (r.error) out.push(String(r.error.message));
  return out;
}

/** CEr compiled by rgrc to C++ or Go (bin/cer_main_<target>). */
export function runCerBinary(target, src) {
  const bin = path.join(CER, "bin", "cer_main_" + target);
  const f = tmpFile(src);
  const r = spawnSync(bin, [path.dirname(f), path.basename(f)], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: 600000 });
  fs.unlinkSync(f);
  return lines(r.stdout).concat(lines(r.stderr));
}

export function engines(want) {
  const all = {
    node: runNode,
    "ce-js": runComponentEngine,
    "cer-rust": runCerNative,
    "cer-js": runCerJs,
    "cer-cpp": (s) => runCerBinary("cpp", s),
    "cer-go": (s) => runCerBinary("go", s),
    qjs: runQuickJs,
  };
  const out = {};
  for (const k of want) {
    if (all[k]) out[k] = all[k];
  }
  return out;
}

/** Builds what the chosen engines need. */
export function build(want) {
  const run = (cmd, args, cwd) => {
    const r = spawnSync(cmd, args, { cwd: cwd || (IN_RANGER ? RANGER : REPO), encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
    const log = (r.stdout || "") + (r.stderr || "");
    if (r.status !== 0 || log.includes("[FAIL]")) {
      throw new Error(cmd + " " + args.join(" ") + "\n" + log.slice(-3000));
    }
  };
  if (want.includes("cer-rust")) {
    run("cargo", ["build", ...cargoConfig(), "--release", "--quiet", "--manifest-path", path.join(CER, "Cargo.toml")]);
  }
  if (want.includes("cer-js")) {
    run("node", ["--stack-size=8000", rgrc(), "-es6", "-nodemodule", path.join(CER, "src/lib.rs"), "-d=" + path.join(CER, "bin"), "-o=Cer.cjs"]);
  }
  if (want.includes("ce-js") && !fs.existsSync(CE_MODULE)) {
    if (IN_RANGER) run("bash", ["scripts/build-engine-module.sh"]);
    else run("node", ["scripts/build.mjs"]);
  }
  for (const t of ["cpp", "go"]) {
    if (!want.includes("cer-" + t)) continue;
    const dir = path.join(os.tmpdir(), "cer-" + t);
    fs.mkdirSync(dir, { recursive: true });
    const srcName = "cer_main." + t;
    run("node", ["--stack-size=8000", rgrc(), "-l=" + t, path.join(CER, "bench/CerMain.rgr"), "-d=" + dir, "-o=" + srcName]);
    const bin = path.join(CER, "bin", "cer_main_" + t);
    if (t === "cpp") {
      run("g++", ["-std=c++17", "-O2", "-o", bin, path.join(dir, srcName)]);
    } else {
      run("go", ["build", "-o", bin, srcName], dir);
    }
  }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//
// PDFKit document generation as a benchmark: CErXes (native, and as
// WebAssembly with the baseline JIT off and on) against Hermes and Node.
//
//   node cerxes/bench/pdfkit.mjs [--pages=10,100] [--runs=3] [--engines=...]
//
// The script is PDFKit's standalone build (pdfkit@PDFKIT_VERSION from npm,
// lowered to ES2017 by esbuild because Hermes has no async generators;
// every engine runs the same file), pdfkit/shim.js before it and
// pdfkit/main.js after it; they are written to cerxes/bench/pdfkit/out.
// Engines (--engines, default all that are found):
//   node         the Node running this
//   cerxes       cerxes/target/release/cerxes, the register machine (CER_REGS=1)
//   wasm-off     playground/cerxes-wasm built with wjit, the JIT off (wjit.mjs)
//   wasm-on      the same with the JIT on (jit_min 64)
//   hermes       $HERMES (a `hermes` binary), run as `hermes -O -w`
// Every engine has to print the same size and hash. CERXES_PROF=1 on the
// cerxes engine prints its profile (src/bin/cerxes.rs).
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const PDFKIT_VERSION = "0.20.2";
const ESBUILD_VERSION = "0.25.12";
const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "../..");
const DIR = path.join(HERE, "pdfkit");
const OUT = path.join(DIR, "out");
const args = process.argv.slice(2);
const opt = (name, def) => args.find((a) => a.startsWith("--" + name + "="))?.slice(name.length + 3) ?? def;
const pages = opt("pages", "10,100").split(",").map(Number);
const runs = Number(opt("runs", "3"));

/** PDFKit's standalone build, lowered and with the one line that needs a
 * browser or Node at load time (the sRGB profile's URL) made a constant. */
function library() {
  const lib = path.join(OUT, `pdfkit-${PDFKIT_VERSION}.es2017.js`);
  if (fs.existsSync(lib)) return fs.readFileSync(lib, "utf8");
  fs.mkdirSync(OUT, { recursive: true });
  const pkg = path.join(OUT, "pkg");
  fs.mkdirSync(pkg, { recursive: true });
  if (!fs.existsSync(path.join(pkg, "package.json"))) fs.writeFileSync(path.join(pkg, "package.json"), '{"private":true}\n');
  console.log(`==> npm install pdfkit@${PDFKIT_VERSION} esbuild@${ESBUILD_VERSION}`);
  const r = spawnSync("npm", ["install", "--no-save", "--no-audit", "--no-fund", `pdfkit@${PDFKIT_VERSION}`, `esbuild@${ESBUILD_VERSION}`], { cwd: pkg, stdio: "inherit" });
  if (r.status !== 0) process.exit(1);
  let src = fs.readFileSync(path.join(pkg, "node_modules/pdfkit/js/pdfkit.standalone.js"), "utf8");
  const icc = /^const ICC_PROFILE_PATH = new URL\(.*$/m;
  if (!icc.test(src)) throw new Error("pdfkit.standalone.js: the ICC_PROFILE_PATH line moved");
  src = src.replace(icc, "const ICC_PROFILE_PATH = './data/sRGB_IEC61966_2_1.icc';");
  const raw = path.join(OUT, "pdfkit.standalone.js");
  fs.writeFileSync(raw, src);
  const e = spawnSync(path.join(pkg, "node_modules/.bin/esbuild"), [raw, "--target=es2017", "--log-level=error", "--outfile=" + lib], { stdio: "inherit" });
  if (e.status !== 0) process.exit(1);
  return fs.readFileSync(lib, "utf8");
}

/** The benchmark script for `n` pages. */
function script(n) {
  const file = path.join(OUT, `pdfkit-${n}.js`);
  const text =
    `var PDF_PAGES = ${n}, PDF_RUNS = ${runs};\n` +
    fs.readFileSync(path.join(DIR, "shim.js"), "utf8") +
    "var PDFDocument = (function () { var module = { exports: {} }, exports = module.exports;\n" +
    library() +
    "\n; return module.exports; })();\n" +
    fs.readFileSync(path.join(DIR, "main.js"), "utf8");
  fs.writeFileSync(file, text);
  return file;
}

const CERXES = path.join(REPO, "cerxes/target/release/cerxes");
const ENGINES = {
  node: { found: () => true, cmd: (f) => [process.execPath, [f]] },
  cerxes: { found: () => fs.existsSync(CERXES), cmd: (f) => [CERXES, [f], { CER_REGS: "1" }] },
  "wasm-off": { found: () => true, cmd: (f) => [process.execPath, [path.join(HERE, "wjit.mjs"), "--eval=" + f, "--only-off=1"]] },
  "wasm-on": { found: () => true, cmd: (f) => [process.execPath, [path.join(HERE, "wjit.mjs"), "--eval=" + f, "--only-on=1", "--min=64"]] },
  hermes: { found: () => !!process.env.HERMES && fs.existsSync(process.env.HERMES), cmd: (f) => [process.env.HERMES, ["-O", "-w", f]] },
};
const want = opt("engines", Object.keys(ENGINES).join(",")).split(",");
const table = [];
for (const n of pages) {
  const f = script(n);
  for (const id of want) {
    const e = ENGINES[id];
    if (!e || !e.found()) {
      console.log(`${id}: not found, skipped`);
      continue;
    }
    const [bin, argv, env] = e.cmd(f);
    const r = spawnSync(bin, argv, { env: { ...process.env, ...(env || {}) }, encoding: "utf8", maxBuffer: 1 << 26 });
    const out = (r.stdout || "") + (r.stderr || "");
    const m = /pdfkit (\d+) pages: (\d+) bytes, hash (-?\d+), best ([\d.]+) ms/.exec(out);
    if (!m) {
      console.log(`${id} ${n} pages: no result\n${out.split("\n").slice(-8).join("\n")}`);
      continue;
    }
    table.push({ engine: id, pages: n, ms: Number(m[4]), bytes: Number(m[2]), hash: m[3] });
    console.log(`${id.padEnd(9)} ${String(n).padStart(4)} pages  ${m[4].padStart(8)} ms  ${m[2]} bytes  hash ${m[3]}`);
    if (process.env.CERXES_PROF && id === "cerxes") console.log(out.slice(out.indexOf("profile:")));
  }
}
for (const n of pages) {
  const rows = table.filter((t) => t.pages === n);
  if (new Set(rows.map((t) => t.hash + "/" + t.bytes)).size > 1) {
    console.log(`FAIL ${n} pages: the engines made different documents`);
    process.exitCode = 1;
  }
}

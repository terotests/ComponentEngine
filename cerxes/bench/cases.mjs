// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The cases of tests/cases.txt, and CEr's (../cer/tests/cases.txt) as
// JavaScript and as TSX, on CErXes built by the Ranger compiler instead of
// cargo: the same answers on every target.
//
//   node cerxes/bench/cases.mjs                     # JavaScript (bin/Cerxes.cjs)
//   node cerxes/bench/cases.mjs --targets=js,cpp,go # also C++ and Go
//   node cerxes/bench/cases.mjs --targets=cer-js    # CEr's cases on CEr's JS build
//   node cerxes/bench/cases.mjs --build             # rebuild first
//
// js is `rgrc -es6 -nodemodule src/lib.rs`; cpp and go are
// bench/CerxesMain.rgr through `rgrc -l=cpp|go`, then g++ / go build.
import fs from "fs";
import os from "os";
import path from "path";
import { spawnSync } from "child_process";
import { fileURLToPath } from "url";
import { createRequire } from "module";
import { compile } from "../../scripts/compiler.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CRATE = path.resolve(HERE, "..");
const BIN = path.join(CRATE, "bin");
const args = process.argv.slice(2);
const rebuild = args.includes("--build");
const targets = (args.find((a) => a.startsWith("--targets="))?.slice(10) || "js").split(",");

function run(cmd, argv, cwd) {
  const r = spawnSync(cmd, argv, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(cmd + " " + argv.join(" ") + "\n" + (r.stdout + r.stderr).slice(-3000));
}

fs.mkdirSync(BIN, { recursive: true });
const engines = {};
for (const t of targets) {
  if (t === "js") {
    const out = path.join(BIN, "Cerxes.cjs");
    if (rebuild || !fs.existsSync(out)) compile(["-es6", "-nodemodule", "cerxes/src/lib.rs", "-d=cerxes/bin", "-o=Cerxes.cjs"]);
    const { Engine } = createRequire(import.meta.url)(out);
    engines.js = (mode, src) => {
      const e = Engine.new_();
      e.set_syntax(mode.includes("ts"), mode.includes("x"));
      try {
        return e.eval(src);
      } catch (ex) {
        return "host exception: " + ex;
      }
    };
  } else if (t === "cer-js") {
    // CEr itself built by rgrc (cer/bin/Cer.cjs): CEr's cases only
    const out = path.join(CRATE, "../cer/bin/Cer.cjs");
    if (rebuild || !fs.existsSync(out)) compile(["-es6", "-nodemodule", "cer/src/lib.rs", "-d=cer/bin", "-o=Cer.cjs"]);
    const { Engine } = createRequire(import.meta.url)(out);
    engines["cer-js"] = (mode, src) => {
      if (mode !== "js") return null;
      try {
        return Engine.new_().eval(src);
      } catch (ex) {
        return "host exception: " + ex;
      }
    };
  } else if (t === "cpp" || t === "go") {
    const bin = path.join(BIN, "cerxes_main_" + t);
    if (rebuild || !fs.existsSync(bin)) {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "cerxes-" + t + "-"));
      const src = "cerxes_main." + t;
      compile(["-l=" + t, "cerxes/bench/CerxesMain.rgr", "-d=" + dir, "-o=" + src]);
      if (t === "cpp") run("g++", ["-std=c++17", "-O2", "-o", bin, path.join(dir, src)]);
      else run("go", ["build", "-o", bin, src], dir);
    }
    engines[t] = (mode, src) => {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "cerxes-case-"));
      fs.writeFileSync(path.join(dir, "case.txt"), src);
      const r = spawnSync(bin, [dir, "case.txt", mode], { encoding: "utf8", timeout: 60000 });
      fs.rmSync(dir, { recursive: true });
      return r.status === 0 ? r.stdout.replace(/\n$/, "") : "exited with " + (r.status ?? r.signal) + ": " + r.stderr.slice(-300);
    };
  } else {
    throw new Error("unknown target " + t);
  }
}

/** The cases of a file: `==== <syntax> [title]`, script, `----`, value. */
function readCases(file) {
  const out = [];
  let cur = null;
  for (const line of fs.readFileSync(file, "utf8").split("\n")) {
    if (line.startsWith("==== ")) {
      if (cur) out.push(cur);
      const head = line.slice(5).trim();
      const sp = head.indexOf(" ");
      cur = { mode: sp < 0 ? head : head.slice(0, sp), title: sp < 0 ? "" : head.slice(sp + 1), src: [], want: [], inWant: false };
    } else if (cur) {
      if (line === "----" && !cur.inWant) cur.inWant = true;
      else if (cur.inWant) {
        if (!line.startsWith("#")) cur.want.push(line);
      } else cur.src.push(line);
    }
  }
  if (cur) out.push(cur);
  return out;
}

const own = readCases(path.join(CRATE, "tests/cases.txt"));
const cerCases = readCases(path.join(CRATE, "../cer/tests/cases.txt"));
// CEr's cases as they are (JavaScript) and read as TSX: plain JavaScript
// means the same either way
const suites = [
  ["cerxes cases", own],
  ["CEr cases as js", cerCases],
  ["CEr cases as tsx", cerCases.map((c) => ({ ...c, mode: "tsx" }))],
];

let bad = 0;
for (const [t, evalOn] of Object.entries(engines)) {
  for (const [name, cases] of suites) {
    if (t === "cer-js" && name !== "CEr cases as js") continue;
    let failed = 0;
    for (const c of cases) {
      const want = c.want.join("\n").trimEnd();
      const src = c.src.join("\n");
      const got = evalOn(c.mode, src);
      if (got !== want) {
        failed++;
        console.log(`${t} [${c.mode} ${c.title}] ${src}\n  want: ${want}\n  got:  ${got}`);
      }
    }
    console.log(`${t}, ${name}: ${cases.length - failed} of ${cases.length} pass`);
    bad += failed;
  }
}
process.exit(bad ? 1 : 0);

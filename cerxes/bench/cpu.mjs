// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The CPU workloads of bench/cpu/*.js (MFilesAPI's UIX stress-cpu sample at
// its largest sizes, and string building, templates and property access) on
// Node, QuickJS, CErXes built by cargo and CErXes built by rgrc for Go (the
// target the M-Files UIX host runs). Each script times itself and prints
// "<name>\t<ms>"; a run past --timeout seconds is reported as such.
//
//   node cerxes/bench/cpu.mjs                          # node,rust,go
//   node cerxes/bench/cpu.mjs --engines=rust,go --build --only=regex,json
//   node cerxes/bench/cpu.mjs --runs=3                 # the best of three
//
// qjs is QuickJS from PATH or the binary QJS names.
import fs from "fs";
import os from "os";
import path from "path";
import { spawnSync } from "child_process";
import { fileURLToPath } from "url";
import { compile } from "../../scripts/compiler.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CRATE = path.resolve(HERE, "..");
const BIN = path.join(CRATE, "bin");
const args = process.argv.slice(2);
const opt = (name, def) => args.find((a) => a.startsWith("--" + name + "="))?.slice(name.length + 3) ?? def;
const rebuild = args.includes("--build");
const engines = opt("engines", "node,rust,go").split(",");
const only = opt("only", "");
const timeout = Number(opt("timeout", "300")) * 1000;
const runs = Number(opt("runs", "1"));

function run(cmd, argv, cwd) {
  const r = spawnSync(cmd, argv, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (r.status !== 0) throw new Error(cmd + " " + argv.join(" ") + "\n" + (r.stdout + r.stderr).slice(-3000));
}

const cmds = {};
for (const e of engines) {
  if (e === "node") cmds.node = (f) => [process.execPath, [f]];
  else if (e === "qjs") cmds.qjs = (f) => [process.env.QJS || "qjs", [f]];
  else if (e === "rust") {
    const bin = path.join(CRATE, "target/release/cerxes");
    if (rebuild || !fs.existsSync(bin)) run("cargo", ["build", "--release", "--bin", "cerxes"], CRATE);
    cmds.rust = (f) => [bin, [f]];
  } else if (e === "go") {
    const bin = path.join(BIN, "cerxes_main_go");
    if (rebuild || !fs.existsSync(bin)) {
      fs.mkdirSync(BIN, { recursive: true });
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "cerxes-go-"));
      compile(["-l=go", "cerxes/bench/CerxesMain.rgr", "-d=" + dir, "-o=cerxes_main.go"]);
      run("go", ["build", "-o", bin, "cerxes_main.go"], dir);
    }
    cmds.go = (f) => [bin, [path.dirname(f), path.basename(f)]];
  } else throw new Error("unknown engine " + e);
}

const files = fs
  .readdirSync(path.join(HERE, "cpu"))
  .filter((f) => f.endsWith(".js") && (!only || only.split(",").includes(f.slice(0, -3))))
  .sort();
const names = Object.keys(cmds);
console.log(["workload", ...names].join("\t"));
for (const f of files) {
  const row = [f.slice(0, -3)];
  for (const n of names) {
    const [cmd, argv] = cmds[n](path.join(HERE, "cpu", f));
    let best = null;
    for (let k = 0; k < runs; k++) {
      const r = spawnSync(cmd, argv, { encoding: "utf8", timeout, maxBuffer: 64 * 1024 * 1024 });
      const m = /\t(\d+)\s*$/.exec(r.stdout || "");
      if (!(m && r.status === 0)) {
        best = r.signal === "SIGTERM" ? ">" + timeout / 1000 + "s" : "error";
        break;
      }
      best = best === null ? Number(m[1]) : Math.min(best, Number(m[1]));
    }
    row.push(String(best));
  }
  console.log(row.join("\t"));
}

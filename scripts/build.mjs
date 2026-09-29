#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
//
// npm run build: fetch the dependencies (rgrc install in test/, which names
// engine/ by path) and compile the Node module bin/engine_module.cjs.
//   node scripts/build.mjs [--cer]    also CEr to JavaScript (cer/bin/Cer.cjs)
//   node scripts/build.mjs --cerxes   only CErXes to JavaScript (cerxes/bin/Cerxes.cjs)
import fs from "node:fs";
import path from "node:path";
import { REPO, compile, rgrc } from "./compiler.mjs";

const t0 = Date.now();
console.log("compiler " + rgrc());
if (process.argv.includes("--cerxes")) {
  compile(["-es6", "-nodemodule", "cerxes/src/lib.rs", "-d=cerxes/bin", "-o=Cerxes.cjs"]);
  console.log("wrote cerxes/bin/Cerxes.cjs");
  console.log(((Date.now() - t0) / 1000).toFixed(1) + " s");
  process.exit(0);
}
compile(["install"], path.join(REPO, "test"));
fs.mkdirSync(path.join(REPO, "bin"), { recursive: true });
compile(["-es6", "-nodemodule", "test/engine_module.rgr", "-d=bin", "-o=engine_module.cjs"]);
console.log("wrote bin/engine_module.cjs");
if (process.argv.includes("--cer")) {
  compile(["-es6", "-nodemodule", "cer/src/lib.rs", "-d=cer/bin", "-o=Cer.cjs"]);
  console.log("wrote cer/bin/Cer.cjs");
}
console.log(((Date.now() - t0) / 1000).toFixed(1) + " s");

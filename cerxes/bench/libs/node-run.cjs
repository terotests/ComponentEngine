// SPDX-License-Identifier: AGPL-3.0-or-later
// V8 runner: the bundle as a script in a fresh realm (no Node globals), as
// the CErXes CLI runs it; only console and performance come from the host.
const vm = require("vm");
const fs = require("fs");
const ctx = vm.createContext({
  console: { log: (...a) => console.log(...a), error: (...a) => console.error(...a), warn: (...a) => console.error(...a), info: (...a) => console.log(...a), debug() {} },
  performance: { now: () => performance.now() },
});
vm.runInContext(fs.readFileSync(process.argv[2], "utf8"), ctx, { filename: process.argv[2] });

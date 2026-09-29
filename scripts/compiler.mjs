// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Where the Ranger compiler is: RANGER_ROOT/dist/rgrc.js, the
// ranger-compiler package in node_modules (`npm ci`), else a Ranger checkout
// at RANGER_DIR or ../Ranger.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const RANGER = path.resolve(process.env.RANGER_DIR || path.join(REPO, "../Ranger"));

export function rgrc() {
  const cands = [
    process.env.RANGER_ROOT && path.join(process.env.RANGER_ROOT, "dist/rgrc.js"),
    path.join(REPO, "node_modules/ranger-compiler/dist/rgrc.js"),
    path.join(RANGER, "dist/rgrc.js"),
  ].filter(Boolean);
  const hit = cands.find((f) => fs.existsSync(f));
  if (!hit) throw new Error("no Ranger compiler: npm ci, or set RANGER_ROOT / RANGER_DIR");
  return hit;
}

/** Runs the compiler; fails on a non-zero exit or a `[FAIL]` in its log,
 * which it does not put in its exit status. */
export function compile(args, cwd = REPO) {
  const r = spawnSync(process.execPath, ["--stack-size=8000", rgrc(), ...args], {
    cwd,
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  });
  const log = (r.stdout || "") + (r.stderr || "");
  if (r.status !== 0 || /\[FAIL\]|Compilation FAILED/.test(log)) {
    process.stderr.write(log.slice(-6000));
    throw new Error("rgrc " + args.join(" ") + " failed");
  }
  return log;
}

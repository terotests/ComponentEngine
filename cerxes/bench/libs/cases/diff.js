import { diffLines, diffWords, diffChars, createPatch, applyPatch } from "diff";
const a = Array.from({ length: 400 }, (_x, i) => `line ${i} ${"word ".repeat(i % 5)}`).join("\n");
const b = a.split("\n").map((l, i) => (i % 17 === 0 ? l + " changed" : i % 23 === 0 ? null : l)).filter((l) => l !== null).join("\n") + "\nadded";
const s = (parts) => parts.map((p) => (p.added ? "+" : p.removed ? "-" : "=") + p.count).join(" ");
export default {
  check: {
    lines: () => s(diffLines(a, b)),
    words: () => diffWords("the quick brown fox jumps", "the quick red fox jumped over").map((p) => (p.added ? "+" : p.removed ? "-" : "=") + p.value),
    chars: () => s(diffChars("kitten sitting", "sitting kitten")),
    patch: () => { const p = createPatch("f", a, b, "", ""); return [p.length, applyPatch(a, p) === b]; },
  },
  bench: { lines: () => diffLines(a, b).length, words: () => diffWords(a.slice(0, 3000), b.slice(0, 3000)).length, patch: () => createPatch("f", a, b).length },
};

import * as esprima from "esprima";
const src = `
var a = function (x, y) { return x + y; }; const { p, q: [r, ...s] } = obj; let t = \`a\${b}c\`;
class K extends L { constructor() { super(); this.v = 1; } static get g() { return 0; } *gen() { yield* other(); } }
for (const [k, v] of Object.entries(o)) { if (k in o && v instanceof Map) continue; }
label: while (true) { try { throw new Error("x"); } catch (e) { break label; } finally { void 0; } }
const re = /ab+c/gi, n = 0x1f + 0b101 + 0o17 + 1e-3; async function f() { await g(); } x => ({ x });
`;
const big = Array.from({ length: 40 }, (_, i) => "function w" + i + "() {" + src + "}").join("\n");
export default {
  check: {
    ast: () => JSON.stringify(esprima.parseScript(src, { range: true }), (k, v) => (v instanceof RegExp ? String(v) : v)),
    tokens: () => esprima.tokenize("let x = a / b / c; /re/g.test(x) // c", { comment: true }).map((t) => t.type + ":" + t.value),
    errors: () => ["var 1a", "let = ;", "for (;;"].map((s) => { try { esprima.parseScript(s); return "ok"; } catch (e) { return e.description + "@" + e.index; } }),
  },
  bench: {
    parse: () => esprima.parseScript(big, { loc: true }).body.length,
  },
};

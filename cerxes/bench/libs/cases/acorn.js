import * as acorn from "acorn";
const src = `
class A extends B { #p = 1; static s = 2; get x() { return this.#p ?? 0; } async *gen() { for await (const v of xs) yield v?.y; } }
const { a, b: [c, ...d] = [], ...rest } = obj;
label: for (let i = 0; i < 10; i++) { if (i % 2) continue label; else break; }
const re = /(?<year>\\d{4})-(?<m>\\d\\d)/gu, t = tag\`x\${1 + 2}y\`;
export default async function f(x = 1, ...ys) { try { await import("m"); } catch { } finally { } return x ** 2 >>> 1; }
a ||= b; c &&= d; e ??= f; const n = 1_000_000n;
`;
const big = Array.from({ length: 60 }, (_, i) => "async function w" + i + "() {" + src.replace("export default ", "") + "}").join("\n");
function count(node, m = {}) { if (node && typeof node.type === "string") m[node.type] = (m[node.type] || 0) + 1; for (const k in node) { const v = node[k]; if (v && typeof v === "object") { if (Array.isArray(v)) v.forEach((x) => count(x, m)); else if (k !== "loc") count(v, m); } } return m; }
export default {
  check: {
    ast: () => JSON.stringify(acorn.parse(src, { ecmaVersion: "latest", sourceType: "module" }), (k, v) => (typeof v === "bigint" ? String(v) : v instanceof RegExp ? String(v) : v)),
    counts: () => count(acorn.parse(big, { ecmaVersion: "latest", sourceType: "module" })),
    errors: () => ["let let = 1", "a => { yield 1 }", "class { }", "x = 08n", "({a:1} = 1)"].map((s) => { try { acorn.parse(s, { ecmaVersion: 2022 }); return "ok"; } catch (e) { return e.message; } }),
    tokens: () => [...acorn.tokenizer("let x = a / b / c; /re/g.test(x)", { ecmaVersion: 2022 })].map((t) => t.type.label + ":" + t.value),
  },
  bench: {
    parseBig: () => acorn.parse(big, { ecmaVersion: "latest", sourceType: "module" }).body.length,
    parseLocations: () => acorn.parse(big, { ecmaVersion: "latest", sourceType: "module", locations: true }).end,
  },
};

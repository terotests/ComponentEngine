import { parse } from "@babel/parser";
const src = `
import React, { useState } from "react";
interface P<T extends object = {}> { a?: T; readonly b: string[]; }
type U = keyof P | \`x-\${string}\`;
enum E { A = 1, B = A << 2 }
export function C<T,>({ a, b }: P<T>): JSX.Element | null {
  const [s, setS] = useState<number>(0);
  @dec class K { @prop() accessor x = 1; constructor(private q: number) {} }
  return <div className={a ? "x" : undefined} onClick={() => setS(s + 1)}>{b.map((x) => <span key={x}>{x}</span>)}<>{s satisfies number}</></div>;
}
`;
const big = Array.from({ length: 40 }, (_, i) => "function w" + i + "() {" + src.replace(/import .*\n/, "").replace("export ", "") + "}").join("\n");
const opts = { sourceType: "module", plugins: ["typescript", "jsx", ["decorators", { decoratorsBeforeExport: true }], "decoratorAutoAccessors"] };
function count(node, m = {}) { if (node && typeof node.type === "string") m[node.type] = (m[node.type] || 0) + 1; for (const k in node) { if (k === "loc" || k === "extra") continue; const v = node[k]; if (v && typeof v === "object") { if (Array.isArray(v)) v.forEach((x) => count(x, m)); else count(v, m); } } return m; }
export default {
  check: {
    counts: () => count(parse(src, opts)),
    json: () => JSON.stringify(parse("const a = <T,>(x: T) => x as unknown as T[]; a?.b!.c ??= 1n;", { ...opts, plugins: ["typescript"] }).program, (k, v) => (k === "loc" ? undefined : typeof v === "bigint" ? String(v) : v)),
    errors: () => ["let x: = 1", "<a></b>", "class { }"].map((s) => { try { parse(s, opts); return "ok"; } catch (e) { return e.message; } }),
    recovery: () => parse("let a = ; let b = 2", { errorRecovery: true }).errors.map((e) => e.reasonCode),
  },
  bench: {
    parseBig: () => parse(big, opts).program.body.length,
  },
};

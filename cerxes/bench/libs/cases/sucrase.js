import { transform } from "sucrase";
const src = `
import React, { useState } from "react";
import type { X } from "./x";
interface P { a?: number; b: string[] }
enum E { A = 1, B }
export default function C({ a, b }: P): JSX.Element {
  const [s, set] = useState<number>(0);
  class K { constructor(private q: number, public r = 2) {} m = (x: number): number => x * this.q; }
  return <div className={a ? "x" : undefined} onClick={() => set(s + 1)}>{b.map((x) => <span key={x}>{x}</span>)}<>{s as number}</></div>;
}
export const f = async (x?: number) => x ?? (await Promise.resolve(1));
`;
const big = Array.from({ length: 20 }, () => src.replace(/^import.*$/gm, "").replace("export default function C", "export function C")).map((s, i) => s.replace(/\bC\b/, "C" + i).replace(/\bE\b/g, "E" + i).replace(/\bf\b =/, "f" + i + " =")).join("\n");
export default {
  check: {
    tsx: () => transform(src, { transforms: ["typescript", "jsx", "imports"], production: true }).code,
    jsxAuto: () => transform(src, { transforms: ["typescript", "jsx"], jsxRuntime: "automatic", production: true }).code,
  },
  bench: {
    tsx: () => transform(big, { transforms: ["typescript", "jsx", "imports"] }).code.length,
  },
};

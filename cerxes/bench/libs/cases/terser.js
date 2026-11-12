import { minify } from "terser";
const src = `
function fib(n) { if (n < 2) { return n; } else { return fib(n - 1) + fib(n - 2); } }
class Point { constructor(x, y) { this.x = x; this.y = y; } get length() { return Math.sqrt(this.x * this.x + this.y * this.y); } add(other) { return new Point(this.x + other.x, this.y + other.y); } }
const unused = 42; let counter = 0;
export function run(items) { const result = []; for (const item of items) { if (item !== undefined && item !== null) { result.push(item * 2); counter++; } } if (false) { console.log("dead"); } return result.reduce((a, b) => a + b, 0) + fib(10) + new Point(3, 4).length; }
export const obj = { longPropertyName: 1, anotherLongPropertyName: "two", ["computed" + 1]: [1, 2, 3].map(function (x) { return x + 1; }) };
`;
const big = Array.from({ length: 15 }, (_, i) => src.replace(/\bfib\b/g, "fib" + i).replace(/\bPoint\b/g, "Point" + i).replace(/\brun\b/g, "run" + i).replace(/\bobj\b/g, "obj" + i).replace(/\bcounter\b/g, "counter" + i).replace(/\bunused\b/g, "unused" + i)).join("\n");
export default {
  check: {
    minify: () => minify(src, { module: true, compress: { passes: 2 }, mangle: true }).then((r) => r.code),
    beautify: () => minify(src, { module: true, compress: false, mangle: false, format: { beautify: true } }).then((r) => r.code),
    sourceMap: () => minify("function add(first, second) { return first + second; } add(1, 2);", { sourceMap: true, toplevel: true }).then((r) => [r.code, JSON.parse(r.map).mappings]),
  },
  bench: {
    minifyBig: () => minify(big, { module: true, compress: true, mangle: true }).then((r) => r.code.length),
  },
};

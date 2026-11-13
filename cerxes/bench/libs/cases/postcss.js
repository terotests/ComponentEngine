import postcss from "postcss";
import { hash } from "../runtime.js";
const css = Array.from({ length: 200 }, (_x, i) => `.c${i} > a:hover, #id${i} [data-x="${i}"] { color: rgb(${i % 255}, 10, 20); margin: ${i}px 0 0 ${i % 7}em; display: flex; }
@media (min-width: ${300 + i}px) { .m${i} { transform: rotate(${i}deg); user-select: none } }
/* comment ${i} */`).join("\n");
const prefix = { postcssPlugin: "prefix", Declaration: { "user-select": (d) => { d.cloneBefore({ prop: "-webkit-user-select" }); } } };
const px = { postcssPlugin: "px", Declaration(d) { d.value = d.value.replace(/(\d+)px/g, (_m, n) => n / 16 + "rem"); } };
export default {
  check: {
    roundTrip: () => hash(postcss.parse(css).toString()),
    walk: () => { const r = postcss.parse(css); let n = 0, rules = 0, at = 0; r.walkDecls(() => n++); r.walkRules(() => rules++); r.walkAtRules(() => at++); return [n, rules, at]; },
    plugins: () => postcss([prefix, px]).process(css.slice(0, 400), { from: undefined }).css,
    error: () => { try { postcss.parse("a { color: red"); return "ok"; } catch (e) { return e.name + ":" + e.reason; } },
  },
  bench: { parse: () => postcss.parse(css).nodes.length, process: () => postcss([prefix, px]).process(css, { from: undefined }).css.length },
};

import { minify, syntax } from "csso";
import { hash } from "../runtime.js";
const css = Array.from({ length: 150 }, (_x, i) => `.c${i % 40} { color: #ff0000; margin: 0px 0px 0px 0px; padding: ${i % 5}px; background: url("a.png") no-repeat; }
.d${i} > a:hover { color: rgb(255, 0, 0); font-weight: bold; }
.c${i % 40} { padding: 1px; }
@media screen and (min-width: ${400 + (i % 3) * 100}px) { .e${i % 10} { display: none } }`).join("\n");
export default {
  check: {
    small: () => minify(css.slice(0, 600)).css,
    restructure: () => [minify(css).css.length, minify(css, { restructure: false }).css.length],
    bigHash: () => hash(minify(css).css),
    ast: () => syntax.generate(syntax.parse(".a{color:red}.b{x:1px}")),
  },
  bench: { minify: () => minify(css).css.length },
};

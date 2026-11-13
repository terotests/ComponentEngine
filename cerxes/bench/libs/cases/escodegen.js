import * as acorn from "acorn";
import escodegen from "escodegen";
import { hash } from "../runtime.js";
const src = `function A(a, b) { var self = this; this.x = a || b; }
A.prototype.m = function (o) { for (var k in o) { if (!o.hasOwnProperty(k)) continue; this[k] = o[k] * 2 + (k === "q" ? 1 : -1); } return typeof o === "object" ? [1, 2, , 4] : { a: 1, "b-c": [o] }; };
label: for (var i = 0; i < 3; i++) { if (i) continue label; else break; } do { i--; } while (i > 0 && !(i % 2));
switch (x) { case 1: y = (1, 2); break; default: try { throw new Error("e") } catch (e) { y = void 0 } finally { delete o.p } }
var re = /a[b-c]+/gi, n = -(-1), s = "q\\"", f = new (g())(1)(2), t = a ? b : c ? d : e;`;
const big = src.repeat(40);
const ast = acorn.parse(big, { ecmaVersion: 5 });
export default {
  check: {
    gen: () => escodegen.generate(acorn.parse(src, { ecmaVersion: 5 })),
    compact: () => escodegen.generate(acorn.parse("if (a) { b(1 + 2 * 3) } else c = d ? e : f", { ecmaVersion: 5 }), { format: { compact: true } }),
    bigHash: () => hash(escodegen.generate(ast)),
  },
  bench: { generate: () => escodegen.generate(ast).length },
};

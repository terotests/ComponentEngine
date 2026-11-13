import CoffeeScript from "coffeescript/lib/coffeescript-browser-compiler-modern/coffeescript.js";
import { hash } from "../runtime.js";
const src = `
class Animal
  constructor: (@name) ->
  move: (meters = 5) -> "#{@name} moved #{meters}m."
class Snake extends Animal
  move: -> super 5
square = (x) -> x * x
cubes = (square(n) * n for n in [1..10] when n % 2)
obj = {a: 1, b: [1, 2, 3]}
{a, b} = obj
result = switch a
  when 1 then "one"
  else "many"
fn = (args...) => args.length
try
  throw new Error "boom" unless cubes?
catch e then console.log e
`;
const big = Array.from({ length: 30 }, (_x, i) => src.replace(/Animal/g, "Animal" + i).replace(/Snake/g, "Snake" + i)).join("\n");
export default {
  check: {
    compile: () => CoffeeScript.compile(src, { bare: true }),
    tokens: () => CoffeeScript.tokens("a = (x) -> x + 1").map((t) => t[0]),
    error: () => { try { CoffeeScript.compile("a = (->"); return "ok"; } catch (e) { return e.name + ":" + e.message; } },
    bigHash: () => hash(CoffeeScript.compile(big, { bare: true })),
  },
  bench: { compileBig: () => CoffeeScript.compile(big, { bare: true }).length },
};

import peggy from "peggy";
const grammar = `
start = _ e:expr _ { return e; }
expr = head:term tail:(_ ("+" / "-") _ term)* { return tail.reduce((r, t) => t[1] === "+" ? r + t[3] : r - t[3], head); }
term = head:factor tail:(_ ("*" / "/") _ factor)* { return tail.reduce((r, t) => t[1] === "*" ? r * t[3] : r / t[3], head); }
factor = "(" _ e:expr _ ")" { return e; } / num
num "number" = d:$([0-9]+ ("." [0-9]+)?) { return parseFloat(d); }
_ = [ \\t\\n]*
`;
const parser = peggy.generate(grammar);
const exprs = Array.from({ length: 300 }, (_x, i) => `(${i} + ${i % 7}) * ${i % 5 + 1} - ${i}/4 + (((1 + 2) * 3) - ${i % 3})`);
export default {
  check: {
    eval: () => exprs.slice(0, 20).map((e) => parser.parse(e)),
    source: () => peggy.generate(grammar, { output: "source" }).length > 1000,
    error: () => { try { parser.parse("1 + * 2"); return "ok"; } catch (e) { return e.message; } },
  },
  bench: { parse: () => exprs.reduce((s, e) => s + parser.parse(e), 0), generate: () => typeof peggy.generate(grammar).parse },
};

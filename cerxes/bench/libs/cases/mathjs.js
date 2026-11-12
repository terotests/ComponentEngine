import { create, all } from "mathjs";
const math = create(all, { number: "number" });
const big = create(all, { number: "BigNumber", precision: 40 });
export default {
  check: {
    evaluate: () => [math.evaluate("sqrt(3^2 + 4^2) * sin(pi/6)"), math.evaluate("f(x) = x^2 + 1; f(4)").entries, math.format(math.evaluate("12.7 cm to inch"), { precision: 8 }), math.evaluate("det([1,2;3,4])"), math.format(math.evaluate("[1,2;3,4] * inv([1,2;3,4])"), { precision: 10 })],
    symbolic: () => [math.derivative("x^3 + 2x*sin(x)", "x").toString(), math.simplify("2x + 3x - x*2 + 4/2").toString(), math.rationalize("(x+1)/(x-1) + 2").toString(), math.parse("a * (b + c) / -d").toTex()],
    matrices: () => { const m = math.matrix([[4, -2, 1], [-2, 4, -2], [1, -2, 4]]); const e = math.eigs(m); return [math.format(math.lusolve(m, [1, 2, 3]), 10), math.format(e.values, 10), math.format(math.multiply(m, math.transpose(m)), 6), math.format(math.qr(m).R, 8)]; },
    complex: () => [math.format(math.sqrt(math.complex(-4, 3)), 12), math.format(math.exp(math.complex(0, math.pi)), 12), math.format(math.fraction(1, 3).add(math.fraction(1, 6))), math.format(math.unit("5 km/h").to("m/s"), 10)],
    bignumber: () => [big.format(big.evaluate("1/3 + 2/7"), { precision: 40 }), big.evaluate("2^200").toString(), big.format(big.pi, { precision: 40 })],
    stats: () => [math.mean([1, 2, 3, 4, 10]), math.std([1, 2, 3, 4, 10]), math.median([5, 1, 3]), math.mode([1, 2, 2, 3]), math.combinations(20, 7), math.gamma(5.5), math.erf(0.5)],
  },
  bench: {
    evaluate: () => { let s = 0; for (let i = 0; i < 100; i++) s += math.evaluate(`sin(${i}) * cos(${i} / 2) + sqrt(${i} + 1)^2`); return s; },
    parseCompile: () => math.compile("a * x^3 + b * x^2 + c * log(x + 1) - d").evaluate({ a: 1, b: 2, c: 3, d: 4, x: 5 }),
    matrix: () => { const m = math.random([30, 30], -1, 1); return math.det(math.add(m, math.multiply(math.identity(30), 5))); },
    derivative: () => math.derivative("x^4 * sin(x) / (1 + cos(x)^2)", "x").toString().length,
  },
};

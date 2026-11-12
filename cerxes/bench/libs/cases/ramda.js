import * as R from "ramda";
const data = Array.from({ length: 2000 }, (_x, i) => ({ id: i, dept: ["a", "b", "c", "d"][i % 4], pay: 1000 + (i * 7919) % 5000, name: "n" + (i * 31 % 997) }));
const pipeline = R.pipe(R.filter(R.propSatisfies(R.gt(R.__, 3000), "pay")), R.groupBy(R.prop("dept")), R.map(R.pipe(R.pluck("pay"), R.mean, Math.round)), R.toPairs, R.sortBy(R.head));
export default {
  check: {
    pipeline: () => pipeline(data),
    lenses: () => { const l = R.lensPath(["a", "b", 1]); const o = { a: { b: [1, 2, 3] } }; return [R.view(l, o), R.set(l, 9, o), R.over(l, R.multiply(10), o), o]; },
    curry: () => { const f = R.curry((a, b, c) => a * 100 + b * 10 + c); return [f(1)(2)(3), f(1, 2)(3), f(R.__, 2, 3)(1), R.flip(R.subtract)(1, 10), R.converge(R.divide, [R.sum, R.length])([1, 2, 3, 4])]; },
    transduce: () => R.transduce(R.compose(R.map(R.add(1)), R.filter((x) => x % 3 === 0), R.take(5)), R.flip(R.append), [], R.range(0, 100)),
    misc: () => [R.evolve({ a: R.inc, b: { c: R.toUpper } }, { a: 1, b: { c: "x", d: 2 } }), R.mergeDeepRight({ a: { b: 1, c: 2 } }, { a: { b: 3 } }), R.zipObj(["a", "b"], [1, 2]), R.splitEvery(3, R.range(0, 10)), R.uniqWith(R.eqBy(Math.abs), [1, -1, 2, -3, 3]), R.cond([[R.equals(0), R.always("zero")], [R.T, R.toString]])(5), R.equals({ a: [1, new Set([2])] }, { a: [1, new Set([2])] })],
    strings: () => R.pipe(R.split(" "), R.map(R.pipe(R.toLower, R.reverse)), R.reject(R.isEmpty), R.join("-"))("Hello  Wonderful Functional World"),
  },
  bench: {
    pipeline: () => pipeline(data).length,
    lensUpdates: () => { let o = { a: { b: R.range(0, 20) } }; for (let i = 0; i < 300; i++) o = R.over(R.lensPath(["a", "b", i % 20]), R.add(1), o); return o.a.b[3]; },
    sortWith: () => R.sortWith([R.ascend(R.prop("dept")), R.descend(R.prop("pay"))], data)[0].id,
  },
};

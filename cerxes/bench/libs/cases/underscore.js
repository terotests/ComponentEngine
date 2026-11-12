import _ from "underscore";
const data = Array.from({ length: 2000 }, (_x, i) => ({ id: i, g: i % 13, s: "k" + (i * 37 % 101), v: (i * 7) % 100 }));
export default {
  check: {
    collections: () => [_.pluck(_.sortBy(data, "s").slice(0, 4), "id"), _.countBy(data, (d) => d.v % 4), _.max(data, "v").id, _.min(data, (d) => d.s).id, _.size(_.groupBy(data, "g"))],
    chain: () => _.chain(data).where({ g: 3 }).map((d) => d.v).uniq().sortBy(_.identity).first(6).value(),
    template: () => _.template("<% _.each(xs, function(x, i){ %>[<%= i %>:<%- x %>]<% }) %>")({ xs: ["a", "<b>", "c&"] }),
    objects: () => [_.invert({ a: "x", b: "y" }), _.pairs({ a: 1, b: 2 }), _.defaults({ a: 1 }, { a: 2, b: 3 }), _.isMatch({ a: 1, b: 2 }, { b: 2 }), _.has({ a: { b: 1 } }, ["a", "b"]), _.isEqual({ a: [1, { b: new Date(0) }] }, { a: [1, { b: new Date(0) }] })],
    functions: () => { let c = 0; const once = _.once(() => ++c); once(); once(); const b = _.bind(function (x) { return this.k + x; }, { k: 2 }); return [c, b(3), _.compose((x) => x + 1, (x) => x * 2)(5), _.partial((a, b, c) => [a, b, c], _, 2)(1, 3)]; },
    arrays: () => [_.flatten([1, [2, [3, [4]]]]), _.flatten([1, [2, [3]]], true), _.union([1, 2], [2, 3]), _.object(["a", "b"], [1, 2]), _.sortedIndex([10, 20, 30], 25), _.range(10, 0, -3), _.chunk([1, 2, 3, 4, 5], 2)],
  },
  bench: {
    groupSort: () => _.chain(data).groupBy("g").map((v, k) => [k, _.reduce(v, (a, d) => a + d.v, 0)]).sortBy((p) => -p[1]).value().length,
    whereUniq: () => { let n = 0; for (let g = 0; g < 13; g++) n += _.uniq(_.pluck(_.where(data, { g }), "s")).length; return n; },
  },
};

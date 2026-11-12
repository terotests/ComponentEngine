import _ from "lodash";
const data = Array.from({ length: 2000 }, (_x, i) => ({ id: i, name: "user" + (i * 7919 % 2000), age: 18 + (i * 31 % 60), tags: ["t" + (i % 5), "g" + (i % 3)], nested: { a: { b: i % 10 } } }));
export default {
  check: {
    groupBy: () => _.mapValues(_.groupBy(data, (d) => d.age % 7), (v) => v.length),
    sortBy: () => _.sortBy(data, ["age", "name"]).slice(0, 5).map((d) => d.id),
    chain: () => _.chain(data).filter((d) => d.age > 40).map("name").uniq().sortBy().take(5).value(),
    deep: () => { const a = _.cloneDeep(data.slice(0, 50)); a[3].nested.a.b = 99; return [_.isEqual(a, data.slice(0, 50)), _.get(a, "[3].nested.a.b"), _.isEqual(_.cloneDeep(data), data)]; },
    merge: () => _.merge({ a: [1, { b: 2 }], c: { d: 1 } }, { a: [3, { e: 4 }], c: { f: [1, 2] } }),
    template: () => _.template("Hello <%= user %>! <% _.forEach(list, function(x){ %><li><%- x %></li><% }); %>")({ user: "Tero", list: ["<a>", "b&c"] }),
    strings: () => [_.camelCase("Foo Bar-baz_qux"), _.kebabCase("fooBarBaz"), _.startCase("--foo-bar--"), _.deburr("déjà vu Ångström"), _.words("fred, barney, & pebbles"), _.pad("abc", 8, "_-"), _.truncate("hi-diddly-ho there, neighborino", { length: 24, separator: /,? +/ })],
    math: () => [_.sum(_.range(0, 1000, 3)), _.meanBy(data, "age"), _.maxBy(data, "age").id, _.chunk(_.range(10), 3), _.zip([1, 2], ["a", "b"], [true]), _.difference(_.range(20), _.range(0, 20, 3)), _.intersectionBy([2.1, 1.2], [2.3, 3.4], Math.floor)],
    fn: () => { let n = 0; const m = _.memoize((x) => (n++, x * 2)); m(2); m(2); m(3); const c = _.curry((a, b, c) => a + b + c); const p = _.partial((a, b) => a - b, 10); return [n, c(1)(2)(3), c(1, 2)(3), p(4), _.flow([_.add, (x) => x * 2])(1, 2), _.negate(Boolean)(0)]; },
    sets: () => [_.uniqBy(data, "age").length, _.countBy(data, (d) => d.tags[0]), _.keyBy(data.slice(0, 3), "name"), _.pick(data[5], ["id", "nested.a.b"]), _.omit(data[5], ["tags", "nested"])],
  },
  bench: {
    groupBySort: () => _.sortBy(_.toPairs(_.groupBy(data, "age")), 0).length,
    cloneDeepEqual: () => _.isEqual(_.cloneDeep(data), data),
    chain: () => _.chain(data).filter((d) => d.age % 2).map((d) => ({ ...d, k: d.name + d.age })).orderBy(["age", "k"], ["desc", "asc"]).groupBy("age").mapValues((v) => _.sumBy(v, "id")).value(),
    strings: () => { let s = 0; for (let i = 0; i < 500; i++) s += _.camelCase("foo bar " + i).length + _.kebabCase("FooBar" + i).length; return s; },
  },
};

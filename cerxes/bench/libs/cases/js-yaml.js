import * as yaml from "js-yaml";
const doc = `
anchors:
  base: &base { name: x, list: [1, 2, 3] }
  derived:
    <<: *base
    extra: true
multi: |
  line one
  line two
folded: >
  folded
  text
dates: [2024-02-27, 2024-02-27T13:45:07Z]
nums: [0x1F, 0o17, 1e3, .inf, -.Inf, .NaN, 1_000]
"quoted key": 'it''s'
nulls: [~, null, ""]
nested:
  - a: 1
    b: [x, {c: d}]
  - !!str 123
`;
const big = { items: Array.from({ length: 300 }, (_, i) => ({ id: i, name: "item " + i, tags: ["a", "b" + (i % 5)], props: { x: i * 1.5, flag: i % 2 === 0, note: i % 7 ? null : "multi\nline" } })) };
const bigText = yaml.dump(big);
export default {
  check: {
    load: () => yaml.load(doc),
    dump: () => yaml.dump({ a: [1, "two", { three: 3 }], s: "multi\nline", d: new Date(0), e: "", q: "yes", n: null, long: "x".repeat(100) }, { lineWidth: 40 }),
    roundtrip: () => JSON.stringify(yaml.load(bigText)) === JSON.stringify(big),
    errors: () => { try { yaml.load("a: [1, 2\nb: c"); return "ok"; } catch (e) { return e.reason + "@" + e.mark.line; } },
  },
  bench: {
    load: () => yaml.load(bigText).items.length,
    dump: () => yaml.dump(big).length,
  },
};

import MiniSearch from "minisearch";
const words = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa".split(" ");
const docs = Array.from({ length: 2000 }, (_x, i) => ({ id: i, title: words[i % 16] + " " + words[(i * 3) % 16], text: Array.from({ length: 12 }, (_y, j) => words[(i * 7 + j * 5) % 16]).join(" ") + " doc" + i }));
function build() { const ms = new MiniSearch({ fields: ["title", "text"], storeFields: ["title"] }); ms.addAll(docs); return ms; }
const ms = build();
const r = (res) => res.slice(0, 5).map((x) => x.id + ":" + x.score.toFixed(4));
export default {
  check: {
    exact: () => r(ms.search("charlie delta")),
    fuzzy: () => r(ms.search("bravp", { fuzzy: 0.2 })),
    prefix: () => r(ms.search("nov", { prefix: true, boost: { title: 2 } })),
    suggest: () => ms.autoSuggest("ech").slice(0, 3).map((s) => s.suggestion),
  },
  bench: { index: () => build().documentCount, search: () => ms.search("hotel india", { fuzzy: 0.2, prefix: true }).length },
};

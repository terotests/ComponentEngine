import Fuse from "fuse.js";
const words = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet", "kilo", "lima"];
const list = Array.from({ length: 2000 }, (_x, i) => ({ id: i, title: words[i % 12] + " " + words[(i * 7) % 12] + " " + i, author: { name: words[(i * 5) % 12] + "son" } }));
const fuse = new Fuse(list, { keys: ["title", "author.name"], includeScore: true, threshold: 0.4 });
const r = (res) => res.slice(0, 5).map((x) => x.item.id + ":" + x.score.toFixed(4));
export default {
  check: {
    fuzzy: () => r(fuse.search("charly delta")),
    nested: () => r(fuse.search("echoson")),
    extended: () => new Fuse(list, { keys: ["title"], useExtendedSearch: true }).search("^alpha !bravo 1$").slice(0, 5).map((x) => x.item.id),
  },
  bench: { search: () => fuse.search("hotl kilo").length, index: () => new Fuse(list, { keys: ["title", "author.name"] }).search("lima").length },
};

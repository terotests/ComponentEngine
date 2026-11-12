import * as pako from "pako";
const text = Array.from({ length: 3000 }, (_, i) => `line ${i}: ${"lorem ipsum dolor sit amet ".repeat((i % 5) + 1)} ${(i * 7919) % 1000}`).join("\n");
const bytes = new TextEncoder().encode(text);
const rnd = new Uint8Array(100000); { let s = 1; for (let i = 0; i < rnd.length; i++) { s = (s * 1103515245 + 12345) >>> 0; rnd[i] = s >>> 24; } }
function fnv(b) { let h = 0x811c9dc5; for (let i = 0; i < b.length; i++) h = Math.imul(h ^ b[i], 0x01000193); return (h >>> 0).toString(16) + ":" + b.length; }
const def = pako.deflate(bytes);
export default {
  check: {
    deflate: () => [fnv(pako.deflate(bytes)), fnv(pako.deflate(bytes, { level: 1 })), fnv(pako.deflateRaw(bytes, { level: 9 })), fnv(pako.gzip(bytes))],
    inflate: () => [fnv(pako.inflate(def)), pako.inflate(def, { to: "string" }) === text, fnv(pako.ungzip(pako.gzip(rnd)))],
    errors: () => { try { pako.inflate(new Uint8Array([1, 2, 3, 4])); return "ok"; } catch (e) { return String(e); } },
  },
  bench: {
    deflate: () => pako.deflate(bytes).length,
    inflate: () => pako.inflate(def).length,
    deflateRandom: () => pako.deflate(rnd, { level: 6 }).length,
  },
};

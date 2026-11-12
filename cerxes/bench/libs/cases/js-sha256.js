import { sha256, sha224 } from "js-sha256";
const text = Array.from({ length: 2000 }, (_x, i) => "line " + i + " äö€ ").join("\n");
const bytes = new Uint8Array(200000).map((_x, i) => (i * 31) & 255);
export default {
  check: {
    hashes: () => [sha256(""), sha256("abc"), sha224("abc"), sha256(text)],
    hmac: () => sha256.hmac("key", "The quick brown fox jumps over the lazy dog"),
    bytes: () => sha256(bytes),
    incremental: () => { const h = sha256.create(); for (let i = 0; i < 100; i++) h.update("chunk" + i); return h.hex(); },
  },
  bench: { text: () => sha256(text).length, bytes: () => sha256(bytes).length },
};

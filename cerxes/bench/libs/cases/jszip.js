import JSZip from "jszip";
const text = Array.from({ length: 2000 }, (_x, i) => `row ${i}, value ${(i * 7919) % 1000}, ${"abc ".repeat(i % 6)}`).join("\n");
function fnv(b) { let h = 0x811c9dc5; for (let i = 0; i < b.length; i++) h = Math.imul(h ^ b[i], 0x01000193); return (h >>> 0).toString(16) + ":" + b.length; }
const opts = { type: "uint8array", compression: "DEFLATE", compressionOptions: { level: 6 } };
function make() { const z = new JSZip(); z.file("a.txt", text, { date: new Date(Date.UTC(2020, 0, 1)) }); z.file("d/b.json", JSON.stringify({ x: 1, y: [1, 2] }), { date: new Date(Date.UTC(2020, 0, 1)), createFolders: false }); return z; }
export default {
  check: {
    generate: async () => fnv(await make().generateAsync(opts)),
    load: async () => { const z = await JSZip.loadAsync(await make().generateAsync(opts)); return [Object.keys(z.files), (await z.file("a.txt").async("string")) === text, await z.file("d/b.json").async("string")]; },
  },
  bench: {
    generate: async () => (await make().generateAsync(opts)).length,
    roundTrip: async () => (await (await JSZip.loadAsync(await make().generateAsync(opts))).file("a.txt").async("string")).length,
  },
};

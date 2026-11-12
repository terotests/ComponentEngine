import LZString from "lz-string";
const text = Array.from({ length: 2000 }, (_, i) => `{"id":${i},"name":"item ${i % 97}","ok":${i % 2 === 0},"tag":"ä€😀"}`).join(",");
const comp = LZString.compressToUTF16(text);
export default {
  check: {
    roundtrips: () => [LZString.decompress(LZString.compress(text)) === text, LZString.decompressFromBase64(LZString.compressToBase64(text)) === text, LZString.decompressFromEncodedURIComponent(LZString.compressToEncodedURIComponent(text)) === text, LZString.decompressFromUint8Array(LZString.compressToUint8Array(text)) === text],
    outputs: () => [LZString.compressToBase64(text).length, LZString.compressToBase64("hello hello hello world äö"), LZString.compressToEncodedURIComponent(text.slice(0, 200))],
  },
  bench: {
    compress: () => LZString.compressToUTF16(text).length,
    decompress: () => LZString.decompressFromUTF16(comp).length,
  },
};

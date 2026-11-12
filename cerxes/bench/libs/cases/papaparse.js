import Papa from "papaparse";
const rows = Array.from({ length: 1000 }, (_, i) => ({ id: i, name: i % 7 ? "name " + i : 'quote "x", comma', price: (i * 1.37).toFixed(2), date: "2024-01-" + String((i % 28) + 1).padStart(2, "0"), note: i % 11 ? "" : "multi\nline" }));
const csv = Papa.unparse(rows);
export default {
  check: {
    parse: () => { const r = Papa.parse(csv, { header: true, dynamicTyping: true, skipEmptyLines: true }); return [r.data.length, r.data[7], r.data[11], r.meta.fields, r.errors.length]; },
    unparse: () => Papa.unparse([["a", "b,c", 'd"e'], [1, null, true]], { quotes: [false, true, false] }),
    detect: () => { const r = Papa.parse("a;b;c\n1;2;3\n4;5;6"); return [r.meta.delimiter, r.data]; },
    errors: () => Papa.parse('a,b\n"1,2\n3,4', { header: true }).errors.map((e) => e.code),
    hash: () => csv.length,
  },
  bench: {
    parse: () => Papa.parse(csv, { header: true, dynamicTyping: true }).data.length,
    unparse: () => Papa.unparse(rows).length,
  },
};

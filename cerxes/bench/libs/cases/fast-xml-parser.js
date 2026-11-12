import { XMLParser, XMLBuilder, XMLValidator } from "fast-xml-parser";
const xml = `<?xml version="1.0"?><catalog xmlns:x="urn:x">` + Array.from({ length: 300 }, (_, i) => `<book id="b${i}" x:lang="${i % 2 ? "fi" : "en"}"><title>Title &amp; ${i}</title><price>${(i * 1.5).toFixed(2)}</price><tags><tag>a</tag><tag>b${i % 3}</tag></tags><![CDATA[raw <${i}>]]><!-- c --></book>`).join("") + `</catalog>`;
const parser = new XMLParser({ ignoreAttributes: false, attributeNamePrefix: "@", parseTagValue: true, cdataPropName: "#cdata", commentPropName: "#c" });
export default {
  check: {
    parse: () => { const j = parser.parse(xml); return [j.catalog.book.length, j.catalog.book[5], j.catalog.book[299].price]; },
    build: () => new XMLBuilder({ ignoreAttributes: false, attributeNamePrefix: "@", format: true }).build({ root: { "@a": "1", item: [{ "#text": "x & y" }, { b: true, n: 3 }] } }),
    validate: () => [XMLValidator.validate(xml), XMLValidator.validate("<a><b></a>")],
  },
  bench: {
    parse: () => parser.parse(xml).catalog.book.length,
    roundtrip: () => new XMLBuilder({ ignoreAttributes: false, attributeNamePrefix: "@" }).build(parser.parse(xml)).length,
  },
};

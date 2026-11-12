import { PDFDocument, StandardFonts, rgb, degrees } from "pdf-lib";
async function make(pages) {
  const doc = await PDFDocument.create({ updateMetadata: false });
  const font = await doc.embedFont(StandardFonts.Helvetica);
  const bold = await doc.embedFont(StandardFonts.TimesRomanBold);
  for (let p = 0; p < pages; p++) {
    const page = doc.addPage([595, 842]);
    page.drawText("Page " + (p + 1) + " – åäö", { x: 50, y: 790, size: 24, font: bold, color: rgb(0.1, 0.2, 0.6) });
    for (let i = 0; i < 30; i++) page.drawText(`Line ${i}: The quick brown fox ${p * 30 + i}`, { x: 50, y: 750 - i * 22, size: 11, font });
    page.drawRectangle({ x: 400, y: 600, width: 100, height: 80, borderColor: rgb(1, 0, 0), borderWidth: 2, rotate: degrees(10) });
    page.drawSvgPath("M 0,20 L 100,160 Q 130,200 150,120 C 190,-40 200,200 300,150 L 400,90", { x: 100, y: 300, borderColor: rgb(0, 0.5, 0) });
  }
  return doc.save({ useObjectStreams: true });
}
function fnv(b) { let h = 0x811c9dc5; for (let i = 0; i < b.length; i++) h = Math.imul(h ^ b[i], 0x01000193); return (h >>> 0).toString(16); }
export default {
  check: {
    create: async () => { const b = await make(3); return [b.length, fnv(b)]; },
    roundtrip: async () => { const b = await make(2); const d = await PDFDocument.load(b, { updateMetadata: false }); const c = await PDFDocument.create({ updateMetadata: false }); const [p] = await c.copyPages(d, [1]); c.addPage(p); d.removePage(0); return [d.getPageCount(), c.getPageCount(), d.getPage(0).getSize(), (await c.save()).length]; },
    form: async () => { const d = await PDFDocument.create({ updateMetadata: false }); const p = d.addPage(); const f = d.getForm(); const t = f.createTextField("name"); t.setText("Tero"); t.addToPage(p, { x: 50, y: 50 }); const cb = f.createCheckBox("ok"); cb.check(); cb.addToPage(p, { x: 10, y: 10 }); const b = await d.save(); const e = await PDFDocument.load(b); return [e.getForm().getFields().map((x) => x.getName()), e.getForm().getTextField("name").getText(), b.length]; },
  },
  bench: {
    create10: async () => (await make(10)).length,
    load: async () => { if (!globalThis.__pdf10) globalThis.__pdf10 = await make(10); return (await PDFDocument.load(globalThis.__pdf10)).getPageCount(); },
  },
};

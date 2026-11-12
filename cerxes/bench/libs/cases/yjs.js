import * as Y from "yjs";
function fnv(b) { let h = 0x811c9dc5; for (let i = 0; i < b.length; i++) h = Math.imul(h ^ b[i], 0x01000193); return (h >>> 0).toString(16) + ":" + b.length; }
function edits(n) {
  const a = new Y.Doc(); a.clientID = 1; const b = new Y.Doc(); b.clientID = 2;
  const ta = a.getText("t"), tb = b.getText("t"), ma = a.getMap("m"), ab = b.getArray("l");
  for (let i = 0; i < n; i++) {
    ta.insert(Math.min(i % 7, ta.length), "a" + i); tb.insert(0, "b" + (i % 10));
    if (i % 5 === 0 && ta.length > 3) ta.delete(1, 2);
    ma.set("k" + (i % 13), { i, s: "v" + i }); ab.push([i, "x" + i]); if (i % 4 === 3) ab.delete(0, 1);
    if (i % 10 === 9) { const ua = Y.encodeStateAsUpdate(a, Y.encodeStateVector(b)); const ub = Y.encodeStateAsUpdate(b, Y.encodeStateVector(a)); Y.applyUpdate(b, ua); Y.applyUpdate(a, ub); }
  }
  Y.applyUpdate(b, Y.encodeStateAsUpdate(a)); Y.applyUpdate(a, Y.encodeStateAsUpdate(b));
  return [a, b];
}
export default {
  check: {
    converge: () => { const [a, b] = edits(60); return [a.getText("t").toString() === b.getText("t").toString(), a.getText("t").toString().slice(0, 120), b.getArray("l").toJSON().slice(0, 6), a.getMap("m").toJSON(), fnv(Y.encodeStateAsUpdate(a))]; },
    richText: () => { const d = new Y.Doc(); d.clientID = 7; const t = d.getText("r"); t.insert(0, "Hello world"); t.format(0, 5, { bold: true }); t.insert(5, ",", { italic: true }); const log = []; t.observe((e) => log.push(JSON.stringify(e.delta))); t.delete(6, 1); return [t.toDelta(), log]; },
    undo: () => { const d = new Y.Doc(); d.clientID = 3; const t = d.getText("t"); const um = new Y.UndoManager(t, { captureTimeout: 0 }); t.insert(0, "abc"); t.insert(3, "def"); um.undo(); const s1 = t.toString(); um.redo(); return [s1, t.toString()]; },
    xml: () => { const d = new Y.Doc(); d.clientID = 4; const x = d.getXmlFragment("x"); const p = new Y.XmlElement("p"); p.setAttribute("class", "c"); p.insert(0, [new Y.XmlText("hi")]); x.insert(0, [p, new Y.XmlElement("br")]); return x.toString(); },
  },
  bench: {
    edits: () => edits(200)[0].getText("t").length,
    encodeDecode: () => { if (!globalThis.__ydoc) globalThis.__ydoc = edits(200)[0]; const u = Y.encodeStateAsUpdate(globalThis.__ydoc); const d = new Y.Doc(); Y.applyUpdate(d, u); return d.getText("t").length; },
  },
};

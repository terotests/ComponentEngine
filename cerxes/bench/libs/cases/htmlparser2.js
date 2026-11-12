import { parseDocument, Parser, DomUtils } from "htmlparser2";
const html = `<!DOCTYPE html><html><head><title>T &amp; t</title><script>if (a < b && c > d) {}</script><style>p > a { x: "</p>" }</style></head><body>` + Array.from({ length: 300 }, (_, i) => `<div class="row r${i % 3}" data-i="${i}"><p>Item <b>${i}</b> &lt;tag&gt; &eacute; &#x263A;<br><img src=x${i}.png alt='a"b'><a href="?q=${i}&amp;x">link</a></p><!-- c${i} --><ul><li>one<li>two</ul></div>`).join("") + `</body></html>`;
export default {
  check: {
    dom: () => { const d = parseDocument(html); const rows = DomUtils.getElementsByTagName("div", d); return [rows.length, DomUtils.getText(rows[5]), DomUtils.getAttributeValue(DomUtils.findOne((e) => e.name === "img", d.children), "alt"), DomUtils.getOuterHTML(rows[1]), DomUtils.textContent(DomUtils.getElementsByTagName("title", d)[0])]; },
    events: () => { const log = []; const p = new Parser({ onopentag: (n, a) => log.push("<" + n + JSON.stringify(a)), ontext: (t) => t.trim() && log.push(t), onclosetag: (n, implied) => log.push("/" + n + (implied ? "*" : "")), oncomment: (c) => log.push("!" + c) }, { decodeEntities: true }); p.write("<p a=1 b='2'>x &amp; y<br/><i>it</p><!--z-->"); p.end(); return log; },
    xmlMode: () => DomUtils.getOuterHTML(parseDocument("<a:b x='1'><c/><![CDATA[q<]]></a:b>", { xmlMode: true }), { xmlMode: true }),
  },
  bench: {
    parse: () => DomUtils.getElementsByTagName("a", parseDocument(html)).length,
    serialize: () => { if (!globalThis.__hdoc) globalThis.__hdoc = parseDocument(html); return DomUtils.getOuterHTML(globalThis.__hdoc).length; },
  },
};

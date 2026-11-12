import { parse, parseFragment, serialize } from "parse5";
import { hash } from "../runtime.js";
const page = `<!DOCTYPE html><html><head><title>T &amp; x</title><meta charset=utf-8></head><body>
<table><tr><td>1<td>2</table><p>unclosed <b>bold <i>both</b> italic</i><ul><li>a<li>b</ul>
<select><option>1<option selected>2</select><svg><circle r=5/></svg><script>if (a < b) {}</script>
<!-- c --><textarea>a<b></textarea><div id=x class="a b" data-q='1'>${"text ".repeat(20)}</div></body></html>`;
const big = page.replace("<body>", "<body>" + Array.from({ length: 150 }, (_x, i) => `<section><h2>S${i}</h2><p>p<a href="/x/${i}">l</a><br>t</p></section>`).join(""));
const names = (n, out = []) => { out.push(n.nodeName); for (const c of n.childNodes || []) names(c, out); return out; };
export default {
  check: {
    serialize: () => serialize(parse(page)),
    tree: () => names(parse(page)).join(" "),
    fragment: () => serialize(parseFragment("<td>x</td><p>a<p>b")),
    bigHash: () => hash(serialize(parse(big))),
  },
  bench: { parseBig: () => parse(big).childNodes.length, roundTrip: () => serialize(parse(big)).length },
};

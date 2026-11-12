import { marked } from "marked";
const doc = `# Title *with* **emphasis**

Some paragraph with \`code\`, [a link](http://example.com "t"), and an autolink <https://x.y>.

> quote line one
> quote **two**

1. one
2. two
   - nested *a*
   - nested b

| a | b |
|---|:-:|
| 1 | 2 |
| x | y |

\`\`\`js
const x = 1 < 2 && "q";
\`\`\`

Setext
------

***

Hard  
break and ~~strike~~ and html <span class="q">inline</span> &amp; entities &copy;.
`;
const big = Array.from({ length: 40 }, (_x, i) => doc.replace("Title", "Title " + i)).join("\n\n");
export default {
  check: {
    html: () => marked.parse(doc),
    gfm: () => marked.parse("- [ ] task\n- [x] done\n\nwww.example.com and foo@bar.com\n\n<div>\n*no md*\n</div>"),
    lexer: () => marked.lexer(doc).map((t) => t.type),
    bigHash: () => marked.parse(big).length,
  },
  bench: {
    parseBig: () => marked.parse(big).length,
  },
};

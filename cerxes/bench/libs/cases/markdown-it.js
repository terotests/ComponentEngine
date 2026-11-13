import markdownit from "markdown-it";
import { hash } from "../runtime.js";
const md = markdownit({ html: true, linkify: true, typographer: true });
const doc = `# Title *with* **emphasis**

Paragraph with \`code\`, [link](http://example.com "t"), www.example.com and "quotes" -- dashes (c) (tm)...

> quote **two**

1. one
2. two
   - nested *a*

| a | b |
|---|:-:|
| 1 | 2 |

\`\`\`js
const x = 1 < 2 && "q";
\`\`\`

<div class="x">html</div>

Term with ~~strike~~ and &amp; &copy; entities.
`;
const big = Array.from({ length: 40 }, (_x, i) => doc.replace("Title", "Title " + i)).join("\n\n");
export default {
  check: {
    html: () => md.render(doc),
    tokens: () => md.parse(doc, {}).map((t) => t.type + ":" + t.tag),
    inline: () => md.renderInline("*a* __b__ `c` [d](e)"),
    bigHash: () => hash(md.render(big)),
  },
  bench: { renderBig: () => md.render(big).length },
};

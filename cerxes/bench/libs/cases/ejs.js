import ejs from "ejs";
import { hash } from "../runtime.js";
const src = `<h1><%= title %></h1><ul><% items.forEach(function (it, i) { %><li class="<%= i % 2 ? 'odd' : 'even' %>"><%= it.name %> <%- it.raw %> <%= it.price.toFixed(2) %></li><% }) %></ul><%# comment %><p><%= items.length %></p>`;
const tpl = ejs.compile(src);
const items = Array.from({ length: 300 }, (_x, i) => ({ name: "n<" + i + ">", raw: "<b>" + i + "</b>", price: i / 3 }));
export default {
  check: {
    small: () => tpl({ title: "T & U", items: items.slice(0, 3) }),
    bigHash: () => hash(tpl({ title: "T", items })),
    error: () => { try { ejs.render("<% if ( %>", {}); return "ok"; } catch (e) { return e.name; } },
  },
  bench: { render: () => tpl({ title: "T", items }).length, compile: () => typeof ejs.compile(src) },
};

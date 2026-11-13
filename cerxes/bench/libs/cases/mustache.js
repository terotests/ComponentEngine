import Mustache from "mustache";
import { hash } from "../runtime.js";
const tpl = `<h1>{{title}}</h1>{{#items}}<li class="{{#hot}}hot{{/hot}}{{^hot}}cold{{/hot}}">{{name}} &mdash; {{price}} {{{raw}}} {{> tag}}</li>{{/items}}{{^items}}none{{/items}}{{! comment }}`;
const partials = { tag: "<span>{{#tags}}{{.}},{{/tags}}</span>" };
const items = Array.from({ length: 300 }, (_x, i) => ({ name: "n<" + i + ">", price: i * 0.5, hot: i % 3 === 0, raw: "<b>" + i + "</b>", tags: ["a", "b"] }));
export default {
  check: {
    small: () => Mustache.render(tpl, { title: "T & U", items: items.slice(0, 3) }, partials),
    empty: () => Mustache.render(tpl, { title: "x", items: [] }, partials),
    bigHash: () => hash(Mustache.render(tpl, { title: "T", items }, partials)),
    parse: () => Mustache.parse("a {{b}} {{#c}}d{{/c}}").map((t) => t[0]),
  },
  bench: { render: () => Mustache.render(tpl, { title: "T", items }, partials).length },
};

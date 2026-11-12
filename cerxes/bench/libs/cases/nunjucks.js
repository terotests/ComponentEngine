import nunjucks from "nunjucks";
import { hash } from "../runtime.js";
const env = new nunjucks.Environment(null, { autoescape: true });
env.addFilter("money", (n) => n.toFixed(2) + " €");
const src = `{% macro row(i) %}<tr class="{{ loop.cycle('odd','even') if loop else '' }}"><td>{{ i.name | title }}</td><td>{{ i.price | money }}</td><td>{{ i.tags | join(", ") }}</td></tr>{% endmacro %}
<h1>{{ title | upper }}</h1>{% set total = 0 %}<table>{% for i in items %}{% if i.price > 2 %}{{ row(i) }}{% else %}<tr><td>{{ i.name }} &lt;cheap&gt;</td></tr>{% endif %}{% endfor %}</table>
<p>{{ items | length }} items, sum {{ items | sum(attribute="price") | money }}, {{ "<b>" }} {{ "<i>" | safe }}</p>`;
const tpl = nunjucks.compile(src, env);
const items = Array.from({ length: 200 }, (_x, i) => ({ name: "item number " + i, price: (i % 9) * 0.75, tags: ["a", "b" + (i % 3)] }));
export default {
  check: {
    small: () => tpl.render({ title: "Shop", items: items.slice(0, 4) }),
    bigHash: () => hash(tpl.render({ title: "Shop", items })),
    error: () => { try { nunjucks.renderString("{% if %}", {}); return "ok"; } catch (e) { return e.name; } },
  },
  bench: { render: () => tpl.render({ title: "Shop", items }).length, compile: () => typeof nunjucks.compile(src, env).render },
};

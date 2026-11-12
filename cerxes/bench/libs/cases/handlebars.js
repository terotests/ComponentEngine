import Handlebars from "handlebars";
const hb = Handlebars.create();
hb.registerHelper("upper", (s) => String(s).toUpperCase());
hb.registerHelper("eq", (a, b) => a === b);
hb.registerPartial("row", "<tr class='{{#if @odd}}odd{{else}}even{{/if}}'><td>{{@index}}</td><td>{{upper name}}</td><td>{{#each tags}}{{this}}{{#unless @last}}, {{/unless}}{{/each}}</td></tr>");
const tpl = `<h1>{{title}}</h1>{{! comment }}<table>{{#each people}}{{> row odd=@odd}}{{/each}}</table>{{#with boss}}<p>{{name}} &amp; {{{html}}} {{html}}</p>{{/with}}{{#if (eq kind "x")}}X{{else if missing}}M{{else}}none{{/if}} {{lookup map key}} {{people.length}}`;
const ctx = { title: "Team <&>", kind: "y", key: "b", map: { a: 1, b: 2 }, boss: { name: "Q", html: "<b>bold</b>" }, people: Array.from({ length: 200 }, (_, i) => ({ name: "p" + i, tags: ["t" + (i % 3), "s"] })) };
const compiled = hb.compile(tpl);
export default {
  check: {
    render: () => compiled(ctx),
    precompile: () => hb.precompile("{{#each xs}}{{this}}{{/each}}").length > 100,
    errors: () => { try { hb.compile("{{#if x}}open")({}); return "ok"; } catch (e) { return e.message.split("\n")[0]; } },
    ast: () => hb.parse("a {{b.c}} {{#d e=f}}g{{/d}}").body.map((n) => n.type),
  },
  bench: {
    compile: () => hb.compile(tpl + "{{#each people}}{{name}}{{/each}}")(ctx).length,
    render: () => compiled(ctx).length,
  },
};

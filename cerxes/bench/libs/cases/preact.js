import { h, Fragment, createContext } from "preact";
import { useState, useMemo, useContext } from "preact/hooks";
import { renderToString } from "preact-render-to-string";
const Theme = createContext("light");
function Row({ i, item }) { const t = useContext(Theme); const [open] = useState(i % 2 === 0); const label = useMemo(() => item.name.toUpperCase(), [item.name]); return h("tr", { class: t + (open ? " open" : ""), "data-i": i }, h("td", null, i), h("td", { style: { color: i % 3 ? "red" : "blue", fontSize: 12 } }, label), h("td", { dangerouslySetInnerHTML: { __html: "<b>" + item.v + "</b>" } })); }
function Table({ items }) { return h(Theme.Provider, { value: "dark" }, h("table", null, h("tbody", null, items.map((item, i) => h(Row, { key: i, i, item })))), h(Fragment, null, h("p", { hidden: false, title: '"q"&' }, "x < y & z"), null, false, "tail")); }
const items = Array.from({ length: 300 }, (_, i) => ({ name: "item " + i, v: i * 3 }));
export default {
  check: {
    render: () => renderToString(h(Table, { items: items.slice(0, 5) })),
    big: () => renderToString(h(Table, { items })).length,
  },
  bench: {
    render: () => renderToString(h(Table, { items })).length,
  },
};

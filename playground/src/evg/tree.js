// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The tree `__frame` returns (runtime.js), made into EVGElements. A tag is
// the element's name (`span`, `p`, `b`, `label` and `text` are text), `cls`
// its class for the stylesheet, `a` attributes EVG reads (`src`, `role`, …)
// and `s` the inline style: each property goes through EVGElement's own
// `setAttribute`, the same door the stylesheet uses, and is marked inline
// so the sheet does not overrule it.
//
// Text: a text element's string children are its text; a string among a
// box's children becomes a text element of its own (class `text`).

const TEXT_TAGS = new Set(["span", "text", "p", "b", "label"]);

export function buildTree(node, Host) {
  if (typeof node === "string") {
    const t = Host.element("span");
    t.setAttribute("className", "text");
    t.textContent = node;
    return t;
  }
  const el = Host.element(node.t);
  if (node.cls) el.setAttribute("className", node.cls);
  if (node.id) el.setAttribute("id", node.id);
  if (node.a) for (const k in node.a) el.setAttribute(k, node.a[k]);
  if (node.s) for (const k in node.s) Host.setInline(el, k, node.s[k]);
  const kids = node.c || [];
  if (TEXT_TAGS.has(node.t)) {
    let text = "";
    for (const c of kids) if (typeof c === "string") text += c;
    el.textContent = text;
    for (const c of kids) if (typeof c !== "string") el.addChild(buildTree(c, Host));
  } else {
    for (const c of kids) el.addChild(buildTree(c, Host));
  }
  return el;
}

/** How many elements a tree has, for the status line. */
export function countNodes(node) {
  if (typeof node === "string") return 1;
  let n = 1;
  for (const c of node.c || []) n += countNodes(c);
  return n;
}

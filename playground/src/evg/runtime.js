// SPDX-License-Identifier: AGPL-3.0-or-later
//
// What the EVG demo runs in CErXes before the script: the frame entry the page
// calls, and the serializer from the JSX runtime's elements to the tree the
// page turns into EVGElements.
//
// The page calls `__frame(json)` once per animation frame (through cx_call,
// so nothing is compiled per frame). Its argument carries the view's size,
// the time since the last frame, the input state, and the clicks since the
// last frame, each with the id of the element EVG's hit test found under
// the pointer. The clicks go to the handlers the previous `view()`
// registered, then the script's `tick(dt, input)` runs, then `view()`, and
// the element tree comes back as JSON:
//
//   { t: tag, id?, cls?, a?: { attr: value }, s?: { style: value }, c?: [child] }
//
// with a text child as a plain string. A function-valued `on…` prop is kept
// here under an id (the element's own `id`, or one made up), which is how a
// click on the page finds its way back to it.
//
// An element object is serialized once: JSX elements are not changed after
// they are made, so a subtree without handlers keeps its serialized form on
// the object, and a scene the script builds once (and returns again every
// frame) costs nothing after the first frame.
export const RUNTIME = String.raw`
var __handlers = {};
var __ids = 0;
var __input = { keys: {}, pointer: { x: 0, y: 0, down: false }, time: 0 };
var width = 640, height = 480;

var __LENGTH = { width: 1, height: 1, left: 1, top: 1, right: 1, bottom: 1, minWidth: 1, minHeight: 1,
  maxWidth: 1, maxHeight: 1, margin: 1, marginTop: 1, marginLeft: 1, marginRight: 1, marginBottom: 1,
  padding: 1, paddingTop: 1, paddingLeft: 1, paddingRight: 1, paddingBottom: 1, fontSize: 1,
  borderRadius: 1, borderWidth: 1, gap: 1, letterSpacing: 1 };

// Appends n's serialized form to out; answers whether it registered a handler.
function __ser(n, out) {
  if (n === null || n === undefined || n === true || n === false) return false;
  if (Array.isArray(n)) {
    var any = false;
    for (var i = 0; i < n.length; i++) if (__ser(n[i], out)) any = true;
    return any;
  }
  if (typeof n !== "object") { out.push(String(n)); return false; }
  if (typeof n.type !== "string") return false;
  if (n.__s !== undefined) { out.push(n.__s); return false; }
  var handlers = false;
  var e = { t: n.type };
  var p = n.props || {};
  for (var k in p) {
    var v = p[k];
    if (v === null || v === undefined || v === false || k === "key" || k === "children") continue;
    if (typeof v === "function") {
      if (k.slice(0, 2) !== "on") continue;
      if (!e.id) e.id = p.id ? String(p.id) : "__h" + (__ids++);
      __handlers[e.id + ":" + k.slice(2).toLowerCase()] = v;
      handlers = true;
    } else if (k === "className" || k === "class") {
      e.cls = String(v);
    } else if (k === "id") {
      e.id = String(v);
    } else if (k === "style" && typeof v === "object") {
      var s = {};
      for (var sk in v) {
        var sv = v[sk];
        if (sv === null || sv === undefined || sv === false) continue;
        s[sk] = typeof sv === "number" && __LENGTH[sk] ? sv + "px" : String(sv);
      }
      e.s = s;
    } else {
      if (!e.a) e.a = {};
      e.a[k] = String(v);
    }
  }
  var kids = n.children || [];
  if (kids.length) {
    e.c = [];
    for (var j = 0; j < kids.length; j++) if (__ser(kids[j], e.c)) handlers = true;
  }
  if (!handlers) Object.defineProperty(n, "__s", { value: e, enumerable: false });
  out.push(e);
  return handlers;
}

function __frame(arg) {
  var a = JSON.parse(arg);
  width = a.w;
  height = a.h;
  __input.keys = a.keys;
  __input.pointer = a.pointer;
  __input.time = a.time;
  var ev = a.events;
  for (var i = 0; i < ev.length; i++) {
    var e = ev[i];
    if (e.type === "keydown" && typeof onKeyDown === "function") onKeyDown(e.key, __input);
    else if (e.type === "keyup" && typeof onKeyUp === "function") onKeyUp(e.key, __input);
    else if (e.type === "click" || e.type === "pointerdown") {
      var h = __handlers[e.id + ":" + e.type];
      if (h) h({ x: e.x, y: e.y, id: e.id });
      else if (e.type === "pointerdown" && typeof onPointerDown === "function") onPointerDown(e.x, e.y, __input);
    }
  }
  if (typeof tick === "function") tick(a.dt, __input);
  __handlers = {};
  __ids = 0;
  var out = [];
  __ser(typeof view === "function" ? view() : null, out);
  return JSON.stringify(out.length === 1 ? out[0] : { t: "div", c: out });
}
`;

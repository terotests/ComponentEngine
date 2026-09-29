// SPDX-License-Identifier: AGPL-3.0-or-later
//! The JSX runtime, in JavaScript, run once by `Engine::new`: what the
//! default factory `__jsx` makes of an element, `__jsxFragment`, and
//! `renderToString`. All three are non-enumerable globals a script may
//! replace (or bypass with a `@jsx` comment).
//!
//! A host tag becomes `{ type, props, children }`: `props` without
//! `children`, `children` flat, with `null`, `undefined` and booleans left
//! out. A function tag is a component, called at once with its props
//! (`children` among them: one child as itself, more as an array), as
//! ComponentEngine expands components; a class with a `render` method is
//! constructed and rendered. A fragment is the array of its children, which
//! an enclosing element splices in.

pub const PRELUDE: &str = r#"(function (global) {
function hide(name, v) { Object.defineProperty(global, name, { value: v, writable: true, enumerable: false, configurable: true }); }
function flatten(list, out) {
  for (var i = 0; i < list.length; i++) {
    var c = list[i];
    if (Array.isArray(c)) flatten(c, out);
    else if (c !== null && c !== undefined && c !== true && c !== false) out.push(c);
  }
  return out;
}
function Fragment(props) {
  var c = props == null ? undefined : props.children;
  return c === undefined ? [] : flatten([c], []);
}
function jsx(type, props) {
  var kids = [];
  for (var i = 2; i < arguments.length; i++) kids.push(arguments[i]);
  var p = {};
  if (props != null) for (var k in props) p[k] = props[k];
  if (typeof type === 'function') {
    if (kids.length === 1) p.children = kids[0];
    else if (kids.length > 1) p.children = kids;
    if (type.prototype && typeof type.prototype.render === 'function') return new type(p).render();
    return type(p);
  }
  if (kids.length === 0 && p.children !== undefined) kids = [p.children];
  delete p.children;
  return { type: type, props: p, children: flatten(kids, []) };
}
var VOID = { area: 1, base: 1, br: 1, col: 1, embed: 1, hr: 1, img: 1, input: 1, link: 1, meta: 1, source: 1, track: 1, wbr: 1 };
function esc(s) { return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }
function css(o) {
  var out = [];
  for (var k in o) if (o[k] != null) out.push(k.replace(/[A-Z]/g, function (m) { return '-' + m.toLowerCase(); }) + ':' + o[k]);
  return out.join(';');
}
function render(n) {
  if (n === null || n === undefined || n === true || n === false) return '';
  if (Array.isArray(n)) { var s = ''; for (var i = 0; i < n.length; i++) s += render(n[i]); return s; }
  if (typeof n !== 'object') return esc(n);
  if (typeof n.type !== 'string') return '';
  var out = '<' + n.type;
  var props = n.props || {};
  for (var k in props) {
    var v = props[k];
    if (k === 'key' || k === 'ref' || v === null || v === undefined || v === false || typeof v === 'function') continue;
    var name = k === 'className' ? 'class' : k === 'htmlFor' ? 'for' : k;
    if (v === true) out += ' ' + name;
    else if (k === 'style' && typeof v === 'object') out += ' style="' + esc(css(v)) + '"';
    else out += ' ' + name + '="' + esc(v) + '"';
  }
  var kids = n.children || [];
  if (kids.length === 0 && VOID[n.type]) return out + ' />';
  return out + '>' + render(kids) + '</' + n.type + '>';
}
hide('__jsx', jsx);
hide('__jsxFragment', Fragment);
hide('renderToString', render);
})(this);"#;

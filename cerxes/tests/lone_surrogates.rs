// SPDX-License-Identifier: AGPL-3.0-or-later
// Lone surrogates (jsstr.rs: a unit 0xd800..0xdfff without its partner is
// kept as a plane-16 stand-in): lengths, units, joining halves back into
// one character, JSON, URI, isWellFormed, a pair among the stand-ins. Rust
// only (other targets have no stand-ins, so this is not in cases.txt);
// the expected value is Node's.
use cerxes::Engine;

const SCRIPT: &str = r#"var h = "\ud83d", l = "\ude00", e = h + l, o = [];
o.push(e.length, h.length, h.charCodeAt(0), l.charCodeAt(0), e.codePointAt(0), h.codePointAt(0));
o.push(h + l === e, [h, l].join("") === e, `${h}${l}` === e, h.concat(l) === e, String.fromCharCode(0xd83d, 0xde00) === e, String.fromCodePoint(0xd83d, 0xde00) === e);
var s = h; s += l; o.push(s === e, "x".padStart(3, l + h).length, (l + h).repeat(2).slice(1, 3) === e, (h + "x").replace("x", l) === e);
o.push(JSON.stringify(h + "a" + l + e), JSON.parse('"\\ud83d"') === h, h.isWellFormed(), e.isWellFormed(), ("a" + h).toWellFormed() === "a�");
var u = ""; try { encodeURIComponent(h); } catch (x) { u = x.name; } o.push(u, encodeURIComponent(e));
o.push(h < l, l < "", [..."􏰀" + l].length, "􏰀" === "\u{10fc00}", "\u{10fc00}".length, "􏰀".codePointAt(0));
var b = ""; for (var i = 0xdbfd; i < 0xdc03; i++) b += String.fromCharCode(i); o.push(b.length, b.charCodeAt(2), b.charCodeAt(3));
var m = {}; m[h] = 1; o.push(m["\ud83d"], Object.keys(m)[0].charCodeAt(0), (h + "é").normalize().length);
o.join(",")"#;

#[test]
fn lone_surrogates_as_node() {
    let mut e = Engine::new();
    let r = e.eval(SCRIPT);
    assert_eq!(e.error, "");
    assert_eq!(r, r#"2,1,55357,56832,128512,55357,true,true,true,true,true,true,true,3,true,true,"\ud83da\ude00😀",true,false,true,true,URIError,%F0%9F%98%80,true,true,2,true,2,1113088,6,56319,56320,1,55357,2"#);
}

#[test]
fn lone_surrogates_without_snapshot() {
    let mut e = Engine::compiled(true);
    let r = e.eval(SCRIPT);
    assert_eq!(e.error, "");
    assert_eq!(r, r#"2,1,55357,56832,128512,55357,true,true,true,true,true,true,true,3,true,true,"\ud83da\ude00😀",true,false,true,true,URIError,%F0%9F%98%80,true,true,2,true,2,1113088,6,56319,56320,1,55357,2"#);
}

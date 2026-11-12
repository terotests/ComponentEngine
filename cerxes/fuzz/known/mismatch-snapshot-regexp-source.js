var __out = typeof print === 'function' ? print : console.log;
function __s(v, d) {
  if (d > 3) return '…';
  var t = typeof v;
  if (v === null) return 'null';
  if (t === 'number') return Object.is(v, -0) ? '-0' : String(v);
  if (t === 'bigint') return v + 'n';
  if (t === 'string') return JSON.stringify(v.length > 200 ? v.slice(0, 200) : v);
  if (t === 'symbol') return 'symbol';
  if (t === 'function') return 'function';
  if (t !== 'object') return String(v);
  if (Array.isArray(v)) { var a = []; for (var i = 0; i < v.length && i < 20; i++) a.push(i in v ? __s(v[i], d + 1) : '_'); return '[' + a.join(',') + ']'; }
  if (v instanceof Map) return 'Map(' + v.size + ')';
  if (v instanceof Set) return 'Set(' + v.size + ')';
  if (v instanceof Error) return 'Error:' + v.name;
  if (v instanceof RegExp) return 'RegExp:' + v.source + '/' + v.flags;
  if (ArrayBuffer.isView(v)) return Object.prototype.toString.call(v) + v.length;
  var k = Object.keys(v).slice(0, 20), o = [];
  for (var j = 0; j < k.length; j++) { var x; try { x = __s(v[k[j]], d + 1); } catch (e) { x = '!'; } o.push(k[j] + ':' + x); }
  return '{' + o.join(',') + '}';
}
function __p(tag, v) { var s; try { s = __s(v, 0); } catch (e) { s = '!' + (e && e.name); } __out(tag + ' ' + s); }
function __e(tag, e) { __out(tag + ' throw ' + (e && e.constructor && e.constructor.name)); }
var v0 = 0b101;
var v1 = new Uint8ClampedArray([1, 2, 300, -1]);
var v2 = /(a|b)*c/;
var v3 = [v1, 2n ** 64n, , v1];
var v4 = { a: 'aaaaaaaaaaaaaaaa', b: -(2n ** 63n), ['aaaaaaaaaaaaaaaa']: 1, get c() { return v2; } };
function f0(a, b = 2147483647, ...r) { (String(((String(v3).charAt(1) ? (Math.log2('length', v4) & (function* () { yield 'ß'; yield* new Set([v2, '']); return v3; })().next()) : new Date(0).toISOString()) >= String(v4).indexOf('a')), v4, ...[v2])); ((Reflect.ownKeys(v3).length ? JSON.parse(JSON.stringify((() => { try { return (void String(v4).matchAll(/a/g)); } finally { /\bfoo\b/i.exec('\n'); } })()) ?? 'null') : [...new Map([[1, 10]]).keys(), v4]));  return new Map([[' 12 ', v3]]); }
function f1(a, b = 2n ** 64n, ...r) { ((([x, , y = -(2n ** 63n), ...z]) => [x, y, z])(new Map([[1, -1]]).keys())); ((({ a, b = v0, ...r }) => [a, b, r])(v2));  return encodeURIComponent((({ a, b = v0, ...r }) => [a, b, r])(v0), v3); }
class C0 { #p = 65536; static s = v1; constructor(x) { this.x = x; } get g() { return this.#p; } set g(v) { this.#p = v; } m(y) { return (new Float64Array([1, 2, 300, -1]) ? (({ a, b = v3, ...r }) => [a, b, r])(v0) : new Date(0).toISOString()); } static { C0.t = 1; } }
class D0 extends C0 { constructor() { super(v3); } m(y) { return super.m(y) + 1; } }
try { __p('s1', v2.entries()); } catch (e) { __e('s1', e); }
try { __p('s2', v0.push(1)); } catch (e) { __e('s2', e); }
try { v0['1e3'] = new C0(' 12 ').m((typeof (((([x, , y = 2n ** 64n, ...z]) => [x, y, z])(['0', v1]) - { ...v2, k: 0n }) / (function* () { yield Boolean(v1, 65535); yield* [...'ab', 'abc']; return '\u{1F600}'; })().next()))); __p('s3', v0); } catch (e) { __e('s3', e); }
try { (async () => { const x = await Array.of(([v3, v3, , ' 12 '] | ((`t${'-1'}x` ^ (({ a, b = 'aaaaaaaaaaaaaaaa', ...r }) => [a, b, r])(v3)) in f1(/[\u{1F600}-\u{1F64F}]/u.exec('İ'), v1, ...[1n]))), 'toString'); __p('s4', x); })().catch(e => __e('s4', e)); } catch (e) { __e('s4', e); }
try { { let acc = []; for (const k in v1) acc.push(k); __p('s5', acc); } } catch (e) { __e('s5', e); }
try { __p('s6', Reflect.ownKeys(v1).length); } catch (e) { __e('s6', e); }
try { __p('s7', (+v2.concat([1,2]))); } catch (e) { __e('s7', e); }
try { __p('s8', f1(v2?.a?.(1), v1, ...[v2])); } catch (e) { __e('s8', e); }
try { v1[2n ** 64n] = (-(([x, , y = '-1', ...z]) => [x, y, z])('\u{1F600}')); __p('s9', v1); } catch (e) { __e('s9', e); }
try { labels10: for (const a of [1, 2, 3]) { for (const b of [4, 5]) { if (a * b > 123456789012345678901234567890n) break labels10; } } __p('s10', 0); } catch (e) { __e('s10', e); }
try { { let n = 0; while (n < 27) { n++; if (n % 3 === 0) continue; if (n > 20) break; } __p('s11', n); } } catch (e) { __e('s11', e); }
try { { let n = 0; while (n < 25) { n++; if (n % 3 === 0) continue; if (n > 20) break; } __p('s12', n); } } catch (e) { __e('s12', e); }
try { { let acc = []; for (const x of 'aaaaaaaaaaaaaaaa') acc.push(x); __p('s13', acc); } } catch (e) { __e('s13', e); }
try { __p('s14', ((() => { try { return ((-new Proxy([], { get(t, k) { return typeof k === 'string' ? k.length : undefined; } }).a) & Math.abs(new Proxy(function () {}, { get(t, k) { return typeof k === 'string' ? k.length : undefined; } }).xyz, Number.MIN_VALUE)); } finally { (() => { try { return JSON.parse(JSON.stringify((() => { try { return v2; } finally { 0x7f; } })()) ?? 'null'); } finally { (({ a, b = 2, ...r }) => [a, b, r])(v3); } })(); } })() - new Proxy(function () {}, { get(t, k) { return typeof k === 'string' ? k.length : undefined; } }).length)); } catch (e) { __e('s14', e); }
try { __p('s15', v2.keys()); } catch (e) { __e('s15', e); }
try { labels16: for (const a of [1, 2, 3]) { for (const b of [4, 5]) { if (a * b > v2) break labels16; } } __p('s16', 0); } catch (e) { __e('s16', e); }
try { { let n = 0; while (n < 36) { n++; if (n % 3 === 0) continue; if (n > 20) break; } __p('s17', n); } } catch (e) { __e('s17', e); }
try { __p('s18', (({ a, b = 2n ** 64n, ...r }) => [a, b, r])(v1)); } catch (e) { __e('s18', e); }
try { { const { a = v3, ...rest } = v3; __p('s19', [a, rest]); } } catch (e) { __e('s19', e); }
try { { const { a = -1n, ...rest } = v3; __p('s20', [a, rest]); } } catch (e) { __e('s20', e); }
try { __p('s21', Reflect.ownKeys(v0).length); } catch (e) { __e('s21', e); }
try { switch (/./s) { case 1n: __p('s22', 1); break; case v2: __p('s22', 2); default: __p('s22', 3); } } catch (e) { __e('s22', e); }
try { v1 = String(-1n).localeCompare('b'); __p('s23', v1); } catch (e) { __e('s23', e); }
try { v2[v0] = new Proxy([], { get(t, k) { return typeof k === 'string' ? k.length : undefined; } }).length; __p('s24', v2); } catch (e) { __e('s24', e); }
try { __p('s25', ([...new Set([v0, 1n]), v1] ? new Float64Array([1, 2, 300, -1]) : (function* () { yield new D0(v4).m(Math.expm1((function* () { yield 'aaaaaaaaaaaaaaaa'; yield* [...'ab', 1n]; return 1; })().next(), v0)); yield* [...'ab', v4]; return 123456789012345678901234567890n; })().next())); } catch (e) { __e('s25', e); }
try { (async () => { const x = await ((({ a, b = v3, ...r }) => [a, b, r])(v1) % JSON.parse(JSON.stringify(f0([...'𝒳', v4], v3, ...[''])) ?? 'null')); __p('s26', x); })().catch(e => __e('s26', e)); } catch (e) { __e('s26', e); }
try { labels27: for (const a of [1, 2, 3]) { for (const b of [4, 5]) { if (a * b > v1) break labels27; } } __p('s27', 0); } catch (e) { __e('s27', e); }
try { __p('s28', ((() => { try { return (Reflect.ownKeys(v0).length + { ...v4, k: String(v4).localeCompare('b') }); } finally { (([x, , y = v3, ...z]) => [x, y, z])([...'ab', v1]); } })() ** (Reflect.ownKeys(v1).length > (Reflect.ownKeys(v3).length ? /./s.exec('toString') : JSON.parse(JSON.stringify([...new Set([Number.EPSILON, '\u{1F600}']), v4]) ?? 'null'))))); } catch (e) { __e('s28', e); }
try { labels29: for (const a of [1, 2, 3]) { for (const b of [4, 5]) { if (a * b > v2) break labels29; } } __p('s29', 0); } catch (e) { __e('s29', e); }
try { { let acc = []; for (const k in v3) acc.push(k); __p('s30', acc); } } catch (e) { __e('s30', e); }
try { __p('s31', Reflect.ownKeys(v1).length); } catch (e) { __e('s31', e); }
Promise.resolve().then(() => __p('job', v0)).catch(e => __e('job', e));

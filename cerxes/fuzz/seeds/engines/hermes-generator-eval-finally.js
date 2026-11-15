// hermes: generators compiled lazily / from eval, resumed through finally (Hermes generator CVEs) iterator
var mk = eval('(function* (x) { try { yield x; yield eval("x + 1"); } finally { yield "f"; } })');
var g = mk(1);
print(JSON.stringify([g.next(), g.return(9), g.next(), g.next()]));
var h = new Function('return function* () { var a = yield 1; try { throw a; } catch (e) { yield e; } finally { return 7; } }')()();
print(JSON.stringify([h.next(), h.next('x'), h.next(), h.next()]));
function* deep(n) { if (n > 0) yield* deep(n - 1); else yield 'bottom'; }
try { print(deep(3000).next().value); } catch (e) { print(e.name); }

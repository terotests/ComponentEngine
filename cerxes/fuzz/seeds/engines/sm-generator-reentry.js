// sm: generator closed or resumed from inside itself (SpiderMonkey / Hermes generator bugs) iterator
var g;
function* gen() { try { yield 1; g.next(); yield 2; } catch (e) { yield e.name; } finally { try { g.return(5); } catch (e) { print('finally', e.name); } yield 3; } }
g = gen();
print(JSON.stringify(g.next()), JSON.stringify(g.next()), JSON.stringify(g.next()), JSON.stringify(g.next()));
function* inner() { try { yield 1; } finally { outer.throw(new Error('x')); } }
var outer = (function* () { yield* inner(); })();
outer.next();
try { outer.return(1); } catch (e) { print('outer', e.name); }
var ag = (async function* () { try { yield 1; } finally { await null; ag.return(2); } })();
ag.next().then(() => ag.return(3)).then(v => print('ag', JSON.stringify(v)), e => print('ag throw', e.name));

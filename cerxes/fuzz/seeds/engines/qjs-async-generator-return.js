// qjs: async generator return / throw while awaiting (QuickJS UAF reports, V8 async generator bugs) iterator
var order = [];
async function* g() { try { order.push('start'); yield await Promise.resolve(1); order.push('after'); } finally { order.push('fin'); await null; order.push('fin2'); } }
var it = g();
var p1 = it.next(), p2 = it.return(7), p3 = it.next(), p4 = it.throw(new Error('t'));
Promise.allSettled([p1, p2, p3, p4]).then(r => print(r.map(x => x.status + ':' + JSON.stringify(x.value)).join(' '), order.join()));
var sync = { [Symbol.iterator]() { return { next() { return { value: Promise.reject(1), done: false }; }, return() { order.push('closed'); return {}; } }; } };
(async () => { try { for await (const v of sync) {} } catch (e) { print('caught', e, order.includes('closed')); } })();

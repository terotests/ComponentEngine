// sm: CVE-2022-1802 class (prototype pollution reaching internal promise code), V8 await bugs iterator-promise
var log = [];
Object.defineProperty(Object.prototype, 'then', { configurable: true, get() { log.push('then'); delete Object.prototype.then; return undefined; } });
(async function () { await 1; log.push('after await'); })();
async function* ag() { yield 1; yield 2; }
(async function () { for await (const v of ag()) log.push('v' + v); })();
Promise.resolve({ x: 1 }).then(v => log.push('resolved ' + v.x));
Promise.resolve().then(() => Promise.resolve().then(() => print(log.join())));
var it = ag();
Object.prototype.then = function (r) { delete Object.prototype.then; r('poisoned'); };
it.next().then(v => print('next', JSON.stringify(v)));

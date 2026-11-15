// qjs: CVE-2023-31922 (QuickJS js_proxy_isArray stack overflow) native-recursion
var p = [];
for (var i = 0; i < 200000; i++) p = new Proxy(p, {});
try { print(Array.isArray(p)); } catch (e) { print(e.name); }
try { print(JSON.stringify(p)); } catch (e) { print(e.name); }
try { print(Object.prototype.toString.call(p)); } catch (e) { print(e.name); }
var f = function () {};
for (var j = 0; j < 200000; j++) f = new Proxy(f, {});
try { print(typeof f); new f(); print('constructed'); } catch (e) { print(e.name); }

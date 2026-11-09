// scale.js: each operation at N and 4N; a time ratio far above 4 is
// superlinear (an O(n) step inside a loop). <engine> scale.js; Node: ONLY=name.
var out = typeof print === "function" ? print : console.log;
var now = typeof performance !== "undefined" ? function(){return performance.now();} : Date.now;
var ops = {
  "str-concat-loop": function(n){ var s=""; for (var i=0;i<n;i++) s+="ab"; return s.length; },
  "str-concat-charAt": function(n){ var s=""; for (var i=0;i<n;i++) { s+="ab"; if (s.charCodeAt(i) < 0) return 0; } return s.length; },
  "str-prepend": function(n){ var s=""; for (var i=0;i<n;i++) s="ab"+s; return s.length; },
  "arr-push": function(n){ var a=[]; for (var i=0;i<n;i++) a.push(i); return a.length; },
  "arr-unshift": function(n){ var a=[]; for (var i=0;i<n/8;i++) a.unshift(i); return a.length; },
  "arr-shift-queue": function(n){ var a=[]; for (var i=0;i<n;i++) a.push(i); var s=0; while (a.length) s+=a.shift(); return s; },
  "arr-splice-mid": function(n){ var a=[]; for (var i=0;i<n/4;i++) a.splice(a.length>>1,0,i); return a.length; },
  "arr-index-set-grow": function(n){ var a=[]; for (var i=0;i<n;i++) a[i]=i; return a.length; },
  "arr-reverse-set": function(n){ var a=[]; for (var i=n-1;i>=0;i--) a[i]=i; return a.length; },
  "arr-concat-acc": function(n){ var a=[]; for (var i=0;i<n/16;i++) a=a.concat([i]); return a.length; },
  "arr-spread-acc": function(n){ var a=[]; for (var i=0;i<n/16;i++) a=[...a,i]; return a.length; },
  "arr-sort-num": function(n){ var a=[]; for (var i=0;i<n;i++) a.push((i*7919)%n); a.sort(function(x,y){return x-y;}); return a[1]; },
  "arr-sort-default": function(n){ var a=[]; for (var i=0;i<n;i++) a.push((i*7919)%n); a.sort(); return a.length; },
  "arr-includes-miss": function(n){ var a=[]; for (var i=0;i<1000;i++) a.push("k"+i); var c=0; for (var i=0;i<n/10;i++) if (a.includes("x")) c++; return c; },
  "arr-join": function(n){ var a=[]; for (var i=0;i<n;i++) a.push(i); return a.join(",").length; },
  "arr-filter": function(n){ var a=[]; for (var i=0;i<n;i++) a.push(i); return a.filter(function(x){return x&1;}).length; },
  "arr-pop-shrink": function(n){ var a=[]; for (var i=0;i<n;i++) a.push(i); while (a.length) a.pop(); return 0; },
  "arr-length-grow": function(n){ var a=[]; a.length = n; for (var i=0;i<n;i++) a[i]=i; return a.length; },
  "arr-holes-fill": function(n){ var a=new Array(n); for (var i=0;i<n;i+=2) a[i]=i; for (var i=1;i<n;i+=2) a[i]=i; return a.length; },
  "obj-add-keys": function(n){ var o={}; for (var i=0;i<n;i++) o["k"+i]=i; return Object.keys(o).length; },
  "obj-add-int-keys-sparse": function(n){ var o={}; for (var i=0;i<n;i++) o[i*7]=i; var c=0; for (var k in o) c++; return c; },
  "obj-delete-keys": function(n){ var o={}; for (var i=0;i<n;i++) o["k"+i]=i; for (var i=0;i<n;i++) delete o["k"+i]; return Object.keys(o).length; },
  "obj-keys-repeat": function(n){ var o={}; for (var i=0;i<200;i++) o["k"+i]=i; var c=0; for (var i=0;i<n/50;i++) c+=Object.keys(o).length; return c; },
  "obj-spread-big": function(n){ var o={}; for (var i=0;i<n/16;i++) o["k"+i]=i; var c=0; for (var r=0;r<16;r++) c+=Object.keys({...o}).length; return c; },
  "map-set-get": function(n){ var m=new Map(); for (var i=0;i<n;i++) m.set("k"+i,i); var s=0; for (var i=0;i<n;i++) s+=m.get("k"+i); return s; },
  "map-delete": function(n){ var m=new Map(); for (var i=0;i<n;i++) m.set(i,i); for (var i=0;i<n;i++) m.delete(i); return m.size; },
  "map-delete-iter": function(n){ var m=new Map(); for (var i=0;i<n;i++) m.set(i,i); for (var i=0;i<n-1;i++) m.delete(i); var c=0; for (var r=0;r<100;r++) for (var e of m) c++; return c; },
  "set-add-delete-churn": function(n){ var s=new Set(); for (var i=0;i<n;i++) { s.add(i); if (i>=100) s.delete(i-100); } return s.size; },
  "weakmap-set": function(n){ var w=new WeakMap(), ks=[]; for (var i=0;i<n;i++) { var k={}; ks.push(k); w.set(k,i); } var s=0; for (var i=0;i<n;i++) s+=w.get(ks[i]); return s; },
  "json-roundtrip": function(n){ var a=[]; for (var i=0;i<n;i++) a.push({i:i,s:"x"+i}); return JSON.parse(JSON.stringify(a)).length; },
  "str-split-join": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("w"+i); return a.join(" ").split(" ").length; },
  "str-replace-all-fn": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("w"+i); return a.join(" ").replace(/w(\d+)/g, function(m,d){return d;}).length; },
  "str-indexOf-scan": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("w"+i); var s=a.join(" "), p=0, c=0; while ((p = s.indexOf(" ", p)+1) > 0) c++; return c; },
  "str-slice-walk": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("w"+i); var s=a.join(" "), c=0; for (var i=0;i<s.length;i+=7) c+=s.slice(i,i+3).length; return c; },
  "str-charAt-walk-nonascii": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("é"+i); var s=a.join(" "), c=0; for (var i=0;i<s.length;i++) if (s.charCodeAt(i)===233) c++; return c; },
  "str-substring-walk-nonascii": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("é"+i); var s=a.join(" "), c=0; for (var i=0;i<s.length;i+=5) c+=s.substring(i,i+2).length; return c; },
  "str-regex-test-walk": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("w"+i); var s=a.join(" "), c=0, re=/\d/; for (var i=0;i<s.length;i+=3) if (re.test(s[i])) c++; return c; },
  "str-lastIndexOf": function(n){ var a=[]; for (var i=0;i<n;i++) a.push("w"+i); var s=a.join(" "), c=0; for (var r=0;r<100;r++) c+=s.lastIndexOf("w1 "); return c; },
  "str-template-acc": function(n){ var s=""; for (var i=0;i<n;i++) s=`${s}${i&7}`; return s.length; },
  "str-padstart-many": function(n){ var c=0; for (var i=0;i<n;i++) c+=String(i).padStart(8,"0").length; return c; },
  "closure-chain": function(n){ var f=function(){return 0;}; for (var i=0;i<n/10;i++) { (function(g,k){ f=function(){return g()+k;}; })(f,1); if (i%1000==999) f=function(){return 0;}; } return f(); },
  "deep-recursion": function(n){ function r(k){ return k==0?0:1+r(k-1); } var c=0; for (var i=0;i<n/1000;i++) c+=r(1000); return c; },
  "proto-chain-long": function(n){ var o={x:1}; for (var i=0;i<50;i++) o=Object.create(o); var s=0; for (var i=0;i<n;i++) s+=o.x; return s; },
  "class-many-instances": function(n){ class P{constructor(i){this.a=i;this.b=i;}} var a=[]; for (var i=0;i<n;i++) a.push(new P(i)); var s=0; for (var i=0;i<n;i++) s+=a[i].a; return s; },
  "objects-retained-gc": function(n){ var keep=[]; for (var i=0;i<n;i++) { keep.push({i:i, t:[i]}); } var s=0; for (var r=0;r<3;r++) { var tmp=[]; for (var i=0;i<n;i++) tmp.push({j:i}); s+=tmp.length; } return s+keep.length; },
  "strings-retained-gc": function(n){ var keep=[]; for (var i=0;i<n;i++) keep.push("s"+i); var s=0; for (var r=0;r<3;r++) for (var i=0;i<n;i++) s+=("t"+i).length; return s+keep.length; },
  "promise-all": function(n){ var ps=[]; for (var i=0;i<n/10;i++) ps.push(Promise.resolve(i)); Promise.all(ps); return ps.length; },
  "typed-sort": function(n){ var a=new Float64Array(n); for (var i=0;i<n;i++) a[i]=(i*7919)%n; a.sort(); return a[1]; },
  "array-from-map": function(n){ return Array.from({length:n}, function(_,i){return i;}).length; },
  "number-tostring": function(n){ var c=0; for (var i=0;i<n;i++) c+=String(i*1.5).length; return c; },
  "parsefloat": function(n){ var c=0; for (var i=0;i<n;i++) c+=parseFloat("3."+i); return c|0; },
  "date-parse": function(n){ var c=0; for (var i=0;i<n/10;i++) c+=Date.parse("2024-01-0"+(1+i%9)+"T10:00:00Z")>0?1:0; return c; },
  "regex-alternation-big": function(n){ var w=[]; for (var i=0;i<200;i++) w.push("word"+i); var re=new RegExp("\\b(?:"+w.join("|")+")\\b","g"); var t=[]; for (var i=0;i<n/10;i++) t.push("word"+(i%300)); return (t.join(" ").match(re)||[]).length; },
};
var ONLY = (typeof process !== "undefined" && process.env && process.env.ONLY) || "";
var N = 20000;
for (var k in ops) {
  if (ONLY && ONLY !== k) continue;
  var t0 = now(); var r1 = ops[k](N); var t1 = now(); var r2 = ops[k](4*N); var t2 = now();
  var a = t1-t0, b = t2-t1;
  out(k + " " + a.toFixed(1) + " " + b.toFixed(1) + " " + (b/Math.max(a,0.05)).toFixed(1) + " " + r1 + "/" + r2);
}

// ---- promise jobs: N and 4N in flight at once, timed to the last callback
(function () {
  if (typeof Promise === "undefined") return;
  async function w(i) { await null; await i; return i; }
  function once(n, done) {
    var t0 = now(), ps = [];
    for (var i = 0; i < n; i++) ps.push(w(i));
    Promise.all(ps).then(function (v) { done(now() - t0, v.length); });
  }
  if (ONLY && ONLY !== "async-parallel") return;
  once(N / 4, function (a, r1) {
    once(N, function (b, r2) { out("async-parallel " + a.toFixed(1) + " " + b.toFixed(1) + " " + (b / Math.max(a, 0.05)).toFixed(1) + " " + r1 + "/" + r2); });
  });
})();

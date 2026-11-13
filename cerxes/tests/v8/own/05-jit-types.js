function T(n,f){let r;try{r=f();r=typeof r==="string"?JSON.stringify(r):(typeof r==="symbol"?r.toString():String(r))}catch(e){r="throw "+(e&&e.constructor&&e.constructor.name)}console.log(n+": "+r)}
// JIT typer / bounds-check-elimination patterns. These were the core of many V8 in-the-wild bugs:
// Math.expm1(-0) typer (crbug 880207), Array.prototype.indexOf/lastIndexOf range, String.lastIndexOf range,
// x>>>0 / xor sign truncation (CVE-2021-21220, CVE-2021-21224), arguments length typer (CVE-2019-5782),
// Object.is/-0 folding, elements-kind transitions in keyed stores (CVE-2023-3079), map changes after
// warmup (CVE-2020-6418, CVE-2024-4947). Each case is run hot so a JIT compiles the loop, then fed the edge.
function hot(f,warm,edge){let r;for(let i=0;i<3000;i++)r=f(warm(i));return f(edge)}
T("expm1-minus0",()=>{function f(x){return Object.is(Math.expm1(x),-0)}return hot(f,i=>i%3,-0)});
T("expm1-minus0-index",()=>{function f(x){let a=[1,2];return a[1*Object.is(Math.expm1(x),-0)]}return hot(f,i=>0,-0)});
T("minus0-mul",()=>{function f(x){return 1/(x*0)}return hot(f,i=>i+1,-1)});
T("minus0-Math.round",()=>{function f(x){return 1/Math.round(x)}return hot(f,i=>i+0.6,-0.4)});
T("minus0-Math.max",()=>{function f(a,b){return 1/Math.max(a,b)}let r;for(let i=0;i<3000;i++)r=f(i,1);return f(-0,-0)+","+(1/Math.min(0,-0))});
T("NaN-Math.max",()=>{function f(a,b){return Math.max(a,b)}let r;for(let i=0;i<3000;i++)r=f(i,1);return f(NaN,1)+","+f(1,undefined)});
T("int32-overflow-add",()=>{function f(x){return (x+1)|0}return hot(f,i=>i,2147483647)});
T("int32-overflow-noor",()=>{function f(x){return x+1}return hot(f,i=>i,2147483647)});
T("int32-mul-overflow",()=>{function f(x){return x*x}return hot(f,i=>i&255,65536)});
T("int32-neg-min",()=>{function f(x){return -x}return hot(f,i=>i,-2147483648)});
T("int32-div-min",()=>{function f(x){return (x/-1)|0}return hot(f,i=>i+1,-2147483648)+","+((-2147483648/-1))});
T("mod-neg-zero",()=>{function f(x){return 1/(x%5)}return hot(f,i=>i+1,-5)});
T("mod-min-neg1",()=>{function f(x,y){return x%y}let r;for(let i=1;i<3000;i++)r=f(i,7);return f(-2147483648,-1)});
T("ushr-zero",()=>{function f(x){return x>>>0}return hot(f,i=>i,-1)});
T("ushr-as-index",()=>{function f(x){let a=[1,2,3];return a[(x>>>0)%4]}return hot(f,i=>i&1,-1)});
T("xor-sign",()=>{function f(x){return (x^-1)>>>0}return hot(f,i=>i,0)});
T("CVE-2021-21220-shape",()=>{const arr=new Uint32Array([2**31]);function foo(){return (arr[0]^0)+1}let r;for(let i=0;i<3000;i++)r=foo();return r});
T("CVE-2021-21224-shape",()=>{function foo(a){let x=-1;if(a)x=0xFFFFFFFF;let y=Math.max(0,x,-1);return Math.sign(y)}let r;for(let i=0;i<3000;i++)r=foo(i&1);return foo(true)+","+foo(false)});
T("shl-31",()=>{function f(x){return 1<<x}return hot(f,i=>i&15,31)+","+(1<<32)});
T("sar-big",()=>{function f(x){return x>>33}return hot(f,i=>i,-8)});
T("double-to-int-trunc",()=>{function f(x){return x|0}return hot(f,i=>i,2**32+5)+","+hot(f,i=>i,-(2**31)-1)+","+hot(f,i=>i,1e21)+","+hot(f,i=>i,NaN)});
T("indexOf-range",()=>{function f(s){return "abc".indexOf(s,2**30)}return hot(f,i=>"a","")});
T("lastIndexOf-range",()=>{function f(s){return "abc".lastIndexOf(s,-1)+"abc".lastIndexOf("",Infinity)}return hot(f,i=>"c","a")});
T("arr-indexOf-from-neg",()=>{function f(x){return [1,2,3].indexOf(3,x)}return hot(f,i=>0,-(2**53))});
T("string-length-index",()=>{function f(s){let a=new Array(10).fill(1);return a[s.length]}return hot(f,i=>"abc","x".repeat(1e6))});
T("bce-shrink-in-loop",()=>{function f(a,n){let s=0;for(let i=0;i<n;i++){s+=a[i]|0;if(i==5)a.length=2}return s}let r;for(let i=0;i<500;i++)r=f([1,2,3,4,5,6,7,8,9,10],10);return r+","+f([1,2,3,4,5,6,7,8,9,10],10)});
T("bce-neg-index",()=>{function f(a,i){return a[i]}let a=[1,2,3];let r;for(let i=0;i<3000;i++)r=f(a,i%3);return f(a,-1)+","+f(a,-0)+","+f(a,2**32)});
T("bce-hole",()=>{function f(a,i){return a[i]}let a=[1,,3];let r;for(let i=0;i<3000;i++)r=f(a,0);Array.prototype[1]="H";let x=f(a,1);delete Array.prototype[1];return x+","+f(a,1)});
T("elements-kind-transition",()=>{function f(a,v){a[0]=v;return a[0]}let a=[1,2,3];let r;for(let i=0;i<3000;i++)r=f(a,i);r=f(a,1.5);r+=","+f(a,"s");r+=","+f(a,{})+","+a.length;return r});
T("elements-kind-double-hole",()=>{function f(a){return a[1]}let a=[1.5,,3.5];let r;for(let i=0;i<3000;i++)r=f(a);return String(r)});
T("hole-nan-pattern",()=>{let a=[1.1,2.2];let f=new Float64Array(1);let u=new Uint32Array(f.buffer);u[0]=0xFFF7FFFF;u[1]=0xFFF7FFFF;a[0]=f[0];a.length=3;return Number.isNaN(a[0])+","+(1 in a)+","+(2 in a)+","+a[2]});
T("keyed-store-proto-setter",()=>{function f(a,i,v){a[i]=v}let a=[];for(let i=0;i<3000;i++)f(a,0,i);let s=0;Object.defineProperty(Array.prototype,"5",{set(v){s=v},configurable:true});f(a,5,9);delete Array.prototype[5];return s+","+a.length});
T("map-change-after-warm",()=>{function f(o){return o.x}let o={x:1};let r;for(let i=0;i<3000;i++)r=f(o);Object.defineProperty(o,"x",{get(){return 2}});return f(o)});
T("proto-change-after-warm",()=>{function f(o){return o.y}let p={y:1};let o=Object.create(p);for(let i=0;i<3000;i++)f(o);p.y=5;let a=f(o);Object.setPrototypeOf(o,{get y(){return 9}});return a+","+f(o)});
T("delete-after-warm",()=>{function f(o){return o.b}let o={a:1,b:2,c:3};for(let i=0;i<3000;i++)f(o);delete o.b;return f(o)});
T("dictionary-mode",()=>{function f(o){return o.k500}let o={};for(let i=0;i<1000;i++)o["k"+i]=i;for(let i=0;i<3000;i++)f(o);delete o.k1;return f(o)});
T("arguments-length",()=>{function g(){return arguments.length}function f(n){let a=[];a.length=n;return g.apply(null,a)}let r;for(let i=0;i<3000;i++)r=f(i&7);return f(65535)});
T("arguments-mapped",()=>{function f(a){arguments[0]=5;return a}let r;for(let i=0;i<3000;i++)r=f(i);return r});
T("arguments-mapped-delete",()=>{function f(a){delete arguments[0];arguments[0]=5;return a}return hot(f,i=>i,1)});
T("arguments-strict",()=>{function f(a){"use strict";arguments[0]=5;return a}return hot(f,i=>i,1)});
T("rest-mutation",()=>{function f(...a){a.length=0;return a[0]}return hot(f,i=>i,1)});
T("closure-type-change",()=>{let v=1;function f(){return v+1}for(let i=0;i<3000;i++)f();v="s";return f()});
T("const-fold-typeof",()=>{function f(x){return typeof x==="number"?x+1:"no"}return hot(f,i=>i,1n)});
T("bigint-asUintN",()=>{function f(x){return BigInt.asUintN(64,x)}return String(hot(f,i=>BigInt(i),-1n))});
T("bigint-asIntN",()=>{function f(x){return BigInt.asIntN(1,x)}return String(hot(f,i=>BigInt(i),1n))});
T("bigint-shift-huge",()=>1n<<(2n**40n));
T("bigint-pow-huge",()=>2n**(2n**40n));
T("string-concat-loop-type",()=>{function f(a,b){return a+b}let r;for(let i=0;i<3000;i++)r=f(i,1);return f("1",1)+","+f(1,{valueOf(){return 3}})});
T("compare-mixed",()=>{function f(a,b){return a<b}let r;for(let i=0;i<3000;i++)r=f(i,5);return f("10","9")+","+f(1,NaN)+","+f(1n,2)+","+f(null,1)});
T("switch-mixed",()=>{function f(x){switch(x){case 1:return "a";case "1":return "b";default:return "c"}}return hot(f,i=>1,"1")});
T("osr-type-change",()=>{let a=[];for(let i=0;i<100000;i++){a[i&7]=i==99990?"s":i}return a.join()});
T("loop-var-overflow",()=>{let i=2147483640,n=0;for(;i<2147483650;i++)n++;return n+","+i});
T("loop-double-counter",()=>{let n=0;for(let i=0;i<10;i+=0.1)n++;return n});
T("instanceof-change",()=>{class A{};function f(o){return o instanceof A}let a=new A;for(let i=0;i<3000;i++)f(a);Object.defineProperty(A,Symbol.hasInstance,{value:()=>false});return f(a)});
T("escape-analysis-shape",()=>{function f(x){let o={a:x};if(x>2990)return o;return o.a}let r;for(let i=0;i<3000;i++)r=f(i);return JSON.stringify(r)});
T("try-catch-deopt",()=>{function f(x){try{if(x>2995)throw x;return x}catch(e){return -e}}let s=0;for(let i=0;i<3000;i++)s+=f(i);return s});
T("Math.pow-edge",()=>Math.pow(-0,-1)+","+Math.pow(NaN,0)+","+Math.pow(1,Infinity)+","+(-8)**(1/3));
T("Math.imul-clz",()=>Math.imul(0xffffffff,5)+","+Math.clz32(0)+","+Math.clz32(-1)+","+Math.fround(2**128));
T("Math.atan2-hypot",()=>Math.hypot(Infinity,NaN)+","+Math.hypot()+","+Math.atan2(-0,-0));
T("String.fromCharCode-big",()=>String.fromCharCode(65536+65,-1).length);
T("charCodeAt-oob",()=>{function f(s,i){return s.charCodeAt(i)}let r;for(let i=0;i<3000;i++)r=f("abc",i%3);return f("abc",3)+","+f("abc",-1)+","+f("abc",2**32)});
T("at-oob-string",()=>{function f(s,i){return s.at(i)}for(let i=0;i<3000;i++)f("abc",i%3);return String(f("abc",-4))});
T("new.target-reflect",()=>{function F(){return new.target===F}return Reflect.construct(F,[],Object)});
T("super-home-object",()=>{let o={m(){return super.x}};Object.setPrototypeOf(o,{x:1});let r=o.m();Object.setPrototypeOf(o,{x:2});return r+","+o.m()});

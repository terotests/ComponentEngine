function T(n,f){let r;try{r=f();r=typeof r==="string"?JSON.stringify(r):(typeof r==="symbol"?r.toString():String(r))}catch(e){r="throw "+(e&&e.constructor&&e.constructor.name)}console.log(n+": "+r)}
// Native recursion depth: V8 throws a catchable RangeError for all of these. An engine that recurses on
// the native stack without a limit aborts the whole host instead (DoS). Known open issue in CErXes
// (deep recursion, fuzz --heavy); kept here as the regression set for when the limit lands.
T("plain-recursion",()=>{function f(){return f()}f()});
T("toString-recursion",()=>{let o={toString(){return String(o)}};String(o)});
T("getter-recursion",()=>{let o={get x(){return o.x}};o.x});
T("proxy-get-recursion",()=>{let p=new Proxy({},{get(t,k){return p[k]}});p.x});
T("valueOf-recursion",()=>{let o={valueOf(){return o+1}};o+1});
T("toJSON-recursion",()=>{let o={toJSON(){return JSON.stringify(o)}};JSON.stringify(o)});
T("sort-recursion",()=>{let a=[2,1];function c(x,y){a.sort(c);return x-y}a.sort(c)});
T("replace-recursion",()=>{function r(){return "a".replace(/a/,r)}r()});
T("apply-recursion",()=>{function f(){return f.apply(null,[])}f()});
T("bind-recursion",()=>{let f=function(){return g()};let g=f.bind(null);g()});
T("constructor-recursion",()=>{class A{constructor(){new A()}};new A()});
T("Reflect-recursion",()=>{function f(){return Reflect.apply(f,null,[])}f()});
T("iterator-recursion",()=>{let it={[Symbol.iterator](){return [...it][Symbol.iterator]()}};[...it]});
T("then-recursion",()=>{let o={get then(){return Promise.resolve(o)}};return "scheduled"});
T("setter-recursion",()=>{let o={set x(v){o.x=v}};o.x=1});
T("instanceof-recursion",()=>{let C={[Symbol.hasInstance](v){return v instanceof C}};({}) instanceof C});
T("eval-recursion",()=>{function f(){eval("f()")}f()});
T("Function-ctor-recursion",()=>{let f=new Function("f","return f(f)");f(f)});
T("generator-recursion",()=>{function* g(){yield* g()}[...g()]});
T("deep-object-structuredClone",()=>{let o={};for(let i=0;i<100000;i++)o={o};structuredClone(o)});
T("deep-proto-chain-lookup",()=>{let o={};for(let i=0;i<100000;i++)o=Object.create(o);return o.missing});
T("deep-array-flat",()=>{let a=[1];for(let i=0;i<100000;i++)a=[a];return a.flat(Infinity).length});
T("deep-array-toString",()=>{let a=[1];for(let i=0;i<100000;i++)a=[a];return String(a).length});
T("deep-array-join",()=>{let a=[1];for(let i=0;i<100000;i++)a=[a];return a.join().length});
T("after-all",()=>"still alive");

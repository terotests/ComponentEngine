function T(n,f){let r;try{r=f();r=typeof r==="string"?JSON.stringify(r):(typeof r==="symbol"?r.toString():String(r))}catch(e){r="throw "+(e&&e.constructor&&e.constructor.name)}console.log(n+": "+r)}
// CErXes-specific: state that must survive a snapshot taken in the middle of it (the `snap` configuration
// snapshots and restores every 0.05 ms). V8 has no such feature; its nearest class is snapshot/deserializer
// bugs (code cache, startup snapshot). Each test spins long enough to be cut many times.
const spin=(n)=>{let z=0;for(let s=0;s<n;s++)z+=s;return z};
T("forin-after-delete",()=>{let r=0;for(let j=1;j<100;j++){let a={};for(let i=0;i<j;i++)a["a_"+i]=i+1;for(let i=0;i<j;i+=3)delete a["a_"+i];for(let k in a){spin(50);r++}}return r});
T("forin-shape-change-mid-loop",()=>{let o={};for(let i=0;i<40;i++)o["k"+i]=i;let r=[];for(let k in o){spin(2000);if(k==="k5")delete o.k30;if(k==="k10")o.extra=1;r.push(k)}return r.length});
T("forin-proto-keys",()=>{let p={a:1,b:2};let o=Object.create(p);o.c=3;let r=[];for(let i=0;i<200;i++)for(let k in o){spin(100);r.push(k)}return r.slice(0,3).join()+r.length});
T("generator-suspended",()=>{function* g(){for(let i=0;i<300;i++){spin(500);yield i}}let s=0;for(const v of g())s+=v;return s});
T("map-iterator-mid",()=>{let m=new Map();for(let i=0;i<300;i++)m.set("k"+i,i);let s=0;for(const [k,v] of m){spin(500);if(v==10)m.delete("k200");s+=v}return s});
T("set-iterator-mid",()=>{let m=new Set();for(let i=0;i<300;i++)m.add(i);let s=0;let it=m.values();let r;while(!(r=it.next()).done){spin(500);s+=r.value}return s});
T("array-iterator-mid",()=>{let a=Array.from({length:300},(x,i)=>i);let s=0;for(const v of a){spin(500);s+=v}return s});
T("regexp-lastIndex-mid",()=>{let re=/a/g;let s="ab".repeat(300);let n=0;while(re.exec(s)){spin(300);n+=re.lastIndex}return n+":"+re.source+":"+re.flags});
T("sort-comparator-mid",()=>{let a=Array.from({length:300},(x,i)=>(i*7919)%300);a.sort((x,y)=>{spin(50);return x-y});return a.slice(0,5).join()+a[299]});
T("closure-cells",()=>{let fs=[];for(let i=0;i<200;i++){let c=i;fs.push(()=>c++)}let s=0;for(const f of fs){spin(500);s+=f()+f()}return s});
T("typedarray-shared-buffer",()=>{let b=new ArrayBuffer(64);let u8=new Uint8Array(b);let u32=new Uint32Array(b,4,4);for(let i=0;i<200;i++){spin(500);u32[i&3]+=1}return u8.slice(4,8).join()+":"+u32.join()});
T("proxy-handler-state",()=>{let n=0;let p=new Proxy({},{get(t,k){n++;return k}});let s="";for(let i=0;i<200;i++){spin(500);s=p["x"+i]}return s+n});
T("weakmap-entries",()=>{let w=new WeakMap();let ks=[];for(let i=0;i<200;i++){let k={i};ks.push(k);w.set(k,i);spin(300)}return ks.reduce((s,k)=>s+w.get(k),0)});
T("string-builder",()=>{let s="";for(let i=0;i<300;i++){spin(300);s+=String.fromCharCode(65+i%26)}return s.length+s.slice(-3)});
T("symbol-identity",()=>{let a=Symbol("a");let o={[a]:1};let r=0;for(let i=0;i<300;i++){spin(300);r+=o[a]}return r+":"+(o[a]===1)+":"+String(a)});
T("bigint-values",()=>{let b=1n;for(let i=0;i<200;i++){spin(300);b=b*3n+1n}return String(b%1000000007n)});
T("date-and-math",()=>{let d=new Date(Date.UTC(2020,0,1));for(let i=0;i<200;i++){spin(300);d.setUTCDate(d.getUTCDate()+1)}return d.toISOString()});
T("try-finally-mid",()=>{let log=[];for(let i=0;i<100;i++){try{spin(500);if(i%10==0)throw i}catch(e){log.push(e)}finally{spin(100)}}return log.join()});
T("labelled-loops-mid",()=>{let n=0;outer:for(let i=0;i<30;i++){for(let j=0;j<30;j++){spin(50);if(j==i)continue outer;n++}}return n});
T("bound-and-class",()=>{class A{#x=1;inc(){return ++this.#x}};let a=new A();let f=a.inc.bind(a);let r=0;for(let i=0;i<200;i++){spin(300);r=f()}return r});

function T(n,f){let r;try{r=f();r=typeof r==="string"?JSON.stringify(r):(typeof r==="symbol"?r.toString():String(r))}catch(e){r="throw "+(e&&e.constructor&&e.constructor.name)}console.log(n+": "+r)}
// TypedArray / ArrayBuffer: detach or shrink during valueOf / callbacks. In V8 the classic OOB class
// (e.g. CVE-2016-5129-like, TypedArray.prototype.fill/sort/set with detached buffer; resizable buffers 2023+).
const canTransfer=typeof ArrayBuffer.prototype.transfer==="function";
const detach=(b)=>{if(canTransfer)b.transfer();else structuredClone(b,{transfer:[b]})};
T("has-transfer",()=>canTransfer);
T("ctor-offset-valueOf-detach",()=>{let b=new ArrayBuffer(16);return new Uint8Array(b,{valueOf(){detach(b);return 0}}).length});
T("fill-valueOf-detach",()=>{let t=new Uint8Array(16);t.fill({valueOf(){detach(t.buffer);return 1}});return t.length});
T("fill-start-detach",()=>{let t=new Uint8Array(16);t.fill(1,{valueOf(){detach(t.buffer);return 0}});return t.length});
T("set-array-valueOf-detach",()=>{let t=new Uint8Array(16);t.set([{valueOf(){detach(t.buffer);return 1}},2,3]);return t.length});
T("sort-cmp-detach",()=>{let t=new Float64Array([3,2,1,0,5,4]);t.sort((a,b)=>{detach(t.buffer);return a-b});return t.length});
T("indexOf-from-detach",()=>{let t=new Uint8Array(16);return t.indexOf(0,{valueOf(){detach(t.buffer);return 0}})});
T("includes-from-detach",()=>{let t=new Uint8Array(16);return t.includes(undefined,{valueOf(){detach(t.buffer);return 0}})});
T("lastIndexOf-from-detach",()=>{let t=new Uint8Array(16);return t.lastIndexOf(0,{valueOf(){detach(t.buffer);return 15}})});
T("join-sep-detach",()=>{let t=new Uint8Array(4);return t.join({toString(){detach(t.buffer);return "-"}})});
T("copyWithin-detach",()=>{let t=new Uint8Array(16);t.copyWithin(0,{valueOf(){detach(t.buffer);return 1}});return t.length});
T("slice-species-small",()=>{let t=new Uint8Array([1,2,3,4,5,6,7,8]);t.constructor={[Symbol.species]:function(n){return new Uint8Array(1)}};return t.slice(0)});
T("slice-detach",()=>{let t=new Uint8Array(8);return t.slice({valueOf(){detach(t.buffer);return 0}}).length});
T("subarray-detach",()=>{let t=new Uint8Array(8);return t.subarray({valueOf(){detach(t.buffer);return 0}}).length});
T("map-cb-detach",()=>{let t=new Uint8Array(8);return t.map((x,i)=>{if(i==0)detach(t.buffer);return 1}).length});
T("from-iter-big",()=>Uint8Array.from({length:5},(x,i)=>i*300).join());
T("DataView-detach-get",()=>{let b=new ArrayBuffer(8);let d=new DataView(b);detach(b);return d.getUint8(0)});
T("DataView-offset-valueOf-detach",()=>{let b=new ArrayBuffer(8);let d=new DataView(b);return d.getUint32({valueOf(){detach(b);return 0}})});
T("DataView-oob",()=>new DataView(new ArrayBuffer(4)).getUint32(1));
T("DataView-neg",()=>new DataView(new ArrayBuffer(4)).getUint8(-1));
T("DataView-big-offset",()=>new DataView(new ArrayBuffer(4),5));
T("TA-misaligned",()=>new Uint32Array(new ArrayBuffer(8),1));
T("TA-bad-length",()=>new Uint32Array(new ArrayBuffer(7)));
T("TA-huge",()=>new Uint8Array(2**53));
T("TA-index-oob-write",()=>{let t=new Uint8Array(2);t[5]=1;t[-1]=1;t["1.5"]=1;return Object.keys(t).join()});
T("TA-canonical-numeric",()=>{let t=new Uint8Array(2);t["-0"]=1;return t["-0"]+":"+Object.keys(t).length});
T("TA-proto-chain-oob",()=>{let t=new Uint8Array(2);Object.prototype[5]="P";let r=t[5];delete Object.prototype[5];return r});
T("TA-detached-length",()=>{let t=new Uint8Array(8);detach(t.buffer);return t.length+","+t.byteLength+","+t[0]});
T("TA-detached-iter",()=>{let t=new Uint8Array(8);let it=t.values();it.next();detach(t.buffer);it.next()});
T("TA-set-overlap",()=>{let b=new ArrayBuffer(8);let a=new Uint8Array(b);for(let i=0;i<8;i++)a[i]=i;let c=new Uint16Array(b,0,2);a.set(c,2);return a.join()});
T("TA-set-overlap2",()=>{let b=new ArrayBuffer(16);let a=new Uint32Array(b);a.set([1,2,3,4]);new Uint8Array(b).set(a,1);return new Uint8Array(b).join()});
T("TA-set-neg-offset",()=>new Uint8Array(4).set([1],-1));
T("TA-set-overflow",()=>new Uint8Array(4).set([1,2],3));
T("float-nan-canon",()=>{let f=new Float64Array(1);let u=new Uint8Array(f.buffer);f[0]=NaN;return Number.isNaN(f[0])});
T("Uint8Clamped",()=>{let t=new Uint8ClampedArray(4);t[0]=300;t[1]=-5;t[2]=1.5;t[3]=2.5;return t.join()});
T("BigInt64-mix",()=>{let t=new BigInt64Array(1);t[0]=1});
T("BigInt64-wrap",()=>{let t=new BigInt64Array(1);t[0]=2n**63n;return String(t[0])});
let rab=null;try{rab=new ArrayBuffer(16,{maxByteLength:64})}catch(e){}
T("has-resizable",()=>!!(rab&&rab.resizable));
if(rab&&rab.resizable){
T("rab-shrink-fill",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let t=new Uint8Array(b);t.fill(1,0,{valueOf(){b.resize(2);return 16}});return t.length});
T("rab-shrink-set",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let t=new Uint8Array(b);t.set([1,{valueOf(){b.resize(0);return 2}},3]);return t.length});
T("rab-shrink-sort",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let t=new Uint8Array(b);for(let i=0;i<16;i++)t[i]=16-i;t.sort((x,y)=>{b.resize(4);return x-y});return t.length});
T("rab-fixed-view-oob",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let t=new Uint8Array(b,8,8);b.resize(10);return t.length+","+t.byteOffset+","+t[0]});
T("rab-tracking-grow",()=>{let b=new ArrayBuffer(4,{maxByteLength:64});let t=new Uint8Array(b);b.resize(32);t[31]=1;return t.length});
T("rab-resize-over-max",()=>new ArrayBuffer(4,{maxByteLength:8}).resize(9));
T("rab-copyWithin-shrink",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let t=new Uint8Array(b);t.copyWithin(0,{valueOf(){b.resize(1);return 8}});return t.length});
T("rab-dataview-shrink",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let d=new DataView(b,8);b.resize(9);return d.getUint8(0)});
T("rab-subarray-shrink",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let t=new Uint8Array(b);return t.subarray({valueOf(){b.resize(0);return 0}},8).length});
T("rab-slice-shrink",()=>{let b=new ArrayBuffer(16,{maxByteLength:64});let t=new Uint8Array(b);return t.slice(0,{valueOf(){b.resize(2);return 16}}).length});
}
T("ab-slice-species-small",()=>{let b=new ArrayBuffer(16);b.constructor={[Symbol.species]:function(n){return new ArrayBuffer(1)}};return b.slice(0,8).byteLength});
T("ab-slice-species-same",()=>{let b=new ArrayBuffer(16);b.constructor={[Symbol.species]:function(n){return b}};return b.slice(0,8).byteLength});
T("ab-huge",()=>new ArrayBuffer(2**53));
T("Atomics-oob",()=>Atomics.add(new Int32Array(2),5,1));

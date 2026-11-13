import beautify from "js-beautify";
import { hash } from "../runtime.js";
const js = `function f(a,b){if(a){return b?[1,2,{x:1,y:function(){return a+b}}]:null}else{for(var i=0;i<10;i++){a+=i}}switch(a){case 1:break;default:a=2}return a}var o={a:1,'b':[1,2,3],c:class X extends Y{m(){super.m()}}};`;
const css = `a{color:red;margin:0 1px}@media(min-width:10px){.b>c:hover{x:y}}`;
const html = `<div><p>a<b>b</b></p><ul><li>1</li><li>2</li></ul><script>var a=1;</script><style>a{b:c}</style></div>`;
const big = js.repeat(60);
export default {
  check: {
    js: () => beautify.js(js, { indent_size: 2 }),
    css: () => beautify.css(css),
    html: () => beautify.html(html),
    bigHash: () => hash(beautify.js(big)),
  },
  bench: { jsBig: () => beautify.js(big).length, html: () => beautify.html(html.repeat(30)).length },
};

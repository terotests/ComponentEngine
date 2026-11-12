import * as prettier from "prettier/standalone";
import * as babel from "prettier/plugins/babel";
import * as estree from "prettier/plugins/estree";
import * as postcss from "prettier/plugins/postcss";
import * as markdown from "prettier/plugins/markdown";
import * as ts from "prettier/plugins/typescript";
const plugins = [babel, estree, postcss, markdown, ts];
const js = `const a={b:1,c:[1,2,3].map(x=>x*2),d:async function*(){yield await foo?.bar??baz}};class K extends L{#p=1;static{init()} get q(){return this.#p}}
if(a){b()}else if(c)d();else{e()} const longCall=someFunction(argumentNumberOne,argumentNumberTwo,argumentNumberThree,argumentNumberFour)
export default ()=><div className="x">{list.map(i=><Item key={i} {...i}/>)}</div>`;
const css = `a{color:red;background:URL(x.png)} @media (max-width:100px){.b>.c+.d{margin:0 0 0 0;grid-template-areas:"a b" "c d"}}`;
const md = `# T\n* a\n* b\n\n|x|y|\n|-|-|\n|1|22|\n\nSome *em* text that is long enough to wrap maybe if prose wrap is always set to on here ok.`;
const tsx = `interface A<T>{a:T;b?:string[]} type U=keyof A<number>|\`x\${string}\`; enum E{A=1,B} function f<T,>(x:T):x is T{return true} const c=<T,>(x:T)=>x as unknown as T`;
const big = Array.from({ length: 10 }, () => js).join("\n");
export default {
  check: {
    js: () => prettier.format(js, { parser: "babel", plugins }),
    css: () => prettier.format(css, { parser: "css", plugins }),
    markdown: () => prettier.format(md, { parser: "markdown", proseWrap: "always", printWidth: 40, plugins }),
    typescript: () => prettier.format(tsx, { parser: "typescript", plugins }),
  },
  bench: {
    js: () => prettier.format(big, { parser: "babel", plugins }).then((s) => s.length),
    typescript: () => prettier.format(tsx, { parser: "typescript", plugins }).then((s) => s.length),
  },
};

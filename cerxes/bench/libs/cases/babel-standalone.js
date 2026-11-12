import * as Babel from "@babel/standalone";
const src = `
class A { #x = 1; static y = 2; get x() { return this.#x; } async *g(xs) { for await (const v of xs) yield v?.k ?? 0; } }
const f = async ({ a, ...r } = {}, ...xs) => { const [p = 1, , q] = xs; await null; return { ...r, a: a ** 2, p, q, t: \`\${a}-\${p}\` }; };
let o = { a: 1 }; o.a ||= 2; o.b ??= 3;
const App = (props) => <div className="x" {...props}><span>{props.n}</span></div>;
`;
const ts = `enum E { A, B = 5 } namespace N { export const v: number = E.B; } export function id<T>(x: T): T { return x; } const c = <T,>(x: T) => x as any;`;
export default {
  check: {
    presetEnv: () => Babel.transform(src, { presets: [["env", { targets: { ie: 11 } }], "react"], compact: false, configFile: false, babelrc: false }).code,
    typescript: () => Babel.transform(ts, { presets: ["typescript"], filename: "a.ts", configFile: false, babelrc: false }).code,
    minimalReact: () => Babel.transform("const x = <A b='1'>{c}</A>;", { presets: [["react", { runtime: "automatic" }]], configFile: false, babelrc: false }).code,
  },
  bench: {
    presetEnv: () => Babel.transform(src, { presets: [["env", { targets: { ie: 11 } }], "react"], configFile: false, babelrc: false }).code.length,
    typescript: () => Babel.transform(ts, { presets: ["typescript"], filename: "a.ts", configFile: false, babelrc: false }).code.length,
  },
};

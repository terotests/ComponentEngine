import ts from "typescript";
const src = `
interface Shape { kind: "circle" | "square"; size: number }
enum Color { Red, Green = 5, Blue }
namespace Geo { export const PI2 = Math.PI * 2; export function area(s: Shape): number { return s.kind === "circle" ? Math.PI * s.size ** 2 : s.size ** 2; } }
abstract class Base<T> { protected constructor(public readonly id: T) {} abstract describe(): string }
class Item extends Base<number> { @logged method() {} constructor(id: number, private name?: string) { super(id); } describe() { return \`\${this.id}:\${this.name ?? "?"}\`; } }
function logged(t: any, k?: any) {}
export async function* gen<T extends object>(xs: T[]): AsyncGenerator<T> { for (const x of xs) yield x; }
const tuple: [string, number?] = ["a"]; const sat = { a: 1 } satisfies Record<string, number>;
let x = <const>["a", "b"]; type DeepPartial<T> = { [K in keyof T]?: DeepPartial<T[K]> };
`;
const bad = `let n: number = "str"; function f(a: string): number { return a; } f(1); const o: { a: number } = { a: 1, b: 2 };`;
function check(code) {
  const host = ts.createCompilerHost({}); const file = ts.createSourceFile("a.ts", code, ts.ScriptTarget.ES2022, true);
  const libFile = ts.createSourceFile("lib.d.ts", "interface Array<T>{length:number;[n:number]:T} interface String{} interface Number{} interface Boolean{} interface Function{} interface Object{} interface RegExp{} interface IArguments{} interface CallableFunction{} interface NewableFunction{} type Record<K extends keyof any, T> = { [P in K]: T };", ts.ScriptTarget.ES2022);
  host.getSourceFile = (n) => (n === "a.ts" ? file : n.endsWith("lib.d.ts") ? libFile : undefined);
  host.getDefaultLibFileName = () => "lib.d.ts"; host.fileExists = (n) => n === "a.ts" || n.endsWith("lib.d.ts"); host.readFile = () => ""; host.writeFile = () => {};
  const p = ts.createProgram(["a.ts"], { noEmit: true, strict: true, target: ts.ScriptTarget.ES2022 }, host);
  return ts.getPreEmitDiagnostics(p).map((d) => d.code + ":" + ts.flattenDiagnosticMessageText(d.messageText, " "));
}
export default {
  check: {
    transpileES5: () => ts.transpileModule(src, { compilerOptions: { target: ts.ScriptTarget.ES5, module: ts.ModuleKind.CommonJS, experimentalDecorators: true, downlevelIteration: true } }).outputText,
    transpileESNext: () => ts.transpileModule(src, { compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext, experimentalDecorators: true } }).outputText,
    typeErrors: () => check(bad),
    printer: () => { const f = ts.createSourceFile("b.ts", "const  a=1;function   g( x ){return x+a}", ts.ScriptTarget.Latest); return ts.createPrinter().printFile(f); },
  },
  bench: {
    transpile: () => ts.transpileModule(src, { compilerOptions: { target: ts.ScriptTarget.ES2017, experimentalDecorators: true } }).outputText.length,
    typeCheck: () => check(bad + src.replace("export ", "")).length,
  },
};

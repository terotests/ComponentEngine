// CErXes: TypeScript and JSX read into CEr's tree. Each case runs a script
// in a fresh engine and compares the value of its last expression.
use cerxes::Engine;

fn run(src: &str) -> String {
    let mut e = Engine::new();
    e.eval(src)
}

fn run_as(src: &str, typescript: bool, jsx: bool) -> String {
    let mut e = Engine::new();
    e.set_syntax(typescript, jsx);
    e.eval(src)
}

#[test]
fn annotations_are_erased() {
    assert_eq!(run("let a: number = 1; const b: Array<Map<string, number[]>> = []; var c!: string; a + b.length"), "1");
    assert_eq!(run("function f(a: number, b?: string, ...r: number[]): string { return a + (b ?? '') + r.length; } f(1, 'x', 3, 4)"), "1x2");
    assert_eq!(run("const f = (x: number, y = 2): number => x * y; f(3)"), "6");
    assert_eq!(run("const f = async (x: number): Promise<number> => x; typeof f"), "function");
    assert_eq!(run("const g = ({ a, b }: { a: number; b: number }): number => a + b; g({ a: 1, b: 2 })"), "3");
    assert_eq!(run("try { throw 1 } catch (e: unknown) { String(e) }"), "1");
    assert_eq!(run("let t: [string, number?, ...boolean[]] = ['a']; let u: (a: string) => void = () => {}; let k: keyof typeof t = 0; t[k]"), "a");
    assert_eq!(run("type F = <T>(x: T) => T extends string ? 'yes' : 'no'; let o: { readonly [k: string]: number } = { x: 1 }; o.x"), "1");
    assert_eq!(run("function isStr(x: unknown): x is string { return typeof x === 'string'; } isStr('a')"), "true");
    assert_eq!(run("function check(x: unknown): asserts x is number {} check(1); 'ok'"), "ok");
    assert_eq!(run("function f(this: Window, a: number) { return a; } f(4)"), "4");
    assert_eq!(run("let x: `id-${number}` | -1 | typeof globalThis | import('x').Y = -1; x"), "-1");
}

#[test]
fn expressions() {
    assert_eq!(run("const x = (1 as any as number) + (2 satisfies number); x"), "3");
    assert_eq!(run("const o = { a: [1, 2] } as const; o.a.length"), "2");
    assert_eq!(run("const m: { v?: { w: number } } = { v: { w: 5 } }; m.v!.w + m!.v!.w"), "10");
    assert_eq!(run("function id<T>(x: T): T { return x; } id<number>(7) + id<Array<string>>(['a']).length"), "8");
    assert_eq!(run("const m = new Map<string, number>([['a', 1]]); m.get('a')"), "1");
    // comparisons stay comparisons (`a < b > (c)` is a generic call, as in tsc)
    assert_eq!(run("const a = 1, b = 2, c = 3; [a < b, b > c, a < b && c > (b as any)].join()"), "true,false,true");
    assert_eq!(run("let i = 0; for (let j = 0; j < 3; j++) { i += j; } i >> 1"), "1");
    assert_eq!(run("const n: Array<Array<number>>= [[1]]; n[0][0]"), "1");
    assert_eq!(run("const c = true; const r = c ? (1) : (2); r"), "1");
    assert_eq!(run("const f = <T, U extends T = T>(a: T, b: U): [T, U] => [a, b]; f(1, 2).join()"), "1,2");
    assert_eq!(run("const tag = (s: TemplateStringsArray) => s[0]; tag<string>`x`"), "x");
}

#[test]
fn type_assertion_in_plain_typescript() {
    assert_eq!(run_as("const x = <number>(<any>'5' * 2); x", true, false), "10");
    assert_eq!(run_as("const f = <T>(a: T) => a; f(3)", true, false), "3");
}

#[test]
fn declarations_are_erased() {
    let src = "
        interface A<T> extends B, C<T> { x: T; f(): void }
        type U<T> = T | { a: T[] };
        declare const d: number;
        declare function df(x: string): void;
        declare module 'm' { export const y: number; }
        declare global { interface Window { z: 1 } }
        declare class DC { m(): void }
        declare enum DE { A }
        import type { Q } from './q';
        import type R from './r';
        import { type S, type T } from './s';
        export type { A };
        export interface E {}
        typeof DC + typeof DE + typeof df";
    assert_eq!(run(src), "undefinedundefinedundefined");
    // `type` and `interface` stay usable as names
    assert_eq!(run("const type = 1, interface_ = 2; let declare = 3; type + interface_ + declare"), "6");
}

#[test]
fn value_imports_need_modules() {
    assert!(run("import x from 'y'; x").contains("modules are not supported"));
}

#[test]
fn exports_are_dropped() {
    assert_eq!(run("export const a = 1; export function f() { return 2 } export default class K {} export { a as b }; a + f() + typeof K"), "3function");
}

#[test]
fn enums() {
    let src = "
        enum Color { Red, Green = 5, Blue, 'Dark Grey' }
        enum Dir { Up = 'UP', Down = 'DOWN' }
        enum Flags { None = 0, A = 1 << 0, B = 1 << 1, AB = A | B }
        const enum K { X = 2 }
        [Color.Red, Color.Green, Color.Blue, Color[6], Color['Dark Grey'], Dir.Up, Dir.UP, Flags.AB, K.X].join()";
    assert_eq!(run(src), "0,5,6,Blue,7,UP,,3,2");
}

#[test]
fn namespaces() {
    let src = "
        namespace NS { export const a = 1; const hidden = 2; export function f() { return a + hidden; } export enum E { X } }
        namespace NS { export const b = 10; }
        namespace A.B.C { export const deep = 'd'; }
        [NS.a, NS.f(), NS.b, (NS as any).hidden, NS.E.X, A.B.C.deep].join()";
    assert_eq!(run(src), "1,3,10,,0,d");
    assert_eq!(run("namespace N { export namespace M { export let x = 5 } } import Y = N.M; Y.x"), "5");
}

#[test]
fn classes() {
    let src = "
        abstract class Shape<T = {}> implements Named {
            static count: number = 0;
            private readonly id: number;
            declare tag: string;
            [key: string]: any;
            protected constructor(public readonly name: string, private sides?: number) { this.id = ++Shape.count; }
            abstract area(): number;
            describe(): string { return `${this.name}#${this.id}:${this.area()}`; }
            get label(): string { return this.name; }
            opt?(): void;
        }
        class Sq extends Shape<{}> {
            override area(): number { return this.s * this.s; }
            constructor(private s: number) { super('sq', 4); }
            static of<T>(n: number): Sq { return new Sq(n); }
        }
        const q = Sq.of<number>(3);
        [q.describe(), q.label, q.s, 'tag' in q, 'opt' in q, q.sides].join()";
    assert_eq!(run(src), "sq#1:9,sq,3,false,false,4");
    // overloads of a method and of the constructor
    let src2 = "
        class P {
            constructor(a: string);
            constructor(a: number);
            constructor(public a: any) {}
            m(x: string): string;
            m(x: any) { return typeof x; }
        }
        new P(1).a + new P(1).m(2)";
    assert_eq!(run(src2), "1number");
    assert!(run("class D { @dec m() {} }").contains("decorators are not supported"));
}

#[test]
fn function_overloads() {
    assert_eq!(run("function f(a: string): string; function f(a: number): number; function f(a: any): any { return a; } f(2)"), "2");
}

#[test]
fn jsx_elements() {
    let r = |s: &str| run(&format!("renderToString({})", s));
    assert_eq!(r("<div id=\"a\" data-x='1' hidden>hi</div>"), "<div id=\"a\" data-x=\"1\" hidden>hi</div>");
    assert_eq!(r("<br />"), "<br />");
    assert_eq!(r("<p className=\"c\" style={{ marginTop: 4 }}>{1 + 1} &amp; &#65;&#x42; &copy;</p>"), "<p class=\"c\" style=\"margin-top:4\">2 &amp; AB ©</p>");
    assert_eq!(r("<ul>{[1, 2].map(i => <li key={i}>{i}</li>)}</ul>"), "<ul><li>1</li><li>2</li></ul>");
    assert_eq!(r("<svg:rect xlink:href=\"#a\" />"), "<svg:rect xlink:href=\"#a\"></svg:rect>");
    assert_eq!(r("<my-el a={null} b={false}>{null}{true}{undefined}{0}</my-el>"), "<my-el>0</my-el>");
    // whitespace: lines trimmed, blank lines dropped, joined by one space
    assert_eq!(r("<p>\n   it's   a\n\n   long   <b>line</b>\n   </p>"), "<p>it's   a long   <b>line</b></p>");
    assert_eq!(r("<p>{/* nothing */}  x  </p>"), "<p>  x  </p>");
    assert_eq!(r("<div {...{ id: 'x', title: 't' }} title=\"u\">{...['a', 'b']}</div>"), "<div id=\"x\" title=\"u\">ab</div>");
    assert_eq!(r("<a b=<i>x</i> />").contains("<a"), true);
    assert_eq!(run("const e = <div a=\"1\">x{2}</div>; [e.type, e.props.a, e.children.length, e.children[1]].join()"), "div,1,2,2");
}

#[test]
fn jsx_components() {
    let src = "
        const Hello = ({ name, children }) => <b>Hi {name}{children}</b>;
        function Wrap(props) { return <section>{props.children}</section>; }
        const UI = { Title: (p) => <h1>{p.text}</h1> };
        class Counter { constructor(p) { this.n = p.start; } render() { return <i>{this.n}</i>; } }
        const Nothing = () => null;
        renderToString(<Wrap><Hello name=\"A\">!</Hello><UI.Title text=\"T\" /><Counter start={3} /><Nothing /></Wrap>)";
    assert_eq!(run(src), "<section><b>Hi A!</b><h1>T</h1><i>3</i></section>");
}

#[test]
fn jsx_fragments() {
    assert_eq!(run("renderToString(<ul><><li>a</li><li>b</li></><li>c</li></ul>)"), "<ul><li>a</li><li>b</li><li>c</li></ul>");
    assert_eq!(run("const f = <><i>1</i>x</>; Array.isArray(f) + ':' + f.length"), "true:2");
}

#[test]
fn jsx_in_expressions() {
    assert_eq!(run("const ok = true; renderToString(ok ? <b>y</b> : <i>n</i>)"), "<b>y</b>");
    assert_eq!(run("const xs = [1, 2]; renderToString(xs.length > 1 && <p>{xs.length}</p>)"), "<p>2</p>");
    assert_eq!(run("const f = () => (\n  <div>\n    a / b\n  </div>\n); renderToString(f()) + (6 / 3)"), "<div>a / b</div>2");
    assert_eq!(run("const t = `${renderToString(<b>1</b>)}!`; t"), "<b>1</b>!");
    assert_eq!(run("function f<T,>(x: T) { return <p>{String(x)}</p>; } renderToString(f(1))"), "<p>1</p>");
}

#[test]
fn jsx_custom_factory() {
    let src = "/** @jsx h */\n/** @jsxFrag Frag */\nconst Frag = 'F';\nfunction h(t, p, ...c) { return [typeof t === 'string' ? t : t.name, p === null ? 'null' : Object.keys(p).join('+'), c.length].join(':'); }\n[<a x=\"1\" y={2}>t{3}</a>, <></>].join(' ')";
    assert_eq!(run(src), "a:x+y:2 F:null:0");
    // the default factory is an ordinary global a script may replace
    assert_eq!(run("__jsx = (t) => 'mine ' + t; <p/>"), "mine p");
}

#[test]
fn jsx_errors() {
    assert!(run("<div>").contains("unterminated JSX"));
    assert!(run("<a></b>").contains("expected '</a>'"));
    assert!(run("<a b={}></a>").contains("must not be empty"));
}

#[test]
fn syntax_switches() {
    // without JSX, `<` in an operand position is a type assertion in TS
    assert!(run_as("<div/>", false, false).contains("SyntaxError"));
    // with JSX only, annotations are errors
    assert!(run_as("let a: number = 1", false, true).contains("SyntaxError"));
    assert_eq!(run_as("renderToString(<b>j</b>)", false, true), "<b>j</b>");
}

#[test]
fn line_numbers_survive_jsx() {
    let src = "const a = <div>\n  <p>x</p>\n</div>;\nlet b = ;";
    assert!(run(src).contains("line 4"));
}

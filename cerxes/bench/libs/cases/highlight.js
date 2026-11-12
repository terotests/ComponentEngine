import hljs from "highlight.js";
const js = `async function* gen(xs = []) { for await (const x of xs) yield x?.y ?? \`t\${1}\`; } const re = /a+b/gi; class A extends B { #p = 1; static s() { return 0x1F + 1e3; } } // c\n/* block */`;
const py = `@decorator\ndef f(x: int = 3, *args, **kw) -> str:\n    """doc"""\n    return f"{x!r:>10}" if x else lambda y: y ** 2  # c\nclass K(Base, metaclass=M): pass`;
const css = `@media (max-width: 600px) { .a > b:hover::after { content: "x"; color: #fff !important; transition: all .3s ease-in-out; } }`;
const sql = `SELECT a.id, COUNT(*) AS n FROM t a LEFT JOIN u ON u.id = a.uid WHERE a.x BETWEEN 1 AND 10 GROUP BY a.id HAVING n > 2 ORDER BY n DESC LIMIT 5;`;
const big = Array.from({ length: 20 }, () => js + "\n" + py.replace(/^/gm, "// ")).join("\n");
export default {
  check: {
    javascript: () => hljs.highlight(js, { language: "javascript" }).value,
    python: () => hljs.highlight(py, { language: "python" }).value,
    css: () => hljs.highlight(css, { language: "css" }).value,
    sql: () => hljs.highlight(sql, { language: "sql" }).value,
    auto: () => [js, py, css, sql].map((s) => hljs.highlightAuto(s, ["javascript", "python", "css", "sql", "rust", "go"]).language),
  },
  bench: {
    javascript: () => hljs.highlight(big, { language: "javascript" }).value.length,
    auto: () => hljs.highlightAuto(py + "\n" + sql, ["javascript", "python", "css", "sql", "rust", "go", "java", "cpp"]).relevance,
  },
};

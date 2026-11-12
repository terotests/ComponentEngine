import katex from "katex";
const exprs = ["\\int_0^\\infty e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2}", "\\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}", "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}^{-1} = \\frac{1}{ad-bc}\\begin{pmatrix} d & -b \\\\ -c & a \\end{pmatrix}", "f(x) = \\begin{cases} x^2 & x \\ge 0 \\\\ -x & \\text{otherwise} \\end{cases}", "\\mathbb{R}^n \\xrightarrow{\;T\;} \\overbrace{a+b}^{\\text{sum}} \\sqrt[3]{\\hat{x}\\vec{v}}"];
export default {
  check: {
    html: () => exprs.map((e) => katex.renderToString(e, { output: "html" })),
    mathml: () => katex.renderToString(exprs[2], { output: "mathml", displayMode: true }),
    error: () => { try { katex.renderToString("\\frac{1}{"); return "ok"; } catch (e) { return e.message; } },
    errorColor: () => katex.renderToString("\\badmacro x", { throwOnError: false }),
  },
  bench: {
    render: () => exprs.reduce((n, e) => n + katex.renderToString(e).length, 0),
  },
};

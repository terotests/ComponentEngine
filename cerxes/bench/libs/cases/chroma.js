import chroma from "chroma-js";
export default {
  check: {
    convert: () => [chroma("hotpink").hex(), chroma("#D4F880").darken().hex(), chroma("teal").hsl().map((x) => +x.toFixed(6)), chroma(255, 128, 0).lab().map((x) => +x.toFixed(6)), chroma.temperature(5500).hex(), chroma("red").luminance(), chroma.contrast("pink", "hotpink")],
    scales: () => [chroma.scale(["yellow", "008ae5"]).mode("lch").colors(6), chroma.scale("RdYlBu").domain([0, 100]).classes(5)(33).hex(), chroma.cubehelix().scale().colors(4), chroma.mix("red", "blue", 0.3, "lab").hex(), chroma.average(["#ddd", "yellow", "red", "teal"], "lch").hex()],
    misc: () => [chroma.deltaE("#ededee", "#ececee"), chroma.limits([1, 2, 3, 4, 5, 10, 20, 100], "q", 4), chroma.valid("nope"), chroma("rgba(255,0,0,0.5)").css("hsl")],
  },
  bench: {
    scale: () => chroma.scale(["#fafa6e", "#2A4858"]).mode("lch").colors(500).length,
    convert: () => { let s = 0; for (let i = 0; i < 1000; i++) s += chroma.hsl(i % 360, 0.5, 0.5).lab()[0]; return s; },
  },
};

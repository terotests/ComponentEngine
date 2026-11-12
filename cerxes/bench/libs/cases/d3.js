import * as d3 from "d3";
const rnd = d3.randomLcg(42);
const pts = Array.from({ length: 2000 }, (_, i) => ({ x: rnd() * 100, y: rnd() * 50, g: "g" + (i % 7) }));
const tree = { name: "root", children: Array.from({ length: 10 }, (_, i) => ({ name: "c" + i, children: Array.from({ length: 10 }, (_, j) => ({ name: `c${i}-${j}`, value: (i + 1) * (j + 2) })) })) };
export default {
  check: {
    stats: () => [d3.extent(pts, (d) => d.x), d3.mean(pts, (d) => d.y), d3.median(pts, (d) => d.x), d3.deviation(pts, (d) => d.y), d3.quantile(pts.map((d) => d.x).sort(d3.ascending), 0.9), [...d3.rollup(pts, (v) => v.length, (d) => d.g)], d3.bin().thresholds(10)(pts.map((d) => d.x)).map((b) => b.length)],
    scales: () => { const s = d3.scaleLinear().domain([0, 100]).range([0, 960]).nice(); const t = d3.scaleTime().domain([new Date(Date.UTC(2020, 0, 1)), new Date(Date.UTC(2021, 0, 1))]); const c = d3.scaleSequential(d3.interpolateViridis).domain([0, 1]); return [s(33.3), s.invert(500), s.ticks(7), t.ticks(5).map((d) => d.toISOString()), c(0.25), d3.scaleOrdinal(d3.schemeCategory10)("x"), d3.scaleLog().domain([1, 1000]).ticks(5)]; },
    shapes: () => [d3.line().curve(d3.curveCatmullRom)(pts.slice(0, 6).map((d) => [d.x, d.y])), d3.arc()({ innerRadius: 10, outerRadius: 50, startAngle: 0, endAngle: 2 }), d3.pie()([1, 2, 3]).map((a) => +a.endAngle.toFixed(6)), d3.area().curve(d3.curveMonotoneX)([[0, 1], [1, 3], [2, 2]])],
    hierarchy: () => { const r = d3.hierarchy(tree).sum((d) => d.value || 0).sort((a, b) => b.value - a.value); d3.treemap().size([400, 300]).padding(1)(r); return r.leaves().slice(0, 4).map((l) => [l.data.name, +l.x0.toFixed(4), +l.y0.toFixed(4), +l.x1.toFixed(4)]); },
    format: () => [d3.format(",.2f")(1234567.891), d3.format(".3s")(0.000123), d3.format("+$,.0f")(-1e6), d3.timeFormat("%A %d %B %Y %H:%M")(new Date(Date.UTC(2024, 1, 27, 10, 5))), d3.utcParse("%Y-%m-%dT%H:%M")("2024-02-27T10:05").toISOString(), d3.color("steelblue").darker(1).formatHex(), d3.interpolate({ a: 0, b: "1px" }, { a: 10, b: "5px" })(0.25)],
    geo: () => { const p = d3.geoMercator().scale(100).translate([0, 0]); const path = d3.geoPath(p); return [p([24.94, 60.17]), path({ type: "Polygon", coordinates: [[[0, 0], [10, 0], [10, 10], [0, 10], [0, 0]]] }), d3.geoDistance([24.94, 60.17], [-0.12, 51.5]), d3.geoArea({ type: "Polygon", coordinates: [[[0, 0], [0, 10], [10, 10], [10, 0], [0, 0]]] })]; },
    delaunay: () => { const d = d3.Delaunay.from(pts.slice(0, 200), (p) => p.x, (p) => p.y); return [d.triangles.length, d.find(50, 25), d.voronoi([0, 0, 100, 50]).cellPolygon(3).length]; },
    forces: () => { const nodes = Array.from({ length: 50 }, (_, i) => ({ id: i })); const links = nodes.slice(1).map((n, i) => ({ source: i, target: n.id })); const sim = d3.forceSimulation(nodes).force("link", d3.forceLink(links)).force("charge", d3.forceManyBody()).force("center", d3.forceCenter()).stop(); sim.tick(100); return nodes.slice(0, 3).map((n) => [+n.x.toFixed(3), +n.y.toFixed(3)]); },
  },
  bench: {
    groupsBins: () => { const r = d3.rollups(pts, (v) => d3.bin()(v.map((d) => d.x)).length, (d) => d.g); return r.length; },
    treemap: () => { const r = d3.hierarchy(tree).sum((d) => d.value || 0); d3.treemap().size([400, 300])(r); return r.leaves().length; },
    delaunay: () => d3.Delaunay.from(pts, (p) => p.x, (p) => p.y).triangles.length,
    force: () => { const nodes = Array.from({ length: 100 }, (_, i) => ({ id: i })); const sim = d3.forceSimulation(nodes).force("charge", d3.forceManyBody()).force("link", d3.forceLink(nodes.slice(1).map((n, i) => ({ source: i, target: n.id })))).stop(); sim.tick(30); return nodes.length; },
    lineShape: () => d3.line().curve(d3.curveBasis)(pts.map((d) => [d.x, d.y])).length,
  },
};

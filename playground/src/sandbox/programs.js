// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The sandboxes' programs: plain JavaScript that CErXes runs and that never
// returns. The page talks to a running program through three globals, which
// it reads and writes between time slices without running any code:
//
//   view      written by the page: "gen,cx,cy,scale" (scale = the width of
//             the picture in the complex plane). A new gen starts a new
//             picture; the program notices at the next row.
//   progress  written by the program: "gen pass row rows maxIter"
//   result    written by the program at the end of a pass: JSON with what it
//             found so far (the page's "Fetch results" reads it)
//
// and console.log lines "row <gen> <y> <block> <maxIter> <data>": one row of
// blocks, two characters per block (base 64 iteration count, "//" = inside).
//
// Each picture is drawn in passes: blocks of 8, 4, 2 and 1 pixels, then the
// iteration limit doubles, pass after pass, up to 2^16; then the program
// waits for the next view. The four programs share that frame and differ in
// the formula.

const FRAME = String.raw`
var W = 320, H = 240;
var B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
var progress = "";
var result = "{}";
var seen = "";
var gen = 0, cx = 0, cy = 0, scale = 4;

function readView() {
  seen = view;
  var p = String(view).split(",");
  gen = +p[0]; cx = +p[1]; cy = +p[2]; scale = +p[3];
}

function enc(n, max) {
  if (n >= max) return "//";
  if (n > 4094) n = 4094;
  return B64.charAt(n >> 6) + B64.charAt(n & 63);
}

function picture() {
  readView();
  var t0 = Date.now();
  var pass = 0, max = 32, block = 8;
  while (max <= 65536) {
    var inside = 0, total = 0, iters = 0;
    var step = scale / W;
    var x0 = cx - scale / 2, y0 = cy - (step * H) / 2;
    for (var y = 0; y < H; y += block) {
      if (view !== seen) return;
      var line = "";
      var py = y0 + (y + block / 2) * step;
      for (var x = 0; x < W; x += block) {
        var n = escape(x0 + (x + block / 2) * step, py, max);
        if (n >= max) inside++;
        iters += n;
        total++;
        line += enc(n, max);
      }
      console.log("row " + gen + " " + y + " " + block + " " + max + " " + line);
      progress = gen + " " + pass + " " + (y + block) + " " + H + " " + max;
    }
    var cell = (scale / W) * block;
    result = JSON.stringify({
      program: NAME, gen: gen, pass: pass, block: block, maxIter: max,
      center: [cx, cy], width: scale,
      insideShare: inside / total,
      insideArea: inside * cell * cell,
      meanIter: iters / total,
      ms: Date.now() - t0
    });
    pass++;
    if (block > 1) { block = block / 2; max = max * 2; } else max = max * 2;
  }
  progress = gen + " done " + H + " " + H + " " + max;
  while (view === seen) {}
}

while (true) picture();
`;

const program = (name, body) => `// ${name}\nvar NAME = ${JSON.stringify(name)};\n\n${body.trim()}\n${FRAME}`;

export const PROGRAMS = [
  {
    id: "mandelbrot",
    name: "Mandelbrot",
    hue: 0.0,
    view: [-0.6, 0, 3.4],
    code: program("Mandelbrot", `
// z = z² + c, z starts at 0, c is the point
function escape(px, py, max) {
  var x = 0, y = 0, n = 0;
  while (n < max) {
    var xx = x * x, yy = y * y;
    if (xx + yy > 4) break;
    y = 2 * x * y + py;
    x = xx - yy + px;
    n++;
  }
  return n;
}`),
  },
  {
    id: "julia",
    name: "Julia (c = −0.123 + 0.745i)",
    hue: 0.33,
    view: [0, 0, 3.2],
    code: program("Julia", `
// z = z² + c, z starts at the point, c is fixed
var CR = -0.123, CI = 0.745;
function escape(px, py, max) {
  var x = px, y = py, n = 0;
  while (n < max) {
    var xx = x * x, yy = y * y;
    if (xx + yy > 4) break;
    y = 2 * x * y + CI;
    x = xx - yy + CR;
    n++;
  }
  return n;
}`),
  },
  {
    id: "ship",
    name: "Burning Ship",
    hue: 0.08,
    view: [-0.45, 0.5, 3.4],
    code: program("Burning Ship", `
// z = (|Re z| + i|Im z|)² + c, drawn with the imaginary axis up
function escape(px, py, max) {
  var x = 0, y = 0, n = 0;
  while (n < max) {
    var xx = x * x, yy = y * y;
    if (xx + yy > 4) break;
    y = Math.abs(2 * x * y) - py;
    x = xx - yy + px;
    n++;
  }
  return n;
}`),
  },
  {
    id: "multibrot",
    name: "Multibrot (z³ + c)",
    hue: 0.62,
    view: [0, 0, 3.0],
    code: program("Multibrot", `
// z = z³ + c
function escape(px, py, max) {
  var x = 0, y = 0, n = 0;
  while (n < max) {
    var xx = x * x, yy = y * y;
    if (xx + yy > 4) break;
    var nx = x * (xx - 3 * yy) + px;
    y = y * (3 * xx - yy) + py;
    x = nx;
    n++;
  }
  return n;
}`),
  },
];

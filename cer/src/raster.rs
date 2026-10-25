// SPDX-License-Identifier: AGPL-3.0-or-later
//! A 2D rasterizer for canvas drawing: paths filled and stroked into an
//! RGBA surface, with anti-aliasing, the nonzero and even-odd rules,
//! solid, gradient and image paints, Porter-Duff composition and a clip
//! mask. It knows nothing of the engine or of any canvas API: a renderer
//! (web/dom's CanvasRenderingContext2D, or any other 2D canvas) records
//! its drawing as a list of numbers in device coordinates (its own
//! transform already applied) and hands it here.
//!
//! The surface is premultiplied RGBA, four doubles 0..1 per pixel, row by
//! row; a clip mask is one double 0..1 per pixel, empty for none.
//!
//! The command list (each command a code and its numbers):
//!
//! | code | command | numbers |
//! |---|---|---|
//! | 0 | BEGIN | (a new path) |
//! | 1 | MOVE | x y |
//! | 2 | LINE | x y |
//! | 3 | CUBIC | x1 y1 x2 y2 x y |
//! | 4 | QUAD | cx cy x y |
//! | 5 | CLOSE | |
//! | 10 | FILL | rule (0 nonzero, 1 even-odd) |
//! | 11 | STROKE | width cap (0 butt, 1 round, 2 square) join (0 miter, 1 round, 2 bevel) miterLimit |
//! | 12 | DASH | n offset d1 .. dn (n 0: solid) |
//! | 13 | COMPOSITE | mode (OP_* below) |
//! | 20 | SOLID | r g b a (0..1, not premultiplied; a with the global alpha) |
//! | 21 | LINEAR | alpha, the inverse transform a b c d e f, x0 y0 x1 y1, n, n stops (offset r g b a) |
//! | 22 | RADIAL | alpha, the inverse transform, x0 y0 r0 x1 y1 r1, n, n stops |
//! | 23 | IMAGE | alpha, source index, the inverse transform (device to source pixels), repeat x, repeat y, smoothing |
//!
//! A gradient's or an image's inverse transform takes a device pixel to
//! the paint's own space. FILL and STROKE paint the current path with the
//! current paint and composition; the path stays until BEGIN.

use ranger::prelude::*;

pub const RC_BEGIN: int = 0;
pub const RC_MOVE: int = 1;
pub const RC_LINE: int = 2;
pub const RC_CUBIC: int = 3;
pub const RC_QUAD: int = 4;
pub const RC_CLOSE: int = 5;
pub const RC_FILL: int = 10;
pub const RC_STROKE: int = 11;
pub const RC_DASH: int = 12;
pub const RC_COMPOSITE: int = 13;
pub const RC_SOLID: int = 20;
pub const RC_LINEAR: int = 21;
pub const RC_RADIAL: int = 22;
pub const RC_IMAGE: int = 23;

pub const OP_SOURCE_OVER: int = 0;
pub const OP_SOURCE_IN: int = 1;
pub const OP_SOURCE_OUT: int = 2;
pub const OP_SOURCE_ATOP: int = 3;
pub const OP_DESTINATION_OVER: int = 4;
pub const OP_DESTINATION_IN: int = 5;
pub const OP_DESTINATION_OUT: int = 6;
pub const OP_DESTINATION_ATOP: int = 7;
pub const OP_LIGHTER: int = 8;
pub const OP_COPY: int = 9;
pub const OP_XOR: int = 10;

/// sub-scanlines per pixel row (vertical anti-aliasing)
const SS: int = 4;

/// A source image for IMAGE paints: premultiplied RGBA as a surface.
pub struct RasterImage {
    pub w: int,
    pub h: int,
    pub px: Vec<double>,
}

/// The polygon list of a path: points (x, y pairs) and where each ring
/// starts, with whether it was closed.
struct Rings {
    pts: Vec<double>,
    starts: Vec<int>,
    closed: Vec<bool>,
    // after CLOSE, the next segment starts a subpath at the closed one's start
    reopen: bool,
}

impl Rings {
    fn new() -> Rings {
        Rings { pts: Vec::new(), starts: Vec::new(), closed: Vec::new(), reopen: false }
    }
    fn count(&self) -> int {
        self.starts.len() as int
    }
    fn ring_end(&self, r: int) -> int {
        if r + 1 < self.count() {
            self.starts[(r + 1) as usize]
        } else {
            (self.pts.len() / 2) as int
        }
    }
    fn start_ring(&mut self, x: double, y: double) {
        self.reopen = false;
        self.starts.push((self.pts.len() / 2) as int);
        self.closed.push(false);
        self.pts.push(x);
        self.pts.push(y);
    }
    fn add(&mut self, x: double, y: double) {
        if self.reopen {
            let s = self.starts[self.starts.len() - 1];
            let sx = self.pts[(s * 2) as usize];
            let sy = self.pts[(s * 2 + 1) as usize];
            self.start_ring(sx, sy);
        }
        self.pts.push(x);
        self.pts.push(y);
    }
    // the current point (after CLOSE, the closed subpath's start)
    fn last_x(&self) -> double {
        if self.reopen {
            return self.pts[(self.starts[self.starts.len() - 1] * 2) as usize];
        }
        self.pts[self.pts.len() - 2]
    }
    fn last_y(&self) -> double {
        if self.reopen {
            return self.pts[(self.starts[self.starts.len() - 1] * 2 + 1) as usize];
        }
        self.pts[self.pts.len() - 1]
    }
    fn close(&mut self) {
        let k = self.count();
        if k > 0 {
            self.closed[(k - 1) as usize] = true;
            self.reopen = true;
        }
    }
}

/// What a command list draws with: the paint and how lines are drawn.
struct Pen {
    kind: int,
    color: Vec<double>,
    alpha: double,
    m: Vec<double>,
    geo: Vec<double>,
    stops: Vec<double>,
    image: int,
    rep_x: bool,
    rep_y: bool,
    smooth: bool,
    op: int,
    dash: Vec<double>,
    dash_off: double,
}

/// Runs a command list on the surface `px` (w x h), within the clip mask
/// `clip` (empty: none). `images` are the IMAGE paints' sources.
pub fn run(px: &mut Vec<double>, w: int, h: int, cmds: &Vec<double>, clip: &Vec<double>, images: &Vec<RasterImage>) {
    let mut path = Rings::new();
    let mut pen = Pen {
        kind: RC_SOLID,
        color: vec![0.0, 0.0, 0.0, 1.0],
        alpha: 1.0,
        m: vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        geo: Vec::new(),
        stops: Vec::new(),
        image: -1,
        rep_x: false,
        rep_y: false,
        smooth: true,
        op: OP_SOURCE_OVER,
        dash: Vec::new(),
        dash_off: 0.0,
    };
    let n = cmds.len() as int;
    let mut i: int = 0;
    while i < n {
        let c = cmds[i as usize] as int;
        i += 1;
        if c == RC_BEGIN {
            path = Rings::new();
        } else if c == RC_MOVE {
            path.start_ring(at(cmds, i), at(cmds, i + 1));
            i += 2;
        } else if c == RC_LINE {
            if path.count() == 0 {
                path.start_ring(at(cmds, i), at(cmds, i + 1));
            } else {
                path.add(at(cmds, i), at(cmds, i + 1));
            }
            i += 2;
        } else if c == RC_CUBIC {
            if path.count() == 0 {
                path.start_ring(at(cmds, i), at(cmds, i + 1));
            }
            cubic(&mut path, at(cmds, i), at(cmds, i + 1), at(cmds, i + 2), at(cmds, i + 3), at(cmds, i + 4), at(cmds, i + 5));
            i += 6;
        } else if c == RC_QUAD {
            if path.count() == 0 {
                path.start_ring(at(cmds, i), at(cmds, i + 1));
            }
            let x0 = path.last_x();
            let y0 = path.last_y();
            let cx = at(cmds, i);
            let cy = at(cmds, i + 1);
            let x = at(cmds, i + 2);
            let y = at(cmds, i + 3);
            cubic(&mut path, x0 + 2.0 / 3.0 * (cx - x0), y0 + 2.0 / 3.0 * (cy - y0), x + 2.0 / 3.0 * (cx - x), y + 2.0 / 3.0 * (cy - y), x, y);
            i += 4;
        } else if c == RC_CLOSE {
            path.close();
        } else if c == RC_FILL {
            let rule = at(cmds, i) as int;
            i += 1;
            let mut cov = Coverage::new();
            cov.fill(&path, rule, w, h, true);
            paint(px, w, h, &cov, clip, &pen, images);
        } else if c == RC_STROKE {
            let width = at(cmds, i);
            let cap = at(cmds, i + 1) as int;
            let join = at(cmds, i + 2) as int;
            let limit = at(cmds, i + 3);
            i += 4;
            if width > 0.0 {
                let outline = stroke(&path, width / 2.0, cap, join, limit, &pen.dash, pen.dash_off);
                let mut cov = Coverage::new();
                cov.fill(&outline, 0, w, h, true);
                paint(px, w, h, &cov, clip, &pen, images);
            }
        } else if c == RC_DASH {
            let k = at(cmds, i) as int;
            pen.dash_off = at(cmds, i + 1);
            pen.dash = Vec::new();
            let mut j: int = 0;
            while j < k {
                pen.dash.push(at(cmds, i + 2 + j));
                j += 1;
            }
            i += 2 + k;
        } else if c == RC_COMPOSITE {
            pen.op = at(cmds, i) as int;
            i += 1;
        } else if c == RC_SOLID {
            pen.kind = RC_SOLID;
            pen.color = vec![at(cmds, i), at(cmds, i + 1), at(cmds, i + 2), at(cmds, i + 3)];
            i += 4;
        } else if c == RC_LINEAR || c == RC_RADIAL {
            pen.kind = c;
            pen.alpha = at(cmds, i);
            pen.m = read_n(cmds, i + 1, 6);
            let g = if c == RC_LINEAR { 4 } else { 6 };
            pen.geo = read_n(cmds, i + 7, g);
            let k = at(cmds, i + 7 + g) as int;
            pen.stops = read_n(cmds, i + 8 + g, k * 5);
            i += 8 + g + k * 5;
        } else if c == RC_IMAGE {
            pen.kind = RC_IMAGE;
            pen.alpha = at(cmds, i);
            pen.image = at(cmds, i + 1) as int;
            pen.m = read_n(cmds, i + 2, 6);
            pen.rep_x = at(cmds, i + 8) != 0.0;
            pen.rep_y = at(cmds, i + 9) != 0.0;
            pen.smooth = at(cmds, i + 10) != 0.0;
            i += 11;
        } else {
            // an unknown command: the rest cannot be read
            return;
        }
    }
}

/// The clip mask `clip` (w x h, filled with 1 when empty) narrowed to
/// the path the command list builds, filled with `rule`.
pub fn clip_to(clip: &mut Vec<double>, w: int, h: int, cmds: &Vec<double>, rule: int) {
    let n = (w * h) as usize;
    if clip.len() != n {
        *clip = Vec::with_capacity(n);
        let mut k: usize = 0;
        while k < n {
            clip.push(1.0);
            k += 1;
        }
    }
    let mut cov = Coverage::new();
    // the path, through a run that only builds it
    let mut path = Rings::new();
    let mut i: int = 0;
    let len = cmds.len() as int;
    while i < len {
        let c = cmds[i as usize] as int;
        i += 1;
        if c == RC_MOVE {
            path.start_ring(at(cmds, i), at(cmds, i + 1));
            i += 2;
        } else if c == RC_LINE {
            if path.count() == 0 {
                path.start_ring(at(cmds, i), at(cmds, i + 1));
            } else {
                path.add(at(cmds, i), at(cmds, i + 1));
            }
            i += 2;
        } else if c == RC_CUBIC {
            if path.count() == 0 {
                path.start_ring(at(cmds, i), at(cmds, i + 1));
            }
            cubic(&mut path, at(cmds, i), at(cmds, i + 1), at(cmds, i + 2), at(cmds, i + 3), at(cmds, i + 4), at(cmds, i + 5));
            i += 6;
        } else if c == RC_CLOSE {
            path.close();
        } else if c == RC_BEGIN {
            path = Rings::new();
        } else {
            break;
        }
    }
    cov.fill(&path, rule, w, h, false);
    // outside the path's box the clip is gone; inside, times the coverage
    let mut y: int = 0;
    while y < h {
        let mut x: int = 0;
        while x < w {
            let k = (y * w + x) as usize;
            clip[k] = clip[k] * cov.at(x, y);
            x += 1;
        }
        y += 1;
    }
}

fn at(cmds: &Vec<double>, i: int) -> double {
    if i < 0 || i >= cmds.len() as int {
        return 0.0;
    }
    cmds[i as usize]
}

fn read_n(cmds: &Vec<double>, i: int, n: int) -> Vec<double> {
    let mut out: Vec<double> = Vec::new();
    let mut k: int = 0;
    while k < n {
        out.push(at(cmds, i + k));
        k += 1;
    }
    out
}

/// A cubic Bézier from the path's last point, as line segments short
/// enough that the curve is off by less than a tenth of a pixel.
fn cubic(path: &mut Rings, x1: double, y1: double, x2: double, y2: double, x: double, y: double) {
    let x0 = path.last_x();
    let y0 = path.last_y();
    let ddx = (x0 - 2.0 * x1 + x2).abs().max((x1 - 2.0 * x2 + x).abs());
    let ddy = (y0 - 2.0 * y1 + y2).abs().max((y1 - 2.0 * y2 + y).abs());
    let dd = (ddx * ddx + ddy * ddy).sqrt();
    // segments for a flatness of 0.1 px: n^2 >= 3/4 * dd / 0.1
    let mut n = (7.5 * dd).sqrt().ceil() as int;
    if n < 1 {
        n = 1;
    }
    if n > 256 {
        n = 256;
    }
    let mut k: int = 1;
    while k <= n {
        let t = (k as double) / (n as double);
        let u = 1.0 - t;
        let a = u * u * u;
        let b = 3.0 * u * u * t;
        let c = 3.0 * u * t * t;
        let d = t * t * t;
        path.add(a * x0 + b * x1 + c * x2 + d * x, a * y0 + b * y1 + c * y2 + d * y);
        k += 1;
    }
}

// ---- coverage

/// How much of each pixel of a box a path covers, 0..1.
struct Coverage {
    x0: int,
    y0: int,
    w: int,
    h: int,
    cov: Vec<double>,
}

impl Coverage {
    fn new() -> Coverage {
        Coverage { x0: 0, y0: 0, w: 0, h: 0, cov: Vec::new() }
    }

    fn at(&self, x: int, y: int) -> double {
        let lx = x - self.x0;
        let ly = y - self.y0;
        if lx < 0 || ly < 0 || lx >= self.w || ly >= self.h {
            return 0.0;
        }
        self.cov[(ly * self.w + lx) as usize]
    }

    /// Scanline fill of every ring of `path` (each closed for the fill),
    /// with `rule`, inside the surface; `boxed`: only the path's box is
    /// kept, else the whole surface.
    fn fill(&mut self, path: &Rings, rule: int, sw: int, sh: int, boxed: bool) {
        // the edges: x0 y0 x1 y1 winding, top to bottom
        let mut ex0: Vec<double> = Vec::new();
        let mut ey0: Vec<double> = Vec::new();
        let mut ex1: Vec<double> = Vec::new();
        let mut ey1: Vec<double> = Vec::new();
        let mut ewind: Vec<double> = Vec::new();
        let mut minx: double = 1.0e300;
        let mut miny: double = 1.0e300;
        let mut maxx: double = -1.0e300;
        let mut maxy: double = -1.0e300;
        let mut r: int = 0;
        while r < path.count() {
            let s = path.starts[r as usize];
            let e = path.ring_end(r);
            if e - s >= 2 {
                let mut k = s;
                while k < e {
                    let j = if k + 1 < e { k + 1 } else { s };
                    let ax = path.pts[(k * 2) as usize];
                    let ay = path.pts[(k * 2 + 1) as usize];
                    let bx = path.pts[(j * 2) as usize];
                    let by = path.pts[(j * 2 + 1) as usize];
                    if ax < minx { minx = ax; }
                    if ax > maxx { maxx = ax; }
                    if ay < miny { miny = ay; }
                    if ay > maxy { maxy = ay; }
                    if ay != by && ax == ax && bx == bx && ay == ay && by == by {
                        if ay < by {
                            ex0.push(ax); ey0.push(ay); ex1.push(bx); ey1.push(by); ewind.push(1.0);
                        } else {
                            ex0.push(bx); ey0.push(by); ex1.push(ax); ey1.push(ay); ewind.push(-1.0);
                        }
                    }
                    k += 1;
                }
            }
            r += 1;
        }
        if boxed {
            self.x0 = clampi(minx.floor() as int, 0, sw);
            self.y0 = clampi(miny.floor() as int, 0, sh);
            self.w = clampi(maxx.ceil() as int + 1, 0, sw) - self.x0;
            self.h = clampi(maxy.ceil() as int + 1, 0, sh) - self.y0;
        } else {
            self.x0 = 0;
            self.y0 = 0;
            self.w = sw;
            self.h = sh;
        }
        if self.w <= 0 || self.h <= 0 || ex0.is_empty() {
            self.w = 0;
            self.h = 0;
            self.cov = Vec::new();
            return;
        }
        let total = (self.w * self.h) as usize;
        self.cov = Vec::with_capacity(total);
        let mut q: usize = 0;
        while q < total {
            self.cov.push(0.0);
            q += 1;
        }
        // the edges by the row they start in, then an active list
        let ne = ex0.len() as int;
        let mut order: Vec<int> = Vec::new();
        let mut bucket_start: Vec<int> = Vec::new();
        let mut counts: Vec<int> = Vec::new();
        let mut b: int = 0;
        while b <= self.h {
            counts.push(0);
            b += 1;
        }
        let mut k: int = 0;
        while k < ne {
            let row = clampi(ey0[k as usize].floor() as int - self.y0, 0, self.h);
            counts[row as usize] += 1;
            k += 1;
        }
        let mut acc: int = 0;
        b = 0;
        while b <= self.h {
            bucket_start.push(acc);
            acc += counts[b as usize];
            b += 1;
        }
        let mut fillp: Vec<int> = bucket_start.clone();
        k = 0;
        while k < ne {
            order.push(0);
            k += 1;
        }
        k = 0;
        while k < ne {
            let row = clampi(ey0[k as usize].floor() as int - self.y0, 0, self.h);
            order[fillp[row as usize] as usize] = k;
            fillp[row as usize] += 1;
            k += 1;
        }
        let mut active: Vec<int> = Vec::new();
        let mut row_acc: Vec<double> = Vec::new();
        let mut q2: int = 0;
        while q2 < self.w + 2 {
            row_acc.push(0.0);
            q2 += 1;
        }
        let mut xs: Vec<double> = Vec::new();
        let mut ws: Vec<double> = Vec::new();
        let wgt = 1.0 / (SS as double);
        let mut ly: int = 0;
        while ly < self.h {
            // edges starting in this row join (rows above the box: row 0)
            let mut z = bucket_start[ly as usize];
            let zend = bucket_start[(ly + 1) as usize];
            while z < zend {
                active.push(order[z as usize]);
                z += 1;
            }
            let y = (ly + self.y0) as double;
            // edges that ended
            let mut kept: Vec<int> = Vec::new();
            for e in active.iter() {
                if ey1[*e as usize] > y {
                    kept.push(*e);
                }
            }
            active = kept;
            if active.is_empty() {
                ly += 1;
                continue;
            }
            let mut touched = false;
            let mut s: int = 0;
            while s < SS {
                let sy = y + ((s as double) + 0.5) / (SS as double);
                xs.clear();
                ws.clear();
                for e in active.iter() {
                    let ei = *e as usize;
                    if ey0[ei] <= sy && sy < ey1[ei] {
                        let x = ex0[ei] + (sy - ey0[ei]) * (ex1[ei] - ex0[ei]) / (ey1[ei] - ey0[ei]);
                        // insertion, sorted by x
                        let mut p = xs.len();
                        xs.push(x);
                        ws.push(ewind[ei]);
                        while p > 0 && xs[p - 1] > x {
                            xs[p] = xs[p - 1];
                            ws[p] = ws[p - 1];
                            p -= 1;
                        }
                        xs[p] = x;
                        ws[p] = ewind[ei];
                    }
                }
                let mut wind: double = 0.0;
                let mut c: usize = 0;
                while c + 1 < xs.len() {
                    wind = wind + ws[c];
                    let inside = if rule == 1 { (wind as int) % 2 != 0 } else { wind != 0.0 };
                    if inside {
                        let xa = xs[c] - (self.x0 as double);
                        let xb = xs[c + 1] - (self.x0 as double);
                        span(&mut row_acc, xa, xb, wgt, self.w);
                        touched = true;
                    }
                    c += 1;
                }
                s += 1;
            }
            if touched {
                let base = (ly * self.w) as usize;
                let mut x: int = 0;
                while x < self.w {
                    let v = row_acc[x as usize];
                    if v != 0.0 {
                        self.cov[base + (x as usize)] = if v > 1.0 { 1.0 } else if v < 0.0 { 0.0 } else { v };
                        row_acc[x as usize] = 0.0;
                    }
                    x += 1;
                }
                row_acc[self.w as usize] = 0.0;
                row_acc[(self.w + 1) as usize] = 0.0;
            }
            ly += 1;
        }
    }
}

/// Adds the part of each pixel between xa and xb (local x) to `acc`.
fn span(acc: &mut Vec<double>, xa0: double, xb0: double, wgt: double, w: int) {
    let mut xa = xa0;
    let mut xb = xb0;
    if xa < 0.0 { xa = 0.0; }
    if xb > w as double { xb = w as double; }
    if xb <= xa {
        return;
    }
    let ia = xa.floor() as int;
    let ib = xb.floor() as int;
    if ia == ib {
        acc[ia as usize] += (xb - xa) * wgt;
        return;
    }
    acc[ia as usize] += ((ia + 1) as double - xa) * wgt;
    let mut i = ia + 1;
    while i < ib {
        acc[i as usize] += wgt;
        i += 1;
    }
    if ib < w {
        acc[ib as usize] += (xb - (ib as double)) * wgt;
    }
}

fn clampi(v: int, lo: int, hi: int) -> int {
    if v < lo { lo } else if v > hi { hi } else { v }
}

// ---- strokes

/// The outline of the path's lines `hw` from them on both sides, as rings
/// each wound the same way (so nonzero fills their union).
fn stroke(path: &Rings, hw: double, cap: int, join: int, limit: double, dash: &Vec<double>, dash_off: double) -> Rings {
    let mut out = Rings::new();
    let mut r: int = 0;
    while r < path.count() {
        let s = path.starts[r as usize];
        let e = path.ring_end(r);
        // the ring's points without repeats
        let mut px: Vec<double> = Vec::new();
        let mut py: Vec<double> = Vec::new();
        let mut k = s;
        while k < e {
            let x = path.pts[(k * 2) as usize];
            let y = path.pts[(k * 2 + 1) as usize];
            let n = px.len();
            if n == 0 || (px[n - 1] - x).abs() > 1.0e-9 || (py[n - 1] - y).abs() > 1.0e-9 {
                px.push(x);
                py.push(y);
            }
            k += 1;
        }
        let mut closed = path.closed[r as usize];
        if closed && px.len() > 1 {
            let n = px.len();
            if (px[n - 1] - px[0]).abs() < 1.0e-9 && (py[n - 1] - py[0]).abs() < 1.0e-9 {
                px.pop();
                py.pop();
            }
        }
        if px.len() < 2 {
            // a lone point: a dot with round or square caps
            if px.len() == 1 && e - s >= 2 && cap != 0 && !dash_active(dash) {
                if cap == 1 {
                    circle(&mut out, px[0], py[0], hw);
                } else {
                    quad_ring(&mut out, px[0] - hw, py[0] - hw, px[0] + hw, py[0] - hw, px[0] + hw, py[0] + hw, px[0] - hw, py[0] + hw);
                }
            }
            r += 1;
            continue;
        }
        if closed && px.len() < 3 {
            closed = false;
        }
        if dash_active(dash) {
            if closed {
                px.push(px[0]);
                py.push(py[0]);
            }
            dashes(&mut out, &px, &py, hw, cap, join, limit, dash, dash_off);
        } else {
            polyline(&mut out, &px, &py, closed, hw, cap, join, limit);
        }
        r += 1;
    }
    out
}

fn dash_active(dash: &Vec<double>) -> bool {
    let mut sum: double = 0.0;
    for d in dash.iter() {
        sum += *d;
    }
    !dash.is_empty() && sum > 0.0
}

/// The dashes of an open polyline, each stroked as a line of its own.
fn dashes(out: &mut Rings, px: &Vec<double>, py: &Vec<double>, hw: double, cap: int, join: int, limit: double, dash: &Vec<double>, off: double) {
    let mut total: double = 0.0;
    for d in dash.iter() {
        total += *d;
    }
    let mut phase = off % total;
    if phase < 0.0 {
        phase += total;
    }
    let mut di: usize = 0;
    while phase >= dash[di] {
        phase -= dash[di];
        di = (di + 1) % dash.len();
    }
    let mut left = dash[di] - phase;
    let mut on = di % 2 == 0;
    let mut cx: Vec<double> = Vec::new();
    let mut cy: Vec<double> = Vec::new();
    if on {
        cx.push(px[0]);
        cy.push(py[0]);
    }
    let mut i: usize = 0;
    while i + 1 < px.len() {
        let mut ax = px[i];
        let mut ay = py[i];
        let bx = px[i + 1];
        let by = py[i + 1];
        let mut len = ((bx - ax) * (bx - ax) + (by - ay) * (by - ay)).sqrt();
        while len > left {
            let t = left / len;
            let mx = ax + (bx - ax) * t;
            let my = ay + (by - ay) * t;
            if on {
                cx.push(mx);
                cy.push(my);
                polyline(out, &cx, &cy, false, hw, cap, join, limit);
                cx = Vec::new();
                cy = Vec::new();
            } else {
                cx = vec![mx];
                cy = vec![my];
            }
            on = !on;
            len -= left;
            ax = mx;
            ay = my;
            di = (di + 1) % dash.len();
            left = dash[di];
        }
        left -= len;
        if on {
            cx.push(bx);
            cy.push(by);
        }
        i += 1;
    }
    if on && cx.len() > 1 {
        polyline(out, &cx, &cy, false, hw, cap, join, limit);
    }
}

/// One polyline's outline: a quad per segment, a join at each corner,
/// caps at the ends of an open one.
fn polyline(out: &mut Rings, px: &Vec<double>, py: &Vec<double>, closed: bool, hw: double, cap: int, join: int, limit: double) {
    let n = px.len();
    if n < 2 {
        return;
    }
    let segs = if closed { n } else { n - 1 };
    let mut i: usize = 0;
    while i < segs {
        let j = (i + 1) % n;
        let dx = px[j] - px[i];
        let dy = py[j] - py[i];
        let len = (dx * dx + dy * dy).sqrt();
        if len > 0.0 {
            let nx = -dy / len * hw;
            let ny = dx / len * hw;
            quad_ring(out, px[i] + nx, py[i] + ny, px[j] + nx, py[j] + ny, px[j] - nx, py[j] - ny, px[i] - nx, py[i] - ny);
        }
        i += 1;
    }
    // joins
    let mut v: usize = if closed { 0 } else { 1 };
    let last = if closed { n } else { n - 1 };
    while v < last {
        let p = (v + n - 1) % n;
        let q = (v + 1) % n;
        corner(out, px[p], py[p], px[v], py[v], px[q], py[q], hw, join, limit);
        v += 1;
    }
    if !closed {
        end_cap(out, px[1], py[1], px[0], py[0], hw, cap);
        end_cap(out, px[n - 2], py[n - 2], px[n - 1], py[n - 1], hw, cap);
    }
}

/// The join at (vx, vy) between the segments from (ax, ay) and to (bx, by).
fn corner(out: &mut Rings, ax: double, ay: double, vx: double, vy: double, bx: double, by: double, hw: double, join: int, limit: double) {
    let d1x = vx - ax;
    let d1y = vy - ay;
    let d2x = bx - vx;
    let d2y = by - vy;
    let l1 = (d1x * d1x + d1y * d1y).sqrt();
    let l2 = (d2x * d2x + d2y * d2y).sqrt();
    if l1 == 0.0 || l2 == 0.0 {
        return;
    }
    // a corner that barely turns (a flattened curve): the gap a join
    // fills is under a tenth of a pixel, a triangle does
    let turn = (d1x * d2x + d1y * d2y) / (l1 * l2);
    let slight = turn > 0.0 && hw * (2.0 - 2.0 * turn).sqrt() < 0.1;
    if join == 1 && !slight {
        circle(out, vx, vy, hw);
        return;
    }
    let n1x = -d1y / l1 * hw;
    let n1y = d1x / l1 * hw;
    let n2x = -d2y / l2 * hw;
    let n2y = d2x / l2 * hw;
    // the outer side: away from where the line turns
    let side = if n1x * d2x + n1y * d2y > 0.0 { -1.0 } else { 1.0 };
    let ox1 = vx + side * n1x;
    let oy1 = vy + side * n1y;
    let ox2 = vx + side * n2x;
    let oy2 = vy + side * n2y;
    if join == 0 && !slight {
        // the miter's tip: where the two outer edges meet
        let cross = d1x * d2y - d1y * d2x;
        if cross.abs() > 1.0e-12 {
            let t = ((ox2 - ox1) * d2y - (oy2 - oy1) * d2x) / cross;
            let mx = ox1 + d1x * t;
            let my = oy1 + d1y * t;
            let ml = ((mx - vx) * (mx - vx) + (my - vy) * (my - vy)).sqrt();
            if ml <= limit * hw {
                quad_ring(out, vx, vy, ox1, oy1, mx, my, ox2, oy2);
                return;
            }
        }
    }
    tri_ring(out, vx, vy, ox1, oy1, ox2, oy2);
}

/// A cap at the end (ex, ey) of the line coming from (fx, fy).
fn end_cap(out: &mut Rings, fx: double, fy: double, ex: double, ey: double, hw: double, cap: int) {
    if cap == 1 {
        circle(out, ex, ey, hw);
        return;
    }
    if cap == 2 {
        let dx = ex - fx;
        let dy = ey - fy;
        let l = (dx * dx + dy * dy).sqrt();
        if l == 0.0 {
            return;
        }
        let ux = dx / l * hw;
        let uy = dy / l * hw;
        let nx = -uy;
        let ny = ux;
        quad_ring(out, ex + nx, ey + ny, ex + nx + ux, ey + ny + uy, ex - nx + ux, ey - ny + uy, ex - nx, ey - ny);
    }
}

fn circle(out: &mut Rings, cx: double, cy: double, r: double) {
    let mut n = (r * 2.0).ceil() as int + 8;
    if n > 96 {
        n = 96;
    }
    let mut k: int = 0;
    let mut first = true;
    while k < n {
        let a = 6.283185307179586 * (k as double) / (n as double);
        // counter-clockwise on screen, as quad_ring makes rings
        let x = cx + r * a.cos();
        let y = cy - r * a.sin();
        if first {
            out.start_ring(x, y);
            first = false;
        } else {
            out.add(x, y);
        }
        k += 1;
    }
    let s = out.starts[out.starts.len() - 1];
    orient(out, s);
}

fn quad_ring(out: &mut Rings, x1: double, y1: double, x2: double, y2: double, x3: double, y3: double, x4: double, y4: double) {
    out.start_ring(x1, y1);
    let s = out.starts[out.starts.len() - 1];
    out.add(x2, y2);
    out.add(x3, y3);
    out.add(x4, y4);
    orient(out, s);
}

fn tri_ring(out: &mut Rings, x1: double, y1: double, x2: double, y2: double, x3: double, y3: double) {
    out.start_ring(x1, y1);
    let s = out.starts[out.starts.len() - 1];
    out.add(x2, y2);
    out.add(x3, y3);
    orient(out, s);
}

/// Turns the last ring (from point `s`) to positive area if it is not.
fn orient(out: &mut Rings, s: int) {
    let e = (out.pts.len() / 2) as int;
    let mut area: double = 0.0;
    let mut k = s;
    while k < e {
        let j = if k + 1 < e { k + 1 } else { s };
        area += out.pts[(k * 2) as usize] * out.pts[(j * 2 + 1) as usize] - out.pts[(j * 2) as usize] * out.pts[(k * 2 + 1) as usize];
        k += 1;
    }
    if area < 0.0 {
        let mut a = s;
        let mut b = e - 1;
        while a < b {
            let ax = out.pts[(a * 2) as usize];
            let ay = out.pts[(a * 2 + 1) as usize];
            out.pts[(a * 2) as usize] = out.pts[(b * 2) as usize];
            out.pts[(a * 2 + 1) as usize] = out.pts[(b * 2 + 1) as usize];
            out.pts[(b * 2) as usize] = ax;
            out.pts[(b * 2 + 1) as usize] = ay;
            a += 1;
            b -= 1;
        }
    }
}

// ---- paint and composition

/// Paints the coverage `cov` with the pen onto the surface.
fn paint(px: &mut Vec<double>, w: int, h: int, cov: &Coverage, clip: &Vec<double>, pen: &Pen, images: &Vec<RasterImage>) {
    let has_clip = clip.len() as int == w * h;
    let mut src: Vec<double> = vec![0.0, 0.0, 0.0, 0.0];
    if pen.kind == RC_SOLID {
        let a = clamp01(pen.color[3]);
        src = vec![clamp01(pen.color[0]) * a, clamp01(pen.color[1]) * a, clamp01(pen.color[2]) * a, a];
    }
    let mut ly: int = 0;
    while ly < cov.h {
        let y = ly + cov.y0;
        let mut lx: int = 0;
        while lx < cov.w {
            let x = lx + cov.x0;
            let mut f = cov.cov[(ly * cov.w + lx) as usize];
            if f > 0.0 && has_clip {
                f = f * clip[(y * w + x) as usize];
            }
            if f > 0.0 {
                if pen.kind != RC_SOLID {
                    src = paint_at(pen, (x as double) + 0.5, (y as double) + 0.5, images);
                }
                let k = ((y * w + x) * 4) as usize;
                composite(px, k, &src, f, pen.op);
            }
            lx += 1;
        }
        ly += 1;
    }
}

fn clamp01(v: double) -> double {
    if v > 1.0 { 1.0 } else if v < 0.0 || v != v { 0.0 } else { v }
}

/// The pixel at k composed with `s` (premultiplied) by `op`, covered `f`.
fn composite(px: &mut Vec<double>, k: usize, s: &Vec<double>, f: double, op: int) {
    let sa = s[3];
    let da = px[k + 3];
    // the factors of Porter and Duff: result = s * fa + d * fb
    let mut fa: double = 1.0;
    let mut fb: double = 1.0 - sa;
    if op == OP_SOURCE_IN { fa = da; fb = 0.0; }
    else if op == OP_SOURCE_OUT { fa = 1.0 - da; fb = 0.0; }
    else if op == OP_SOURCE_ATOP { fa = da; fb = 1.0 - sa; }
    else if op == OP_DESTINATION_OVER { fa = 1.0 - da; fb = 1.0; }
    else if op == OP_DESTINATION_IN { fa = 0.0; fb = sa; }
    else if op == OP_DESTINATION_OUT { fa = 0.0; fb = 1.0 - sa; }
    else if op == OP_DESTINATION_ATOP { fa = 1.0 - da; fb = sa; }
    else if op == OP_COPY { fa = 1.0; fb = 0.0; }
    else if op == OP_XOR { fa = 1.0 - da; fb = 1.0 - sa; }
    else if op == OP_LIGHTER { fa = 1.0; fb = 1.0; }
    let mut c: usize = 0;
    while c < 4 {
        let d = px[k + c];
        let mut v = s[c] * fa + d * fb;
        if v > 1.0 { v = 1.0; }
        px[k + c] = d + (v - d) * f;
        c += 1;
    }
}

/// A gradient's or an image's colour at a device point, premultiplied.
fn paint_at(pen: &Pen, dx: double, dy: double, images: &Vec<RasterImage>) -> Vec<double> {
    let m = &pen.m;
    let ux = m[0] * dx + m[2] * dy + m[4];
    let uy = m[1] * dx + m[3] * dy + m[5];
    if pen.kind == RC_IMAGE {
        return sample(pen, ux, uy, images);
    }
    let g = &pen.geo;
    let t: double;
    if pen.kind == RC_LINEAR {
        let vx = g[2] - g[0];
        let vy = g[3] - g[1];
        let l2 = vx * vx + vy * vy;
        if l2 == 0.0 {
            return vec![0.0, 0.0, 0.0, 0.0];
        }
        t = ((ux - g[0]) * vx + (uy - g[1]) * vy) / l2;
    } else {
        // two circles: the largest t whose circle passes through the point
        let cdx = g[3] - g[0];
        let cdy = g[4] - g[1];
        let dr = g[5] - g[2];
        let pdx = ux - g[0];
        let pdy = uy - g[1];
        let a = cdx * cdx + cdy * cdy - dr * dr;
        let b = pdx * cdx + pdy * cdy + g[2] * dr;
        let c = pdx * pdx + pdy * pdy - g[2] * g[2];
        if a.abs() < 1.0e-12 {
            if b == 0.0 {
                return vec![0.0, 0.0, 0.0, 0.0];
            }
            t = c / (2.0 * b);
        } else {
            let disc = b * b - a * c;
            if disc < 0.0 {
                return vec![0.0, 0.0, 0.0, 0.0];
            }
            let sq = disc.sqrt();
            let t1 = (b + sq) / a;
            if g[2] + t1 * dr >= 0.0 {
                t = t1;
            } else {
                t = (b - sq) / a;
                if g[2] + t * dr < 0.0 {
                    return vec![0.0, 0.0, 0.0, 0.0];
                }
            }
        }
    }
    stop_color(&pen.stops, t, pen.alpha)
}

fn stop_color(stops: &Vec<double>, t0: double, alpha: double) -> Vec<double> {
    let n = stops.len() / 5;
    if n == 0 {
        return vec![0.0, 0.0, 0.0, 0.0];
    }
    let t = clamp01(t0);
    let mut i: usize = 0;
    while i + 1 < n && stops[(i + 1) * 5] <= t {
        i += 1;
    }
    let mut r = stops[i * 5 + 1];
    let mut g = stops[i * 5 + 2];
    let mut b = stops[i * 5 + 3];
    let mut a = stops[i * 5 + 4];
    if i + 1 < n && t > stops[i * 5] {
        let o0 = stops[i * 5];
        let o1 = stops[(i + 1) * 5];
        let u = if o1 > o0 { (t - o0) / (o1 - o0) } else { 1.0 };
        r = r + (stops[(i + 1) * 5 + 1] - r) * u;
        g = g + (stops[(i + 1) * 5 + 2] - g) * u;
        b = b + (stops[(i + 1) * 5 + 3] - b) * u;
        a = a + (stops[(i + 1) * 5 + 4] - a) * u;
    } else if t < stops[0] {
        r = stops[1];
        g = stops[2];
        b = stops[3];
        a = stops[4];
    }
    let aa = clamp01(a) * clamp01(alpha);
    vec![clamp01(r) * aa, clamp01(g) * aa, clamp01(b) * aa, aa]
}

/// An image's colour at its own pixel coordinates (x, y), premultiplied.
fn sample(pen: &Pen, x: double, y: double, images: &Vec<RasterImage>) -> Vec<double> {
    if pen.image < 0 || pen.image >= images.len() as int {
        return vec![0.0, 0.0, 0.0, 0.0];
    }
    let img = &images[pen.image as usize];
    if img.w <= 0 || img.h <= 0 {
        return vec![0.0, 0.0, 0.0, 0.0];
    }
    let mut out: Vec<double> = vec![0.0, 0.0, 0.0, 0.0];
    if !pen.smooth {
        let ix = x.floor() as int;
        let iy = y.floor() as int;
        texel(img, ix, iy, pen.rep_x, pen.rep_y, 1.0, &mut out);
    } else {
        let fx = x - 0.5;
        let fy = y - 0.5;
        let ix = fx.floor() as int;
        let iy = fy.floor() as int;
        let tx = fx - (ix as double);
        let ty = fy - (iy as double);
        texel(img, ix, iy, pen.rep_x, pen.rep_y, (1.0 - tx) * (1.0 - ty), &mut out);
        texel(img, ix + 1, iy, pen.rep_x, pen.rep_y, tx * (1.0 - ty), &mut out);
        texel(img, ix, iy + 1, pen.rep_x, pen.rep_y, (1.0 - tx) * ty, &mut out);
        texel(img, ix + 1, iy + 1, pen.rep_x, pen.rep_y, tx * ty, &mut out);
    }
    let mut c: usize = 0;
    while c < 4 {
        out[c] = out[c] * clamp01(pen.alpha);
        c += 1;
    }
    out
}

fn texel(img: &RasterImage, x0: int, y0: int, rep_x: bool, rep_y: bool, wgt: double, out: &mut Vec<double>) {
    if wgt == 0.0 {
        return;
    }
    let mut x = x0;
    let mut y = y0;
    if rep_x {
        x = ((x % img.w) + img.w) % img.w;
    } else if x < 0 || x >= img.w {
        return;
    }
    if rep_y {
        y = ((y % img.h) + img.h) % img.h;
    } else if y < 0 || y >= img.h {
        return;
    }
    let k = ((y * img.w + x) * 4) as usize;
    let mut c: usize = 0;
    while c < 4 {
        out[c] += img.px[k + c] * wgt;
        c += 1;
    }
}

// ---- reading and writing pixels

/// The surface's pixels in the rectangle as 8-bit RGBA, not premultiplied
/// (getImageData): outside the surface, transparent black.
pub fn read_rgba(px: &Vec<double>, w: int, h: int, rx: int, ry: int, rw: int, rh: int) -> Vec<double> {
    let mut out: Vec<double> = Vec::with_capacity((rw * rh * 4) as usize);
    let mut y: int = 0;
    while y < rh {
        let sy = ry + y;
        let mut x: int = 0;
        while x < rw {
            let sx = rx + x;
            if sx < 0 || sy < 0 || sx >= w || sy >= h {
                out.push(0.0); out.push(0.0); out.push(0.0); out.push(0.0);
            } else {
                let k = ((sy * w + sx) * 4) as usize;
                let a = px[k + 3];
                if a <= 0.0 {
                    out.push(0.0); out.push(0.0); out.push(0.0); out.push(0.0);
                } else {
                    out.push(byte_of(px[k] / a));
                    out.push(byte_of(px[k + 1] / a));
                    out.push(byte_of(px[k + 2] / a));
                    out.push(byte_of(a));
                }
            }
            x += 1;
        }
        y += 1;
    }
    out
}

fn byte_of(v: double) -> double {
    (clamp01(v) * 255.0 + 0.5).floor()
}

/// 8-bit RGBA (not premultiplied) written over the rectangle at (rx, ry),
/// replacing what is there (putImageData).
pub fn write_rgba(px: &mut Vec<double>, w: int, h: int, rx: int, ry: int, rw: int, rh: int, src: &Vec<double>) {
    let mut y: int = 0;
    while y < rh {
        let dy = ry + y;
        let mut x: int = 0;
        while x < rw {
            let dx = rx + x;
            let s = ((y * rw + x) * 4) as usize;
            if dx >= 0 && dy >= 0 && dx < w && dy < h && s + 3 < src.len() {
                let k = ((dy * w + dx) * 4) as usize;
                let a = clamp01(src[s + 3] / 255.0);
                px[k] = clamp01(src[s] / 255.0) * a;
                px[k + 1] = clamp01(src[s + 1] / 255.0) * a;
                px[k + 2] = clamp01(src[s + 2] / 255.0) * a;
                px[k + 3] = a;
            }
            x += 1;
        }
        y += 1;
    }
}

// ---- PNG

/// The surface as a PNG file (8-bit RGBA, each row filtered, deflate
/// with the fixed codes) in a data: URL, for a renderer to show as a
/// picture.
pub fn png_data_url(px: &Vec<double>, w: int, h: int) -> String {
    let rgba = read_rgba(px, w, h, 0, 0, w, h);
    let stride = w * 4;
    let mut raw: Vec<int> = Vec::with_capacity(((stride + 1) * h) as usize);
    let mut y: int = 0;
    while y < h {
        // the filter whose bytes are smallest (as signed): none, sub, up, paeth
        let mut best: int = 0;
        let mut best_sum: int = -1;
        let mut f: int = 0;
        while f < 4 {
            if f >= 2 && y == 0 {
                break;
            }
            let mut sum: int = 0;
            let mut x: int = 0;
            while x < stride {
                let v = filtered(&rgba, stride, x, y, f);
                sum += if v < 128 { v } else { 256 - v };
                x += 1;
            }
            if best_sum < 0 || sum < best_sum {
                best_sum = sum;
                best = f;
            }
            f += 1;
        }
        // PNG numbers paeth 4 (3 is average)
        raw.push(if best == 3 { 4 } else { best });
        let mut x: int = 0;
        while x < stride {
            raw.push(filtered(&rgba, stride, x, y, best));
            x += 1;
        }
        y += 1;
    }
    let mut z: Vec<int> = vec![0x78, 0x01];
    deflate(&raw, &mut z);
    let mut a: int = 1;
    let mut b: int = 0;
    for v in raw.iter() {
        a = (a + *v) % 65521;
        b = (b + a) % 65521;
    }
    push32(&mut z, b * 65536 + a);
    let mut file: Vec<int> = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr: Vec<int> = Vec::new();
    push32(&mut ihdr, w);
    push32(&mut ihdr, h);
    ihdr.push(8);
    ihdr.push(6);
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    chunk(&mut file, "IHDR", &ihdr);
    chunk(&mut file, "IDAT", &z);
    chunk(&mut file, "IEND", &Vec::new());
    let mut out = String::from("data:image/png;base64,");
    base64(&file, &mut out);
    out
}

/// Byte x of row y with PNG filter f (0 none, 1 sub, 2 up, 3 paeth).
fn filtered(rgba: &Vec<double>, stride: int, x: int, y: int, f: int) -> int {
    let k = (y * stride + x) as usize;
    let v = rgba[k] as int;
    if f == 0 {
        return v;
    }
    let left = if x >= 4 { rgba[k - 4] as int } else { 0 };
    let up = if y > 0 { rgba[k - stride as usize] as int } else { 0 };
    if f == 1 {
        return (v - left + 256) & 255;
    }
    if f == 2 {
        return (v - up + 256) & 255;
    }
    let ul = if x >= 4 && y > 0 { rgba[k - stride as usize - 4] as int } else { 0 };
    let p = left + up - ul;
    let pa = if p > left { p - left } else { left - p };
    let pb = if p > up { p - up } else { up - p };
    let pc = if p > ul { p - ul } else { ul - p };
    let pred = if pa <= pb && pa <= pc { left } else if pb <= pc { up } else { ul };
    (v - pred + 256) & 255
}

/// Bits written least significant first, as deflate packs them.
struct BitOut {
    acc: int,
    n: int,
}

fn put_bits(out: &mut Vec<int>, bo: &mut BitOut, v: int, n: int) {
    bo.acc = bo.acc | (v << bo.n);
    bo.n += n;
    while bo.n >= 8 {
        out.push(bo.acc & 255);
        bo.acc = bo.acc >> 8;
        bo.n -= 8;
    }
}

/// A Huffman code, which deflate writes most significant bit first.
fn put_code(out: &mut Vec<int>, bo: &mut BitOut, code: int, len: int) {
    let mut r: int = 0;
    let mut i: int = 0;
    while i < len {
        r = (r << 1) | ((code >> i) & 1);
        i += 1;
    }
    put_bits(out, bo, r, len);
}

fn put_symbol(out: &mut Vec<int>, bo: &mut BitOut, sym: int) {
    if sym < 144 {
        put_code(out, bo, 0x30 + sym, 8);
    } else if sym < 256 {
        put_code(out, bo, 0x190 + sym - 144, 9);
    } else if sym < 280 {
        put_code(out, bo, sym - 256, 7);
    } else {
        put_code(out, bo, 0xc0 + sym - 280, 8);
    }
}

/// `data` compressed as one deflate block with the fixed codes, matches
/// found through a hash of three bytes (a few earlier places each).
fn deflate(data: &Vec<int>, out: &mut Vec<int>) {
    let len_base: Vec<int> = vec![3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
    let len_extra: Vec<int> = vec![0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
    let dist_base: Vec<int> = vec![1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
    let dist_extra: Vec<int> = vec![0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
    let n = data.len() as int;
    let mut bo = BitOut { acc: 0, n: 0 };
    put_bits(out, &mut bo, 1, 1);
    put_bits(out, &mut bo, 1, 2);
    let mut head: Vec<int> = vec![-1; 32768];
    let mut prev: Vec<int> = vec![-1; n as usize];
    let mut i: int = 0;
    while i < n {
        let mut best_len: int = 0;
        let mut best_dist: int = 0;
        if i + 2 < n {
            let hsh = ((data[i as usize] << 7) ^ (data[(i + 1) as usize] << 4) ^ data[(i + 2) as usize]) & 32767;
            let mut cand = head[hsh as usize];
            let mut tries: int = 0;
            while cand >= 0 && i - cand <= 32768 && tries < 8 {
                let mut l: int = 0;
                while l < 258 && i + l < n && data[(cand + l) as usize] == data[(i + l) as usize] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - cand;
                    if l == 258 {
                        break;
                    }
                }
                cand = prev[cand as usize];
                tries += 1;
            }
            prev[i as usize] = head[hsh as usize];
            head[hsh as usize] = i;
        }
        if best_len >= 3 {
            let mut c: int = 28;
            while len_base[c as usize] > best_len {
                c -= 1;
            }
            put_symbol(out, &mut bo, 257 + c);
            put_bits(out, &mut bo, best_len - len_base[c as usize], len_extra[c as usize]);
            let mut d: int = 29;
            while dist_base[d as usize] > best_dist {
                d -= 1;
            }
            put_code(out, &mut bo, d, 5);
            put_bits(out, &mut bo, best_dist - dist_base[d as usize], dist_extra[d as usize]);
            // the places inside the match go into the hash too
            let end = i + best_len;
            i += 1;
            while i < end {
                if i + 2 < n {
                    let hsh = ((data[i as usize] << 7) ^ (data[(i + 1) as usize] << 4) ^ data[(i + 2) as usize]) & 32767;
                    prev[i as usize] = head[hsh as usize];
                    head[hsh as usize] = i;
                }
                i += 1;
            }
        } else {
            put_symbol(out, &mut bo, data[i as usize]);
            i += 1;
        }
    }
    put_symbol(out, &mut bo, 256);
    if bo.n > 0 {
        out.push(bo.acc & 255);
    }
}

fn push32(out: &mut Vec<int>, v: int) {
    out.push((v >> 24) & 255);
    out.push((v >> 16) & 255);
    out.push((v >> 8) & 255);
    out.push(v & 255);
}

fn chunk(out: &mut Vec<int>, kind: &str, data: &Vec<int>) {
    push32(out, data.len() as int);
    let mut body: Vec<int> = Vec::new();
    for c in kind.chars() {
        body.push(c as int);
    }
    for v in data.iter() {
        body.push(*v);
    }
    for v in body.iter() {
        out.push(*v);
    }
    push32(out, crc32(&body));
}

fn crc32(data: &Vec<int>) -> int {
    let mut table: Vec<int> = Vec::with_capacity(256);
    let mut t: int = 0;
    while t < 256 {
        let mut c = t;
        let mut k: int = 0;
        while k < 8 {
            c = if (c & 1) != 0 { (c >> 1) ^ 0xedb88320 } else { c >> 1 };
            k += 1;
        }
        table.push(c);
        t += 1;
    }
    let mut c: int = 0xffffffff;
    for v in data.iter() {
        c = table[((c ^ *v) & 255) as usize] ^ (c >> 8);
    }
    (c ^ 0xffffffff) & 0xffffffff
}

fn base64(data: &Vec<int>, out: &mut String) {
    let mut i: usize = 0;
    let n = data.len();
    while i < n {
        let b0 = data[i];
        let b1 = if i + 1 < n { data[i + 1] } else { 0 };
        let b2 = if i + 2 < n { data[i + 2] } else { 0 };
        out.push(b64(b0 >> 2));
        out.push(b64(((b0 & 3) << 4) | (b1 >> 4)));
        out.push(if i + 1 < n { b64(((b1 & 15) << 2) | (b2 >> 6)) } else { '=' });
        out.push(if i + 2 < n { b64(b2 & 63) } else { '=' });
        i += 3;
    }
}

fn b64(v: int) -> char {
    let code: int = if v < 26 {
        65 + v
    } else if v < 52 {
        71 + v
    } else if v < 62 {
        v - 4
    } else if v == 62 {
        43
    } else {
        47
    };
    (code as u8) as char
}

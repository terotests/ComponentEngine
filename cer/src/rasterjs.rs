// SPDX-License-Identifier: AGPL-3.0-or-later
//! `__cerRaster`: the rasterizer (raster.rs) for a script's 2D canvas. The
//! script keeps the surface itself, a Float64Array of premultiplied RGBA
//! (w * h * 4), so it lives in the heap like any value (snapshots keep
//! it); the engine only runs commands on it:
//!
//!   __cerRaster(0, surface, w, h, commands, clip?, images?)   draw (raster.rs's command list, a Float64Array);
//!                     clip a Float64Array of w * h or undefined; images an array of [surface, w, h]
//!   __cerRaster(1, clip, w, h, commands, rule)                narrow the clip mask to a path
//!   __cerRaster(2, surface, w, h, x, y, rw, rh, out)          getImageData into a Uint8ClampedArray
//!   __cerRaster(3, surface, w, h, x, y, rw, rh, data)         putImageData from a Uint8ClampedArray
//!   __cerRaster(4, surface, w, h)                             the surface as a PNG data: URL

use ranger::prelude::*;

use crate::raster::*;
use crate::typed::*;
use crate::value::*;
use crate::vm::*;

pub const NF_RASTER: int = 970;

fn number(v: &Val) -> double {
    match v {
        Val::Num(n) => *n,
        _ => 0.0,
    }
}

impl Vm {
    pub fn setup_raster(&mut self) {
        let g = self.global;
        self.method(g, "__cerRaster", NF_RASTER, 9);
    }

    /// The buffer of a whole Float64Array that holds its doubles as they
    /// are (the surface and command arrays), or -1.
    fn f64_buffer(&mut self, v: &Val) -> int {
        let o = obj_of(v);
        if o < 0 || self.objs[o as usize].class != C_TYPED || self.objs[o as usize].pos != TK_FLOAT64 || self.objs[o as usize].func != 0 {
            return -1;
        }
        let b = self.objs[o as usize].env;
        if b < 0 || (self.objs[b as usize].pos & 1) != 0 {
            return -1;
        }
        let len = self.ta_length(o);
        if self.objs[b as usize].home < 0 && (self.objs[b as usize].nums.len() as int) == len * 8 {
            // nothing written yet: all zero, as elements
            let mut z: Vec<double> = Vec::with_capacity(len as usize);
            let mut i: int = 0;
            while i < len {
                z.push(0.0);
                i += 1;
            }
            self.objs[b as usize].nums = z;
            self.objs[b as usize].home = TK_FLOAT64;
        }
        if self.objs[b as usize].home != TK_FLOAT64 || (self.objs[b as usize].nums.len() as int) != len {
            return -1;
        }
        b
    }

    fn f64_values(&mut self, v: &Val) -> Vec<double> {
        let b = self.f64_buffer(v);
        if b < 0 {
            return Vec::new();
        }
        self.objs[b as usize].nums.clone()
    }

    pub fn call_native_raster(&mut self, args: Vec<Val>) -> Val {
        let mut a: Vec<Val> = args;
        while a.len() < 9 {
            a.push(Val::Undef);
        }
        let op = number(&a[0]) as int;
        let w = number(&a[2]) as int;
        let h = number(&a[3]) as int;
        let b = self.f64_buffer(&a[1]);
        let need = if op == 1 { w * h } else { w * h * 4 };
        if b < 0 || w < 0 || h < 0 || (self.objs[b as usize].nums.len() as int) < need {
            self.throw_type("__cerRaster: not a Float64Array of the surface's size");
            return Val::Undef;
        }
        if op == 0 {
            let cmds = self.f64_values(&a[4]);
            let clip = self.f64_values(&a[5]);
            let mut images: Vec<RasterImage> = Vec::new();
            let list = obj_of(&a[6]);
            if list >= 0 && self.objs[list as usize].class == C_ARRAY {
                let items = self.objs[list as usize].elems.clone();
                for it in items.iter() {
                    let io = obj_of(it);
                    let mut img = RasterImage { w: 0, h: 0, px: Vec::new() };
                    if io >= 0 && self.objs[io as usize].class == C_ARRAY && self.objs[io as usize].elems.len() >= 3 {
                        let parts = self.objs[io as usize].elems.clone();
                        let iw = number(&parts[1]) as int;
                        let ih = number(&parts[2]) as int;
                        let px = self.f64_values(&parts[0]);
                        if (px.len() as int) >= iw * ih * 4 {
                            img = RasterImage { w: iw, h: ih, px: px };
                        }
                    }
                    images.push(img);
                }
            }
            crate::raster::run(&mut self.objs[b as usize].nums, w, h, &cmds, &clip, &images);
            return Val::Undef;
        }
        if op == 1 {
            let cmds = self.f64_values(&a[4]);
            let rule = number(&a[5]) as int;
            crate::raster::clip_to(&mut self.objs[b as usize].nums, w, h, &cmds, rule);
            return Val::Undef;
        }
        if op == 2 || op == 3 {
            let x = number(&a[4]) as int;
            let y = number(&a[5]) as int;
            let rw = number(&a[6]) as int;
            let rh = number(&a[7]) as int;
            let t = obj_of(&a[8]);
            if t < 0 || self.objs[t as usize].class != C_TYPED || rw < 0 || rh < 0 || self.ta_length(t) < rw * rh * 4 {
                self.throw_type("__cerRaster: not a Uint8ClampedArray of the rectangle's size");
                return Val::Undef;
            }
            let n = rw * rh * 4;
            if op == 2 {
                let rgba = crate::raster::read_rgba(&self.objs[b as usize].nums, w, h, x, y, rw, rh);
                let mut i: int = 0;
                while i < n {
                    self.ta_set(t, i, &Val::Num(rgba[i as usize]));
                    i += 1;
                }
            } else {
                let mut src: Vec<double> = Vec::with_capacity(n as usize);
                let mut i: int = 0;
                while i < n {
                    let v = self.ta_get(t, i);
                    src.push(number(&v));
                    i += 1;
                }
                crate::raster::write_rgba(&mut self.objs[b as usize].nums, w, h, x, y, rw, rh, &src);
            }
            return Val::Undef;
        }
        if op == 4 {
            let url = crate::raster::png_data_url(&self.objs[b as usize].nums, w, h);
            return str_val(url.as_str());
        }
        Val::Undef
    }
}

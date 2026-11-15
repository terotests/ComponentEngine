// SPDX-License-Identifier: AGPL-3.0-or-later
// A baseline JIT that writes a function's register code (regs.rs) as a
// WebAssembly module. In a browser (or any WebAssembly host) the host
// compiles the module with its own engine, puts the function into this
// module's function table, and the register machine calls it at a loop's
// back edge. Rust only, behind the `wjit` feature on wasm32.
//
// The compiled function works on the frame as the interpreter does: the
// registers stay in Vm.stack, a constant is read from Vm.kvals, and the
// frame's layout is unchanged. That is why there is no deoptimization:
// whenever an op is one this file does not write, or its operands are not
// of the kind the fast path handles (a string in an addition, an object in
// a comparison), the function returns the op's pc and the interpreter runs
// it from there. The function never calls, allocates or throws.
//
//   f(sp, kv, pc, budget) -> pc to go on at in the interpreter
//
// sp is the address of stack[bp], kv of kvals[0], pc where to start (any
// pc that begins a block; any other pc returns at once). budget counts the
// back edges it may take before it returns (time slices, rvm.rs); a return
// because the budget ran out comes back as -1 - pc.
//
// The code is a dispatch loop over the function's blocks (a block starts at
// pc 0 and at every jump target): a br_table picks the block, a block falls
// through into the next, a jump sets the key and goes round the loop. A
// value's layout in memory (its tag byte and where the payloads lie) is
// found once by looking at real values (probe); when it is not the one this
// file expects the JIT stays off.

use ranger::prelude::*;
use crate::ops::*;
use crate::regs::*;
use crate::value::*;
use crate::vm::{A_LENGTH, A_NAME, A_PROTOTYPE};

// ---------------------------------------------------------------- layout

/// a value's size in memory (value.rs Val: 8 bytes, NaN-boxed)
pub const VS: u32 = 8;
const VSU: usize = VS as usize;

/// Where the parts of a Val lie in memory.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub ok: bool,
    pub size: u32,
    pub t_undef: u32,
    pub t_null: u32,
    pub t_bool: u32,
    pub t_num: u32,
    pub t_str: u32,
    pub t_obj: u32,
    /// Val::Big (a small BigInt): its value is a double at off_num
    pub t_big: u32,
    /// offsets of the payloads
    pub off_bool: u32,
    pub off_num: u32,
    pub off_int: u32,
    /// a JsObj (value.rs) in Vm.objs: its size, where its shape is and
    /// where the pointer to its vals' buffer is; obj_ok when found (wasm32)
    pub obj_ok: bool,
    pub obj_size: u32,
    pub off_shape: u32,
    pub off_vals: u32,
    /// JsObj.inl0 (slot 0; inl1 follows at + 16) and JsObj.nv
    pub off_inl0: u32,
    pub off_nv: u32,
    pub off_proto: u32,
    pub off_class: u32,
    /// Vm.shape_epoch's low word, from the Vm's address
    pub off_epoch: u32,
    /// from the Vm's address: its stack's buffer and length words, the
    /// constants' buffer, the frames' buffer and length, the handlers'
    /// length, gc_flag; a Frame's size and where its this_val is
    pub vm_stack_ptr: u32,
    pub vm_stack_len: u32,
    pub vm_stack_cap: u32,
    pub vm_kvals_ptr: u32,
    pub vm_frames_ptr: u32,
    pub vm_frames_len: u32,
    pub vm_handlers_len: u32,
    pub vm_gc_flag: u32,
    pub frame_size: u32,
    pub frame_this: u32,
    /// a Frame's env (low word), a JsObj's elems, Vm.evecs' buffer word
    /// and an Elems' size and buffer word (scope slots, R_GET_ENV)
    pub frame_env: u32,
    pub off_elems: u32,
    pub vm_evecs_ptr: u32,
    pub elems_size: u32,
    pub elems_vp: u32,
    pub elems_vl: u32,
    /// JsObj.rc (the heap's references, Vm::inc/dec_o)
    pub off_rc: u32,
    /// for a property added in place (R_SET_PROP's transition): JsObj's
    /// extensible byte and its vals' length and capacity words, Vm's
    /// proxy_made byte and setter_atoms' buffer and length words
    pub off_ext: u32,
    pub off_vals_len: u32,
    pub off_vals_cap: u32,
    pub vm_proxy_made: u32,
    pub vm_setters_ptr: u32,
    pub vm_setters_len: u32,
    /// a frame pushed by compiled code (call_push): JsObj's func and env,
    /// Vm's frames capacity word and slicing byte, Frame's fields
    pub off_func: u32,
    pub off_env: u32,
    pub vm_frames_cap: u32,
    pub vm_slicing: u32,
    pub fr_proto: u32,
    pub fr_fobj: u32,
    pub fr_pc: u32,
    pub fr_bp: u32,
    pub fr_new_target: u32,
    pub fr_args_obj: u32,
    pub fr_construct: u32,
    pub fr_gen: u32,
    pub fr_ret: u32,
    pub fr_mv_from: u32,
    pub fr_mv_to: u32,
    /// JsObj's zct and free bytes; Vm.zct's words, zct_limit's low word
    pub off_zct: u32,
    pub off_free: u32,
    pub vm_zct_ptr: u32,
    pub vm_zct_len: u32,
    pub vm_zct_cap: u32,
    pub vm_zct_limit: u32,
    /// Vm.strs' and Vm.type_vals' buffer words
    pub vm_srcs_ptr: u32,
    pub vm_type_vals_ptr: u32,
    pub vm_tdz: u32,
    pub vm_string_proto: u32,
    pub vm_number_proto: u32,
    pub vm_jit_calls_ptr: u32,
    pub vm_jit_calls_len: u32,
    /// a string's bytes: *(*(sent + 0) + str_data) for SEnt.s (Rc<String>),
    /// 0 when the probe did not find them so
    pub str_data: u32,
    /// the String's byte length there, beside str_data
    pub str_blen: u32,
    pub off_pos: u32,
    pub off_cold: u32,
    pub vm_shapes_ptr: u32,
    pub shape_size: u32,
    pub sh_nkeys: u32,
    pub sh_kref: u32,
    pub sh_keys_p: u32,
    pub sh_attrs_p: u32,
    pub vm_colds_ptr: u32,
    pub cold_size: u32,
    pub cold_home: u32,
    pub vm_akinds_ptr: u32,
    pub vm_akinds_len: u32,
    pub vm_avals_ptr: u32,
    pub vm_avals_len: u32,
    /// Elems.v's capacity word, Cold.elems2's length word (Array push)
    pub elems_vc: u32,
    pub cold_e2_len: u32,
    /// Cold.prim (an arrow's this)
    pub cold_prim: u32,
    pub vm_global: u32,
    /// Cold.nums' buffer and length words (a typed array's buffer)
    pub cold_nums_p: u32,
    pub cold_nums_l: u32,
}

fn bytes_of(v: Val) -> [u8; 16] {
    // a value's 8 bytes, then 8 zero bytes
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&v.b.to_le_bytes());
    out
}

/// Looks at real values to find the layout; `ok` false when it is not a tag
/// byte at offset 0 followed by the payloads, all 16 bytes.
pub fn probe() -> Layout {
    let mut l = Layout {
        ok: false,
        size: std::mem::size_of::<Val>() as u32,
        t_undef: 0,
        t_null: 0,
        t_bool: 0,
        t_num: 0,
        t_str: 0,
        t_obj: 0,
        t_big: 0,
        off_bool: 0,
        off_num: 0,
        off_int: 0,
        obj_ok: false,
        obj_size: 0,
        off_shape: 0,
        off_vals: 0,
        off_inl0: 0,
        off_nv: 0,
        off_proto: 0,
        off_class: 0,
        off_epoch: 0,
        vm_stack_ptr: 0,
        vm_stack_len: 0,
        vm_stack_cap: 0,
        vm_kvals_ptr: 0,
        vm_frames_ptr: 0,
        vm_frames_len: 0,
        vm_handlers_len: 0,
        vm_gc_flag: 0,
        frame_size: 0,
        frame_this: 0,
        frame_env: 0,
        off_elems: 0,
        vm_evecs_ptr: 0,
        elems_size: 0,
        elems_vp: 0,
        elems_vl: 0,
        off_rc: 0,
        off_ext: 0,
        off_vals_len: 0,
        off_vals_cap: 0,
        vm_proxy_made: 0,
        vm_setters_ptr: 0,
        vm_setters_len: 0,
        off_func: 0,
        off_env: 0,
        vm_frames_cap: 0,
        vm_slicing: 0,
        fr_proto: 0,
        fr_fobj: 0,
        fr_pc: 0,
        fr_bp: 0,
        fr_new_target: 0,
        fr_args_obj: 0,
        fr_construct: 0,
        fr_gen: 0,
        fr_ret: 0,
        fr_mv_from: 0,
        fr_mv_to: 0,
        off_zct: 0,
        off_free: 0,
        vm_zct_ptr: 0,
        vm_zct_len: 0,
        vm_zct_cap: 0,
        vm_zct_limit: 0,
        vm_srcs_ptr: 0,
        vm_type_vals_ptr: 0,
        vm_tdz: 0,
        vm_string_proto: 0,
        vm_number_proto: 0,
        vm_jit_calls_ptr: 0,
        vm_jit_calls_len: 0,
        str_data: 0,
        str_blen: 0,
        off_pos: 0,
        off_cold: 0,
        vm_shapes_ptr: 0,
        shape_size: 0,
        sh_nkeys: 0,
        sh_kref: 0,
        sh_keys_p: 0,
        sh_attrs_p: 0,
        vm_colds_ptr: 0,
        cold_size: 0,
        cold_home: 0,
        vm_akinds_ptr: 0,
        vm_akinds_len: 0,
        vm_avals_ptr: 0,
        vm_avals_len: 0,
        elems_vc: 0,
        cold_e2_len: 0,
        cold_prim: 0,
        vm_global: 0,
        cold_nums_p: 0,
        cold_nums_l: 0,
    };
    if l.size != VS || std::mem::align_of::<Val>() != 8 {
        return l;
    }
    // NaN-boxed (value.rs): the tag is a value's top 16 bits, a number's
    // is taken as 0xFFF8 (tag_at); the payloads are the low bits
    l.t_undef = (crate::value::V_UNDEF >> 48) as u32;
    l.t_null = (crate::value::V_NULL >> 48) as u32;
    l.t_bool = (crate::value::V_BOOL >> 48) as u32;
    l.t_num = 0xFFF8;
    l.t_str = (crate::value::V_STR >> 48) as u32;
    l.t_obj = (crate::value::V_OBJ >> 48) as u32;
    l.t_big = (crate::value::V_BIG >> 48) as u32;
    if Val::Obj(0x1b2c_3d4e).b != crate::value::V_OBJ | 0x1b2c_3d4e || Val::Num(1.5).b != 1.5f64.to_bits() {
        return l;
    }
    l.off_bool = 0;
    l.off_num = 0;
    l.off_int = 0;
    l.ok = true;
    // the buffer pointer of a Vec<Val>: the word of the Vec that holds it
    let mut v: Vec<Val> = Vec::with_capacity(7);
    v.push(Val::Undef());
    v.push(Val::Undef());
    v.push(Val::Undef());
    let ptr = v.as_ptr() as usize;
    let mut vl: i32 = -1;
    let vs = std::mem::size_of::<Vec<Val>>();
    let ws = std::mem::size_of::<usize>();
    let mut vp: i32 = -1;
    if ws == 4 && vs == 12 {
        let mut k = 0;
        while k < vs {
            let mut w: usize = 0;
            for i in 0..ws {
                // SAFETY: a Vec is three usize words, no padding
                let b = unsafe { std::ptr::read_volatile((&v as *const Vec<Val> as *const u8).add(k + i)) };
                w |= (b as usize) << (8 * i);
            }
            if w == ptr && ptr != 7 && ptr != 3 && ptr != 0 {
                vp = k as i32;
            }
            if w == 3 {
                vl = k as i32;
            }
            k += ws;
        }
    }
    drop(v);
    if vp >= 0 && vl >= 0 && vl != vp {
        l.obj_ok = true;
        l.obj_size = std::mem::size_of::<JsObj>() as u32;
        l.off_shape = std::mem::offset_of!(JsObj, shape) as u32;
        // Slots.v is an SVec: its pointer (value 0) is all there is
        l.off_vals = std::mem::offset_of!(JsObj, vals) as u32;
        l.off_inl0 = std::mem::offset_of!(JsObj, inl0) as u32;
        l.off_nv = std::mem::offset_of!(JsObj, nv) as u32;
        assert!(std::mem::offset_of!(JsObj, inl1) == std::mem::offset_of!(JsObj, inl0) + VS as usize);
        l.off_proto = std::mem::offset_of!(JsObj, proto) as u32;
        l.off_class = std::mem::offset_of!(JsObj, class) as u32;
        l.off_epoch = std::mem::offset_of!(crate::vm::Vm, shape_epoch) as u32;
        let (vp, vl) = (vp as u32, vl as u32);
        l.vm_stack_ptr = std::mem::offset_of!(crate::vm::Vm, stack) as u32 + vp;
        l.vm_stack_len = std::mem::offset_of!(crate::vm::Vm, stack) as u32 + vl;
        l.vm_kvals_ptr = std::mem::offset_of!(crate::vm::Vm, kvals) as u32 + vp;
        l.vm_frames_ptr = std::mem::offset_of!(crate::vm::Vm, frames) as u32 + vp;
        l.vm_frames_len = std::mem::offset_of!(crate::vm::Vm, frames) as u32 + vl;
        l.vm_handlers_len = std::mem::offset_of!(crate::vm::Vm, handlers) as u32 + vl;
        l.vm_gc_flag = std::mem::offset_of!(crate::vm::Vm, gc_flag) as u32;
        l.frame_size = std::mem::size_of::<Frame>() as u32;
        l.frame_this = std::mem::offset_of!(Frame, this_val) as u32;
        l.frame_env = std::mem::offset_of!(Frame, env) as u32;
        l.off_elems = std::mem::offset_of!(JsObj, elems) as u32;
        l.vm_evecs_ptr = std::mem::offset_of!(crate::vm::Vm, evecs) as u32 + vp;
        l.elems_size = std::mem::size_of::<crate::value::Elems>() as u32;
        // Elems.v is an EVec whose first field is its Vec (evec.rs)
        l.elems_vp = std::mem::offset_of!(crate::value::Elems, v) as u32 + std::mem::offset_of!(crate::evec::EVec, inner) as u32 + vp;
        l.elems_vl = std::mem::offset_of!(crate::value::Elems, v) as u32 + std::mem::offset_of!(crate::evec::EVec, inner) as u32 + vl;
        l.off_rc = std::mem::offset_of!(JsObj, rc) as u32;
        let vc = 12 - vp - vl;
        l.off_ext = std::mem::offset_of!(JsObj, extensible) as u32;
        // in the SVec's header (vals_hdr: the pointer less 8)
        l.off_vals_len = 0;
        l.off_vals_cap = 4;
        let _ = vc;
        l.vm_proxy_made = std::mem::offset_of!(crate::vm::Vm, proxy_made) as u32;
        l.vm_setters_ptr = std::mem::offset_of!(crate::vm::Vm, setter_atoms) as u32 + vp;
        l.vm_setters_len = std::mem::offset_of!(crate::vm::Vm, setter_atoms) as u32 + vl;
        l.off_func = std::mem::offset_of!(JsObj, func) as u32;
        l.off_env = std::mem::offset_of!(JsObj, env) as u32;
        l.vm_frames_cap = std::mem::offset_of!(crate::vm::Vm, frames) as u32 + vc;
        l.vm_stack_cap = std::mem::offset_of!(crate::vm::Vm, stack) as u32 + vc;
        l.vm_slicing = std::mem::offset_of!(crate::vm::Vm, slicing) as u32;
        l.vm_global = std::mem::offset_of!(crate::vm::Vm, global) as u32;
        l.fr_proto = std::mem::offset_of!(Frame, proto) as u32;
        l.fr_fobj = std::mem::offset_of!(Frame, fobj) as u32;
        l.fr_pc = std::mem::offset_of!(Frame, pc) as u32;
        l.fr_bp = std::mem::offset_of!(Frame, bp) as u32;
        l.fr_new_target = std::mem::offset_of!(Frame, new_target) as u32;
        l.fr_args_obj = std::mem::offset_of!(Frame, args_obj) as u32;
        l.fr_construct = std::mem::offset_of!(Frame, construct) as u32;
        l.fr_gen = std::mem::offset_of!(Frame, gen) as u32;
        l.fr_ret = std::mem::offset_of!(Frame, ret) as u32;
        l.fr_mv_from = std::mem::offset_of!(Frame, mv_from) as u32;
        l.fr_mv_to = std::mem::offset_of!(Frame, mv_to) as u32;
        l.off_zct = std::mem::offset_of!(JsObj, zct) as u32;
        l.off_free = std::mem::offset_of!(JsObj, free) as u32;
        l.vm_zct_ptr = std::mem::offset_of!(crate::vm::Vm, zct) as u32 + vp;
        l.vm_zct_len = std::mem::offset_of!(crate::vm::Vm, zct) as u32 + vl;
        l.vm_zct_cap = std::mem::offset_of!(crate::vm::Vm, zct) as u32 + vc;
        l.vm_zct_limit = std::mem::offset_of!(crate::vm::Vm, zct_limit) as u32;
        l.vm_srcs_ptr = std::mem::offset_of!(crate::vm::Vm, strs) as u32 + vp;
        l.vm_type_vals_ptr = std::mem::offset_of!(crate::vm::Vm, type_vals) as u32 + vp;
        l.vm_tdz = std::mem::offset_of!(crate::vm::Vm, tdz_obj) as u32;
        l.vm_string_proto = std::mem::offset_of!(crate::vm::Vm, string_proto) as u32;
        l.vm_number_proto = std::mem::offset_of!(crate::vm::Vm, number_proto) as u32;
        l.vm_jit_calls_ptr = std::mem::offset_of!(crate::vm::Vm, jit_calls) as u32 + vp;
        l.vm_jit_calls_len = std::mem::offset_of!(crate::vm::Vm, jit_calls) as u32 + vl;
        l.str_data = str_probe(vp, vl);
        let u = bytes_of(Val::Undef());
        // SAFETY: written on the VM's thread before any code reads it
        unsafe {
            *std::ptr::addr_of_mut!(UNDEF16) = [
                i64::from_le_bytes([u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]]),
                i64::from_le_bytes([u[8], u[9], u[10], u[11], u[12], u[13], u[14], u[15]]),
            ];
        }
        l.str_blen = 8 + vl;
        l.off_pos = std::mem::offset_of!(JsObj, pos) as u32;
        l.off_cold = std::mem::offset_of!(JsObj, cold) as u32;
        l.vm_shapes_ptr = std::mem::offset_of!(crate::vm::Vm, shapes) as u32 + vp;
        l.shape_size = std::mem::size_of::<crate::value::Shape>() as u32;
        l.sh_nkeys = std::mem::offset_of!(crate::value::Shape, nkeys) as u32;
        l.sh_kref = std::mem::offset_of!(crate::value::Shape, kref) as u32;
        l.sh_keys_p = std::mem::offset_of!(crate::value::Shape, keys) as u32 + vp;
        l.sh_attrs_p = std::mem::offset_of!(crate::value::Shape, attrs) as u32 + vp;
        l.vm_colds_ptr = std::mem::offset_of!(crate::vm::Vm, colds) as u32 + vp;
        l.cold_size = std::mem::size_of::<crate::value::Cold>() as u32;
        l.cold_home = std::mem::offset_of!(crate::value::Cold, home) as u32;
        l.vm_akinds_ptr = std::mem::offset_of!(crate::vm::Vm, atom_kinds) as u32 + vp;
        l.vm_akinds_len = std::mem::offset_of!(crate::vm::Vm, atom_kinds) as u32 + vl;
        l.vm_avals_ptr = std::mem::offset_of!(crate::vm::Vm, atom_vals) as u32 + vp;
        l.vm_avals_len = std::mem::offset_of!(crate::vm::Vm, atom_vals) as u32 + vl;
        l.elems_vc = std::mem::offset_of!(crate::value::Elems, v) as u32 + (12 - vp - vl);
        l.cold_e2_len = std::mem::offset_of!(crate::value::Cold, elems2) as u32 + vl;
        l.cold_prim = std::mem::offset_of!(crate::value::Cold, prim) as u32;
        // Vec<double> is laid out as Vec<Val> is (the buffer and length
        // words found above)
        l.cold_nums_p = std::mem::offset_of!(crate::value::Cold, nums) as u32 + vp;
        l.cold_nums_l = std::mem::offset_of!(crate::value::Cold, nums) as u32 + vl;
    }
    l
}

/// Where a string's bytes are from an Rc<String> (RcBox: strong, weak,
/// the String, whose buffer pointer is where a Vec's is): checked on a
/// live one, 0 when they are not there
fn str_probe(vp: u32, vl: u32) -> u32 {
    if std::mem::size_of::<usize>() != 4 {
        return 0;
    }
    let r: std::rc::Rc<String> = std::rc::Rc::new(String::from("probe"));
    // SAFETY: an Rc is one pointer (to its RcBox); the reads stay in it
    let boxp: usize = unsafe { std::ptr::read_volatile(&r as *const std::rc::Rc<String> as *const usize) };
    let off = 8 + vp;
    let data: usize = unsafe { std::ptr::read_volatile((boxp + off as usize) as *const usize) };
    let len: usize = unsafe { std::ptr::read_volatile((boxp + 8 + vl as usize) as *const usize) };
    if data == r.as_ptr() as usize && len == r.len() {
        off
    } else {
        0
    }
}

// --------------------------------------------------------------- encoder

/// Bytes of a module being written.
pub struct WBuf {
    pub b: Vec<u8>,
}

impl WBuf {
    pub fn new() -> WBuf {
        WBuf { b: Vec::new() }
    }
    pub fn byte(&mut self, x: u8) {
        self.b.push(x);
    }
    pub fn u(&mut self, mut x: u64) {
        loop {
            let mut c = (x & 0x7f) as u8;
            x >>= 7;
            if x != 0 {
                c |= 0x80;
            }
            self.b.push(c);
            if x == 0 {
                break;
            }
        }
    }
    pub fn s(&mut self, mut x: i64) {
        loop {
            let c = (x & 0x7f) as u8;
            x >>= 7;
            let done = (x == 0 && c & 0x40 == 0) || (x == -1 && c & 0x40 != 0);
            self.b.push(if done { c } else { c | 0x80 });
            if done {
                break;
            }
        }
    }
    pub fn name(&mut self, s: &str) {
        self.u(s.len() as u64);
        self.b.extend_from_slice(s.as_bytes());
    }
    pub fn section(&mut self, id: u8, body: &WBuf) {
        self.byte(id);
        self.u(body.b.len() as u64);
        self.b.extend_from_slice(&body.b);
    }
}

// wasm opcodes this file writes
const UNREACHABLE: u8 = 0x00;
const BLOCK: u8 = 0x02;
const LOOP: u8 = 0x03;
const IF: u8 = 0x04;
const ELSE: u8 = 0x05;
const END: u8 = 0x0b;
const BR: u8 = 0x0c;
const BR_IF: u8 = 0x0d;
const BR_TABLE: u8 = 0x0e;
const RETURN: u8 = 0x0f;
const SELECT: u8 = 0x1b;
const LOCAL_GET: u8 = 0x20;
const LOCAL_SET: u8 = 0x21;
const LOCAL_TEE: u8 = 0x22;
const I32_LOAD: u8 = 0x28;
const I64_LOAD: u8 = 0x29;
const F64_LOAD: u8 = 0x2b;
const I32_LOAD8_U: u8 = 0x2d;
const I32_LOAD16_U: u8 = 0x2f;
const I32_GT_U: u8 = 0x4b;
const I64_EXTEND_I32_U: u8 = 0xad;
const I64_OR: u8 = 0x84;
const I64_SHL: u8 = 0x86;
const I64_SHR_U: u8 = 0x88;
const I64_NE: u8 = 0x52;
const I64_EQ: u8 = 0x51;
const I64_AND: u8 = 0x83;
const I32_WRAP_I64: u8 = 0xa7;
const I64_STORE: u8 = 0x37;
const F64_STORE: u8 = 0x39;
const I32_STORE8: u8 = 0x3a;
const I32_GE_S: u8 = 0x4e;
const I32_LT_U: u8 = 0x49;
const I64_EXTEND_I32_S: u8 = 0xac;
const I32_CONST: u8 = 0x41;
const F64_CONST: u8 = 0x44;
const I32_EQZ: u8 = 0x45;
const I32_EQ: u8 = 0x46;
const I32_NE: u8 = 0x47;
const I32_LT_S: u8 = 0x48;
const I32_LE_S: u8 = 0x4c;
const I32_GT_S: u8 = 0x4a;
const F64_EQ: u8 = 0x61;
const F64_NE: u8 = 0x62;
const F64_LT: u8 = 0x63;
const F64_GT: u8 = 0x64;
const F64_LE: u8 = 0x65;
const F64_GE: u8 = 0x66;
const I32_SUB: u8 = 0x6b;
const I32_AND: u8 = 0x71;
const I32_OR: u8 = 0x72;
const I32_XOR: u8 = 0x73;
const I32_SHL: u8 = 0x74;
const I32_SHR_U: u8 = 0x76;
const I32_SHR_S: u8 = 0x75;
const F64_NEG: u8 = 0x9a;
const F64_ADD: u8 = 0xa0;
const F64_SUB: u8 = 0xa1;
const F64_MUL: u8 = 0xa2;
const F64_DIV: u8 = 0xa3;
const I32_TRUNC_F64_S: u8 = 0xaa;
const F64_TRUNC: u8 = 0x9d;
const F64_ABS: u8 = 0x99;
const F64_CEIL: u8 = 0x9b;
const F64_FLOOR: u8 = 0x9c;
const F64_SQRT: u8 = 0x9f;
const F64_MIN: u8 = 0xa4;
const F64_MAX: u8 = 0xa5;
const F64_CONVERT_I32_S: u8 = 0xb7;
const F64_CONVERT_I32_U: u8 = 0xb8;

const T_I32: u8 = 0x7f;
const T_I64: u8 = 0x7e;
const T_F64: u8 = 0x7c;
const T_VOID: u8 = 0x40;

// params and locals of the compiled function
const L_SP: u32 = 0;
const L_KV: u32 = 1;
const L_KEY: u32 = 2;
const L_BUDGET: u32 = 3;
const L_VM: u32 = 4;
const L_BP: u32 = 5;
const L_OUT: u32 = 6;
const L_TAG: u32 = 7;
const L_LOOPED: u32 = 8;
const L_ENTRY: u32 = 15;
const L_C: u32 = 16;
const L_T2: u32 = 17;
const L_T3: u32 = 18;
const L_T4: u32 = 19;
const L_T5: u32 = 20;
const L_T6: u32 = 21;
// callx's own locals (after the main function's)
const L_F: u32 = 22;
const L_DST: u32 = 23;
const L_ARGC: u32 = 24;
const L_NEXT: u32 = 25;
const L_PC: u32 = 26;
const L_RI: u32 = 27;
const L_CA: u32 = 28;
const L_FB: u32 = 29;
const L_DB: u32 = 30;
/// callx: the caller's proto and frame size (CARGS 7, 8)
const L_CPI: u32 = 31;
const L_CFS: u32 = 32;
// callx: f's object (callx_body)
const L_FO: u32 = 33;
/// the compiled function's own extra locals (inline_call): the caller's
/// sp and bp while an inlined callee's body runs, and the callee's scope
const L_SSP: u32 = 22;
const L_SBP: u32 = 23;
const L_SENV: u32 = 24;
const L_A: u32 = 9; // f64
const L_B: u32 = 10; // f64
const L_Q0: u32 = 11; // i64 x4 (R_LOAD2)
const L_Q2: u32 = 13;
const NPARAMS: u32 = 6;

// the helpers the module imports (function indices; f comes after them)
const H_GET_ELEM: u32 = 0;
const H_SET_ELEM: u32 = 1;
const H_GET_PROP: u32 = 2;
const H_SET_PROP: u32 = 3;
const H_OP: u32 = 4;
const H_CALL: u32 = 5;
const H_SLOW: u32 = 6;
/// R_SWITCH tables up to this size are written as a chain of selects
const SWITCH_MAX: usize = 64;
const H_RET: u32 = 7;
const NHELPERS: u32 = 8;
/// the largest function body compiled (bytes)
const MAX_BODY: usize = 96 * 1024;
const HELPERS: [&str; 8] = ["get_elem", "set_elem", "get_prop", "set_prop", "op", "call", "slow", "ret"];
/// what a compiled function returns for "the op threw" (the frame's pc is
/// set) and "go on with the top frame" (frames changed)
pub const OUT_THROW: i32 = i32::MIN + 4;
pub const OUT_FRAMES: i32 = i32::MIN + 5;
/// "a return op": its pc in cell[2], the value in cell[12..16] (the frame
/// is still there)
pub const OUT_RET: i32 = i32::MIN + 6;
const CALL: u8 = 0x10;
const CALL_INDIRECT: u8 = 0x11;
const I64_CONST: u8 = 0x42;
const I32_STORE: u8 = 0x36;
const I32_GE_U: u8 = 0x4f;
const I32_LE_U: u8 = 0x4d;
const I32_REM_S: u8 = 0x6f;
const I32_ADD: u8 = 0x6a;
const I32_MUL: u8 = 0x6c;

// --------------------------------------------------------------- compiler

impl Opnd {
    fn is_k(&self) -> bool {
        matches!(self, Opnd::K(_))
    }
}

/// What an operand register is at compile time.
#[derive(Clone, Copy)]
enum Opnd {
    /// a frame slot
    Reg(u32),
    /// a constant number
    Num(f64),
    /// another constant: kvals[i]
    K(u32),
}

struct Cg<'a> {
    w: WBuf,
    lay: Layout,
    kvals: &'a [Val],
    /// Vm.rtab (R_SWITCH's tables)
    rtab: &'a [int],
    /// enclosing labels above the current block's code: depth to the exit
    /// block is `exit_d + extra`, to the dispatch loop one more
    exit_d: u32,
    extra: u32,
    /// back edges compiled (a function without one is not worth it)
    backs: u32,
    /// ops written as code rather than as an exit
    done: u32,
    /// exits by op kind
    exits: [u32; 64],
    /// the function's proto and where its code is in rcode
    pi: u32,
    rb: u32,
    /// inside an op's fast path: `extra` at its slow block, which a failed
    /// check branches to (the interpreter's slow path through H_SLOW)
    slow_at: Option<u32>,
    /// by pc: the address of a property site's cache (prop_cache), or 0
    caches: Vec<u32>,
    /// by pc * 4: a method or property one prototype up that the
    /// interpreter's site cache saw when this was compiled (site_pics):
    /// receiver shape, prototype, its shape, slot; -1 for none
    pics: Vec<i32>,
    /// the function's frame size (Proto.fsize)
    fsize: u32,
    /// by pc: a property write's atom, -1 for none; R_INIT_PROP's
    /// transition (before * 65536 + after) when this was compiled
    atoms: Vec<i32>,
    /// a plain call went through callx: the module has it
    callx: bool,
    /// debug: the kind of the op being written
    cur_kind: u32,
    /// by pc: the callee a plain call's site always called, when its body
    /// can be written in place of the call (inline_ok)
    inl: Vec<Option<Inl>>,
    /// writing an inlined callee's body: its checks fail to the normal
    /// call, and what cannot be written that way (a helper, an exit)
    /// sets inl_bad, which drops the inlined code again
    inlining: bool,
    inl_bad: bool,
    /// in an inlined callee: where its this is, from the caller's sp
    /// (L_SSP), and whether it is strict
    inl_this: Option<(u32, bool)>,
    /// by pc: an arithmetic op that never saw anything but numbers in the
    /// interpreter (note_nonnum): written without a slow path, a check
    /// that fails returns its pc (the frame is the interpreter's, so that
    /// is the whole deoptimization) and the interpreter's slow path then
    /// marks the site and sends the function back to be compiled again
    spec: Vec<bool>,
    /// by register: the f64 local (KN0 ..) that holds its number, -1 for
    /// none; known since the block began (the registers in memory stay
    /// written, so any exit leaves the frame right)
    kn: Vec<i32>,
    kn_regs: Vec<u32>,
    kn_next: u32,
    /// by register: its fixed local (KN0 ..) when it is one of the
    /// registers whose numbers are kept across blocks (kn_plan), -1 else
    kn_fix: Vec<i32>,
}

/// the f64 locals that keep registers known to be numbers (Cg.kn)
const KN0: u32 = 25;
const KN_N: u32 = 16;
/// i32: 1 until the first block's start has run (the code was entered from
/// outside, where the interpreter may have left anything in a register)
const L_FRESH: u32 = 41;

/// What an op does to the registers known to hold numbers (kn_plan and
/// the code agree on it): Spec its speculated arithmetic (its operands and
/// result known after it), Kill registers it may write, All when it may
/// write any.
enum KnFx {
    Same,
    Spec(i32, i32, i32),
    Kill(i32, i32),
    All,
}

/// The op's KnFx; `spec` says whether its pc speculates.
fn kn_fx(ins: Op, spec: bool) -> KnFx {
    let kind = (ins.code & 255) as int;
    let sub = ((ins.code >> 8) & 255) as int;
    if spec && ins.a >= 0 && (kind == R_ADD || kind == R_SUB || kind == R_MUL || (kind == R_ARITH && (sub == OP_SUB || sub == OP_MUL || sub == OP_DIV))) {
        return KnFx::Spec(ins.a, ins.b, ins.c);
    }
    match kind {
        R_NOP | R_JUMP | R_LOOP | R_JF | R_JT | R_CMP_JF | R_CMP_JT | R_CMP_JFK | R_CMP_JTK => KnFx::Same,
        R_MOVE | R_GET_ELEM | R_GET_PROP | R_GET_THIS_PROP | R_GET_ENV | R_GET_GLOBAL | R_THIS | R_TYPEOF | R_NOT | R_TONUM | R_CMP | R_ADD | R_SUB | R_MUL | R_ARITH | R_SET_PROP | R_SET_ELEM | R_SET_ENV => KnFx::Kill(ins.a, ins.a + 1),
        R_LOAD2 => KnFx::Kill(ins.a, ins.a + 1),
        _ => KnFx::All,
    }
}

/// The registers (at most KN_N) whose numbers are kept in fixed locals:
/// those of the speculated arithmetic, most used first. Each block's set
/// known on entry (a bit per kept register): what every jump or fall into
/// it brings (a forward must-analysis); an entry from outside checks them
/// (Cg::kn_block).
fn kn_plan(code: &[Op], rtab: &[int], spec: &[bool], starts: &[bool], skip_to: &[usize], fsize: u32) -> (Vec<i32>, Vec<u32>) {
    let n = code.len();
    let mut uses = vec![0u32; fsize as usize + 2];
    for (i, ins) in code.iter().enumerate() {
        if let KnFx::Spec(d, x, y) = kn_fx(*ins, i < spec.len() && spec[i]) {
            for r in [d, x, y] {
                if r >= 0 && (r as usize) < uses.len() {
                    uses[r as usize] += 1;
                }
            }
        }
    }
    let mut regs: Vec<usize> = (0..uses.len()).filter(|r| uses[*r] > 0).collect();
    regs.sort_by(|a, b| uses[*b].cmp(&uses[*a]));
    regs.truncate(KN_N as usize);
    let mut fix = vec![-1i32; uses.len()];
    for (k, r) in regs.iter().enumerate() {
        fix[*r] = k as i32;
    }
    let bit = |r: i32| -> u32 { if r >= 0 && (r as usize) < fix.len() && fix[r as usize] >= 0 { 1u32 << fix[r as usize] } else { 0 } };
    let step = |set: u32, i: usize| -> u32 {
        match kn_fx(code[i], i < spec.len() && spec[i]) {
            KnFx::Same => set,
            KnFx::Spec(d, x, y) => set | bit(d) | bit(x) | bit(y),
            KnFx::Kill(a, b) => set & !bit(a) & !bit(b),
            KnFx::All => 0,
        }
    };
    // in[pc] for block starts; all bits until a path says otherwise
    let full: u32 = if regs.is_empty() { 0 } else { u32::MAX };
    let mut inb = vec![full; n];
    if regs.is_empty() {
        return (fix, vec![0; n]);
    }
    let mut changed = true;
    let mut rounds = 0;
    while changed && rounds < 64 {
        changed = false;
        rounds += 1;
        let mut out: Vec<u32> = vec![full; n];
        let mut seen = vec![false; n];
        let mut set: u32 = 0;
        let mut live = false;
        for i in 0..n {
            if starts[i] {
                // the fall from the op before is one of its ways in
                if live {
                    out[i] &= set;
                    seen[i] = true;
                }
                set = inb[i];
                live = true;
            }
            if !live {
                continue;
            }
            set = step(set, i);
            let mut ts = Vec::new();
            targets(code[i], i, rtab, &mut ts);
            if skip_to[i] > 0 {
                ts.push(skip_to[i]);
            }
            for t in ts {
                if t < n {
                    out[t] &= set;
                    seen[t] = true;
                }
            }
            let k = (code[i].code & 255) as int;
            if k == R_JUMP || k == R_LOOP {
                live = false;
            }
        }
        for i in 0..n {
            if starts[i] {
                let v = if seen[i] { out[i] } else { 0 };
                if v != inb[i] {
                    inb[i] = v;
                    changed = true;
                }
            }
        }
    }
    if changed {
        return (vec![-1i32; uses.len()], vec![0; n]);
    }
    (fix, inb)
}

/// A callee to write in place of its call (call_op): straight register
/// code with no call, no helper and no write to its parameters, so that a
/// failed check can still make the call itself.
#[derive(Clone)]
pub struct Inl {
    pub tp: u32,
    pub code: Vec<Op>,
    pub entry: usize,
    pub nparams: u32,
    pub nslots: u32,
    pub fsize: u32,
    pub rb: u32,
    /// the callee's own site caches and atoms (site_caches)
    pub caches: Vec<u32>,
    pub atoms: Vec<i32>,
    pub strict: bool,
    /// per callee pc: a GET_THIS_PROP read as a move from this operand
    /// (inline_ok), else i32::MIN
    pub fwd: Vec<i32>,
}

/// After a write that cannot be undone (a scope slot, a property): only
/// moves up to the return, which cannot fail and send the call to be made
/// again. `fwd` (atom word, value operand) is a property just stored on
/// this: a read of it again from this becomes a move of that value
/// (written into `out` at its pc).
fn settles(code: &[Op], from: usize, np: i32, fwd: Option<(i32, i32)>, out: &mut Vec<i32>) -> bool {
    let mut pc = from;
    let mut fwd = fwd;
    while pc < code.len() {
        let op = code[pc];
        let k = (op.code & 255) as int;
        // a write to the forwarded value's register ends the forwarding
        let kill = |f: Option<(i32, i32)>, w: i32| match f {
            Some((_, v)) if v == w => None,
            x => x,
        };
        match k {
            R_RET | R_RET_UNDEF => return true,
            R_NOP => pc += 1 + (op.code >> 16) as usize,
            R_MOVE if op.a >= np => {
                fwd = kill(fwd, op.a);
                pc += 1
            }
            // a scope read has no check that could fail
            R_GET_ENV if op.a >= np && ((op.code >> 8) & 255) == 0 && op.b >= 0 && op.c >= 0 => {
                fwd = kill(fwd, op.a);
                pc += 1
            }
            R_GET_THIS_PROP if op.a >= np && ((op.code >> 8) & 255) == 0 && matches!(fwd, Some((c, _)) if c == op.c) => {
                out[pc] = fwd.unwrap().1;
                fwd = kill(fwd, op.a);
                pc += 1
            }
            _ => return false,
        }
    }
    false
}

/// Most ops an inlined callee may have (its NOPs not counted).
pub const INL_OPS: usize = 12;

/// The callee's register code from its entry when inline_call can write
/// it: arithmetic, moves and scope reads up to a return, no jump. Some:
/// per pc, the operand a GET_THIS_PROP is forwarded from (settles), else
/// i32::MIN.
pub fn inline_ok(code: &[Op], entry: usize, nparams: u32) -> Option<Vec<i32>> {
    let mut out = vec![i32::MIN; code.len()];
    let mut pc = entry;
    let mut n = 0usize;
    let np = nparams as i32;
    // the registers that hold this (R_THIS), as a bit mask
    let mut this_regs: u64 = 0;
    while pc < code.len() {
        let op = code[pc];
        let k = (op.code & 255) as int;
        let skip = (op.code >> 16) as usize;
        let sub = ((op.code >> 8) & 255) as int;
        if k == R_NOP {
            pc += 1 + skip;
            continue;
        }
        if k == R_RET || k == R_RET_UNDEF {
            return Some(out);
        }
        // a skip may only step over NOPs (the ops it skips do nothing)
        if pc + 1 + skip > code.len() || (pc + 1..pc + 1 + skip).any(|j| (code[j].code & 255) as int != R_NOP) {
            return None;
        }
        n += 1;
        if n > INL_OPS {
            return None;
        }
        // the registers it writes stay clear of the parameters
        let w = op.a;
        let is_this = |r: i32| r >= 0 && r < 64 && this_regs & (1u64 << r) != 0;
        let ok = match k {
            R_MOVE | R_ADD | R_SUB | R_MUL => w >= np,
            R_ARITH => w >= np && matches!(sub, OP_SUB | OP_MUL | OP_DIV | OP_BAND | OP_BOR | OP_BXOR | OP_SHL | OP_SHR),
            R_LOAD2 => w >= np,
            R_GET_ENV => w >= np && sub <= 1 && op.b >= 0 && op.c >= 0,
            R_THIS | R_GET_THIS_PROP => w >= np,
            R_GET_PROP => w >= np && op.b >= 0,
            R_SET_ENV => op.b >= 0 && op.c >= 0 && return if settles(code, pc + 1, np, None, &mut out) { Some(out) } else { None },
            R_ADD_ENV => op.a >= 0 && op.c >= 0 && return if settles(code, pc + 1, np, None, &mut out) { Some(out) } else { None },
            R_SET_PROP => {
                let f = if is_this(w) && op.c >= 0 { Some((op.c, op.b)) } else { None };
                return if w >= 0 && sub == 0 && settles(code, pc + 1, np, f, &mut out) { Some(out) } else { None };
            }
            _ => false,
        };
        if !ok {
            return None;
        }
        if w >= 0 && w < 64 {
            if k == R_THIS {
                this_regs |= 1u64 << w;
            } else {
                this_regs &= !(1u64 << w);
            }
        }
        if (k == R_LOAD2 || (k == R_GET_ENV && sub == 1)) && w + 1 < 64 {
            this_regs &= !(1u64 << (w + 1));
        }
        pc += 1;
    }
    None
}

impl<'a> Cg<'a> {
    fn op(&mut self, x: u8) {
        self.w.byte(x);
    }
    fn i32c(&mut self, x: i64) {
        self.w.byte(I32_CONST);
        self.w.s(x);
    }
    fn get(&mut self, l: u32) {
        self.w.byte(LOCAL_GET);
        self.w.u(l as u64);
    }
    fn set(&mut self, l: u32) {
        self.w.byte(LOCAL_SET);
        self.w.u(l as u64);
    }
    fn mem(&mut self, op: u8, align: u32, off: u32) {
        self.w.byte(op);
        self.w.u(align as u64);
        self.w.u(off as u64);
    }
    fn opnd(&self, x: i32) -> Opnd {
        if x >= 0 {
            return Opnd::Reg(x as u32);
        }
        let k = (-1 - x) as usize;
        match self.kvals.get(k).map(|v| v.kind()) {
            Some(VK::Num(n)) => Opnd::Num(n),
            _ => Opnd::K(k as u32),
        }
    }
    /// base local and byte offset of an operand's value in memory
    fn place(&self, o: Opnd) -> (u32, u32) {
        match o {
            Opnd::Reg(r) => (L_SP, r * VS),
            Opnd::K(k) => (L_KV, k * VS),
            Opnd::Num(_) => (L_SP, 0),
        }
    }
    fn exit_depth(&self) -> u32 {
        self.exit_d + self.extra
    }
    /// return pc to the interpreter
    fn exit_at(&mut self, pc: i64) {
        if self.inlining {
            self.inl_bad = true;
        }
        self.i32c(pc);
        self.set(L_OUT);
        let d = self.exit_depth();
        self.op(BR);
        self.w.u(d as u64);
    }
    /// the condition on the wasm stack holds: return pc
    fn exit_if(&mut self, pc: i64) {
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        self.exit_at(pc);
        self.extra -= 1;
        self.op(END);
    }
    /// go on at block `to` (through the dispatch loop)
    fn jump(&mut self, to: i64, back: bool) {
        if back {
            self.backs += 1;
            // budget -= 1; when it is gone return -1 - to
            self.get(L_BUDGET);
            self.i32c(1);
            self.op(I32_SUB);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_BUDGET as u64);
            self.i32c(0);
            self.op(I32_LE_S);
            self.exit_if(-1 - to);
            self.i32c(1 << 29);
            self.set(L_LOOPED);
        }
        self.i32c(to);
        self.set(L_KEY);
        let d = self.exit_depth() + 1;
        self.op(BR);
        self.w.u(d as u64);
    }
    /// the operand's tag on the wasm stack
    fn tag(&mut self, o: Opnd) {
        match o {
            Opnd::Num(_) => {
                let t = self.lay.t_num as i64;
                self.i32c(t);
            }
            _ => {
                let (b, off) = self.place(o);
                self.tag_at(b, off);
            }
        }
    }
    /// the condition on the wasm stack holds: the fast path fails (its slow
    /// block, or return pc)
    fn fail_if(&mut self, pc: i64) {
        match self.slow_at {
            Some(at) => {
                self.op(BR_IF);
                let d = self.extra - at;
                self.w.u(d as u64);
            }
            None => self.exit_if(pc),
        }
    }
    fn fail(&mut self, pc: i64) {
        match self.slow_at {
            Some(at) => {
                self.op(BR);
                let d = self.extra - at;
                self.w.u(d as u64);
            }
            None => self.exit_at(pc),
        }
    }
    /// fail unless the operand is a number
    fn need_num(&mut self, o: Opnd, pc: i64) {
        match o {
            Opnd::Num(_) => {}
            Opnd::K(_) => {
                // a constant that is not a number never is
                self.fail(pc);
            }
            Opnd::Reg(_) => {
                self.not_num_o(o);
                self.fail_if(pc);
            }
        }
    }
    /// The op's fast path (`fast`), and the interpreter's whole op through
    /// H_SLOW when one of its checks fails; for a comparison both leave
    /// the answer in L_C.
    fn with_slow(&mut self, pc: i64, next: i64, cmp_result: bool, fast: &dyn Fn(&mut Cg<'a>) -> bool) -> bool {
        self.with_helper(H_SLOW, pc, next, cmp_result, fast)
    }
    /// with_slow with another helper for the slow block
    fn with_helper(&mut self, h: u32, pc: i64, next: i64, cmp_result: bool, fast: &dyn Fn(&mut Cg<'a>) -> bool) -> bool {
        if self.inlining {
            // an inlined callee: a failed check goes to the normal call
            if !fast(self) {
                self.inl_bad = true;
            }
            return true;
        }
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        let saved = self.slow_at;
        self.slow_at = Some(self.extra);
        let ok = fast(self);
        self.slow_at = saved;
        if ok {
            self.op(BR);
            self.w.u(1);
        }
        self.op(END);
        self.extra -= 1;
        self.helper(h, pc, next);
        if cmp_result {
            let cell = cell_addr() as i64;
            self.i32c(cell);
            self.mem(I32_LOAD, 2, 12);
            self.set(L_C);
        }
        self.op(END);
        self.extra -= 1;
        true
    }
    /// the operand's number on the wasm stack (after need_num)
    fn num(&mut self, o: Opnd) {
        match o {
            Opnd::Num(n) => {
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&n.to_le_bytes());
            }
            _ => {
                let (b, off) = self.place(o);
                self.get(b);
                let on = self.lay.off_num;
                self.mem(F64_LOAD, 3, off + on);
            }
        }
    }
    /// slot r = Num(local l)
    fn store_num(&mut self, r: u32, l: u32) {
        self.get(L_SP);
        self.get(l);
        self.mem(F64_STORE, 3, r * VS);
    }
    /// slot r = Bool(the i32 on the wasm stack)
    fn store_bool(&mut self, r: u32) {
        self.set(L_TAG);
        self.get(L_SP);
        self.get(L_TAG);
        self.op(I64_EXTEND_I32_U);
        self.i64c(crate::value::V_BOOL as i64);
        self.op(I64_OR);
        self.mem(I64_STORE, 3, r * VS);
    }
    /// an i64 constant on the wasm stack
    fn i64c(&mut self, x: i64) {
        self.w.byte(I64_CONST);
        self.w.s(x);
    }
    /// the raw top 16 bits of the value at b + off: its tag for any value
    /// but a number (compare with any tag but t_num)
    fn rtag(&mut self, b: u32, off: u32) {
        self.get(b);
        self.mem(I32_LOAD16_U, 1, off + 6);
    }
    /// 1 when the value at b + off is not a number (top bits above 0xFFF8)
    fn not_num(&mut self, b: u32, off: u32) {
        self.rtag(b, off);
        self.i32c(0xFFF9);
        self.op(I32_GE_U);
    }
    /// not_num for an operand (a number constant never fails)
    fn not_num_o(&mut self, o: Opnd) {
        match o {
            Opnd::Num(_) => self.i32c(0),
            _ => {
                let (b, off) = self.place(o);
                self.not_num(b, off);
            }
        }
    }
    /// the tag of the value at b + off on the wasm stack: its top 16 bits
    /// for undefined .. a small BigInt (value.rs V_UNDEF ..), t_num
    /// (0xFFF8) for a number
    fn tag_at(&mut self, b: u32, off: u32) {
        self.get(b);
        self.mem(I32_LOAD16_U, 1, off + 6);
        self.i32c(0xFFF8);
        self.get(b);
        self.mem(I32_LOAD16_U, 1, off + 6);
        self.i32c(0xFFF8);
        self.op(I32_GT_U);
        self.op(SELECT);
    }
    /// the address on the wasm stack gets the bits of a value of tag t
    /// with a payload of 0 (a payload store at the same address after it
    /// makes the whole value)
    fn st_tag(&mut self, off: u32, t: u32) {
        self.i64c(((t as u64) << 48) as i64);
        self.mem(I64_STORE, 3, off);
    }
    /// the 8 bytes of an operand as an i64 into local q
    fn load_val(&mut self, o: Opnd, q: u32) -> bool {
        match o {
            Opnd::Num(_) => false,
            _ => {
                let (b, off) = self.place(o);
                self.get(b);
                self.mem(I64_LOAD, 3, off);
                self.set(q);
                true
            }
        }
    }
    fn store_val(&mut self, r: u32, q: u32) {
        self.get(L_SP);
        self.get(q);
        self.mem(I64_STORE, 3, r * VS);
    }
    /// the raw operand (a kvals index for constants, numbers included)
    fn raw(&self, x: i32) -> Opnd {
        if x >= 0 {
            Opnd::Reg(x as u32)
        } else {
            Opnd::K((-1 - x) as u32)
        }
    }
    /// truthy(operand) as an i32 on the wasm stack, or return pc for a
    /// string that is not a constant
    fn truthy(&mut self, x: i32, pc: i64) {
        if x < 0 {
            let k = (-1 - x) as usize;
            let t = match self.kvals.get(k) {
                Some(v) => crate::vm::truthy(v),
                None => false,
            };
            self.i32c(if t { 1 } else { 0 });
            return;
        }
        let o = self.raw(x);
        let (b, off) = self.place(o);
        self.tag(o);
        self.set(L_TAG);
        let l = self.lay;
        // number: n != 0 && n == n
        self.get(b);
        self.mem(F64_LOAD, 3, off + l.off_num);
        self.set(L_A);
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&0.0f64.to_le_bytes());
        self.op(F64_NE);
        self.get(L_A);
        self.get(L_A);
        self.op(F64_EQ);
        self.op(I32_AND);
        // small BigInt: its 48 payload bits are not all 0
        self.get(b);
        self.mem(I64_LOAD, 3, off);
        self.i64c(16);
        self.op(I64_SHL);
        self.i64c(0);
        self.op(I64_NE);
        // bool or string: the low word (bool's bit, the string handle,
        // 0 for the empty string) is not 0; otherwise object
        self.get(b);
        self.mem(I32_LOAD, 2, off);
        self.i32c(0);
        self.op(I32_NE);
        self.get(L_TAG);
        self.i32c(l.t_obj as i64);
        self.op(I32_EQ);
        self.get(L_TAG);
        self.i32c(l.t_str as i64);
        self.op(I32_EQ);
        self.get(L_TAG);
        self.i32c(l.t_bool as i64);
        self.op(I32_EQ);
        self.op(I32_OR);
        self.op(SELECT);
        self.get(L_TAG);
        self.i32c(l.t_big as i64);
        self.op(I32_EQ);
        self.op(SELECT);
        self.get(L_TAG);
        self.i32c(l.t_num as i64);
        self.op(I32_EQ);
        self.op(SELECT);
    }
    /// x <cmp> y for two numbers as an i32, or return pc
    fn cmp(&mut self, cmp: i32, x: i32, y: i32, pc: i64) -> bool {
        let c = cmp as int;
        if c == OP_SEQ || c == OP_SNE || c == OP_EQ || c == OP_NE {
            self.equal(c, x, y, pc);
            return true;
        }
        let f = match cmp as int {
            OP_LT => F64_LT,
            OP_GT => F64_GT,
            OP_LE => F64_LE,
            OP_GE => F64_GE,
            OP_SEQ | OP_EQ => F64_EQ,
            OP_SNE | OP_NE => F64_NE,
            _ => return false,
        };
        let a = self.opnd(x);
        let b = self.opnd(y);
        self.need_num(a, pc);
        self.need_num(b, pc);
        self.num(a);
        self.num(b);
        self.op(f);
        true
    }
    /// dst = obj.prop where obj has the shape the inline cache `c` (shape
    /// * 65536 + slot, as the interpreter's R_GET_PROP) saw when this was
    /// compiled: the slot read from the object's vals
    fn get_prop_ic(&mut self, dst: u32, obj: u32, c: i32, pc: i64) -> bool {
        self.get_prop_at(dst, L_SP, obj * VS, c, pc)
    }
    /// &objs[id] for the object id on the wasm stack
    fn obj_addr(&mut self) {
        let l = self.lay;
        self.i32c(l.obj_size as i64);
        self.op(I32_MUL);
        let cell = cell_addr() as i64;
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 16);
        self.op(I32_ADD);
    }
    /// The object address on the stack becomes its vals' header address:
    /// the SVec's pointer less 8 (svec.rs; len at 0, cap at 4).
    fn vals_hdr(&mut self) {
        let l = self.lay;
        self.mem(I32_LOAD, 2, l.off_vals);
        self.i32c(8);
        self.op(I32_SUB);
    }
    /// The value at b + off into the heap slot at L_T2, counted as
    /// Vm::inc and dec count it: fails (before writing) for a string, old
    /// or new, and for an old object the write would leave unreferenced
    /// (Vm::dec_o puts it in the zct).
    /// tags 0xFFF8 (number) .. 0xFFFC (string) map to typeof's indices
    /// (vm.rs type_index) by TYPE_NIB
    fn typeof_ok(&self) -> bool {
        let l = self.lay;
        l.t_num == 0xFFF8 && l.t_undef == 0xFFF9 && l.t_null == 0xFFFA && l.t_bool == 0xFFFB && l.t_str == 0xFFFC
    }
    /// heap_store through the module's hstore(src, slot, .., vm, ..)
    fn heap_store(&mut self, b: u32, off: u32, pc: i64) {
        self.get(b);
        if off != 0 {
            self.i32c(off as i64);
            self.op(I32_ADD);
        }
        self.get(L_T2);
        self.i32c(0);
        self.i32c(0);
        self.get(L_VM);
        self.i32c(0);
        self.op(CALL);
        self.w.u(F_HSTORE as u64);
        self.fail_if(pc);
    }
    /// The body of hstore: 0 stored, 1 the helper must do it
    fn hstore_body(&mut self) -> Vec<u8> {
        let saved_w = std::mem::replace(&mut self.w, WBuf::new());
        let (se, sx, ss) = (self.extra, self.exit_d, self.slow_at);
        self.w.u(4);
        self.w.u(3);
        self.w.byte(T_I32);
        self.w.u(2);
        self.w.byte(T_F64);
        self.w.u(4);
        self.w.byte(T_I64);
        self.w.u(7);
        self.w.byte(T_I32);
        self.extra = 0;
        self.exit_d = 0;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.slow_at = Some(self.extra);
        self.get(1);
        self.set(L_T2);
        self.heap_store_inline(0, 0, 0);
        self.i32c(0);
        self.op(RETURN);
        self.extra -= 1;
        self.op(END);
        self.i32c(1);
        self.op(END);
        let out = std::mem::replace(&mut self.w, saved_w).b;
        self.extra = se;
        self.exit_d = sx;
        self.slow_at = ss;
        out
    }
    fn heap_store_inline(&mut self, b: u32, off: u32, pc: i64) {
        let l = self.lay;
        let ts = l.t_str as i64;
        let to = l.t_obj as i64;
        // an old string the write would leave unreferenced (Vm::sdec puts
        // it in szct): the helper
        self.rtag(L_T2, 0);
        self.i32c(ts);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_int);
        self.set(L_T5);
        self.get(L_T5);
        self.i32c(0);
        self.op(I32_GT_S);
        self.srcs_addr(L_T5);
        self.mem(I32_LOAD, 2, 0);
        self.i32c(1);
        self.op(I32_LE_S);
        self.op(I32_AND);
        self.fail_if(pc);
        self.extra -= 1;
        self.op(END);
        // an old object: rc > 1, then rc - 1 (L_T3 its &objs[id])
        self.i32c(0);
        self.set(L_T3);
        self.rtag(L_T2, 0);
        self.i32c(to);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T3 as u64);
        self.mem(I32_LOAD, 2, l.off_rc);
        self.i32c(1);
        self.op(I32_LE_S);
        self.get(L_T3);
        self.mem(I32_LOAD8_U, 0, l.off_zct);
        self.get(L_T3);
        self.mem(I32_LOAD8_U, 0, l.off_free);
        self.op(I32_OR);
        self.op(I32_EQZ);
        self.op(I32_AND);
        // it goes into the zct (Vm::dec_o): room there, else the helper
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_zct_len);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_zct_cap);
        self.op(I32_GE_U);
        self.op(I32_AND);
        self.fail_if(pc);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_int);
        self.set(L_OUT);
        self.extra -= 1;
        self.op(END);
        // a new object: rc + 1
        self.rtag(b, off);
        self.i32c(to);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.get(b);
        self.mem(I32_LOAD, 2, off + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T4 as u64);
        self.get(L_T4);
        self.mem(I32_LOAD, 2, l.off_rc);
        self.i32c(1);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, l.off_rc);
        self.op(END);
        self.get(L_T3);
        self.op(IF);
        self.op(T_VOID);
        self.get(L_T3);
        self.get(L_T3);
        self.mem(I32_LOAD, 2, l.off_rc);
        self.i32c(1);
        self.op(I32_SUB);
        self.mem(I32_STORE, 2, l.off_rc);
        // rc <= 0, not in the zct, not free: zct.push(id), as dec_o
        self.get(L_T3);
        self.mem(I32_LOAD, 2, l.off_rc);
        self.i32c(0);
        self.op(I32_LE_S);
        self.get(L_T3);
        self.mem(I32_LOAD8_U, 0, l.off_zct);
        self.get(L_T3);
        self.mem(I32_LOAD8_U, 0, l.off_free);
        self.op(I32_OR);
        self.op(I32_EQZ);
        self.op(I32_AND);
        self.op(IF);
        self.op(T_VOID);
        self.get(L_T3);
        self.i32c(1);
        self.mem(I32_STORE8, 0, l.off_zct);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_zct_ptr);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_zct_len);
        self.i32c(8);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.get(L_OUT);
        self.op(I64_EXTEND_I32_S);
        self.mem(I64_STORE, 3, 0);
        self.get(L_VM);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_zct_len);
        self.i32c(1);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, l.vm_zct_len);
        // len >= zct_limit: a collection is due
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_zct_len);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_zct_limit);
        self.op(I32_GE_S);
        self.op(IF);
        self.op(T_VOID);
        self.get(L_VM);
        self.i32c(1);
        self.mem(I32_STORE8, 0, l.vm_gc_flag);
        self.op(END);
        self.op(END);
        self.op(END);
        // strings: new + 1, old - 1 (more than one reference, above)
        self.str_count(b, off, I32_ADD);
        self.str_count(L_T2, 0, I32_SUB);
        self.get(L_T2);
        self.get(b);
        self.mem(I64_LOAD, 3, off);
        self.mem(I64_STORE, 3, 0);
    }
    /// &Vm.strs[local h].rc on the wasm stack
    fn srcs_addr(&mut self, h: u32) {
        let l = self.lay;
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_srcs_ptr);
        self.get(h);
        self.i32c(std::mem::size_of::<crate::vm::SEnt>() as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.i32c(std::mem::offset_of!(crate::vm::SEnt, rc) as i64);
        self.op(I32_ADD);
    }
    /// the string at b + off, if one (handle > 0): srcs[h] op= 1
    fn str_count(&mut self, b: u32, off: u32, op: u8) {
        let l = self.lay;
        self.rtag(b, off);
        self.i32c(l.t_str as i64);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.get(b);
        self.mem(I32_LOAD, 2, off + l.off_int);
        self.set(L_T5);
        self.get(L_T5);
        self.i32c(0);
        self.op(I32_GT_S);
        self.op(IF);
        self.op(T_VOID);
        self.srcs_addr(L_T5);
        self.srcs_addr(L_T5);
        self.mem(I32_LOAD, 2, 0);
        self.i32c(1);
        self.op(op);
        self.mem(I32_STORE, 2, 0);
        self.op(END);
        self.op(END);
    }
    /// L_T2 = the address of slot `slot` of the scope `depth` up from the
    /// frame's (rvm.rs R_GET_ENV: get_env, env_slot)
    fn env_slot_addr(&mut self, depth: i32, slot: i32) {
        let l = self.lay;
        let cell = cell_addr() as i64;
        if self.inlining {
            // an inlined callee's scope is its closure's (inline_call)
            self.get(L_SENV);
        } else {
            // the frame's env: cell[5] is &frame.this_val
            self.i32c(cell);
            self.mem(I32_LOAD, 2, 20);
            self.i32c(l.frame_this as i64);
            self.op(I32_SUB);
            self.mem(I32_LOAD, 2, l.frame_env);
        }
        for _ in 0..depth {
            self.obj_addr();
            self.mem(I32_LOAD, 2, l.off_proto);
        }
        self.obj_addr();
        self.mem(I32_LOAD, 2, l.off_elems);
        self.i32c(l.elems_size as i64);
        self.op(I32_MUL);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_evecs_ptr);
        self.op(I32_ADD);
        self.mem(I32_LOAD, 2, l.elems_vp);
        self.i32c(slot as i64 * VS as i64);
        self.op(I32_ADD);
        self.set(L_T2);
    }
    /// dst = an array's element by a whole number in range (h_get_elem's
    /// fast path), else fail
    /// The next key of a for-in iterator (register it) into dst, as
    /// for_in_next while the object keeps the shape the loop began with:
    /// the shape's keys from the iterator's position, hidden ones,
    /// symbols and private names skipped. Anything else (another
    /// iterator, the end, a key whose kind or value is not known yet)
    /// fails to the helper.
    fn for_in_step(&mut self, it: u32, dst: u32, pc: i64) {
        let l = self.lay;
        self.need_obj(L_SP, it * VS, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, it * VS + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_class);
        self.i32c(C_ITER as i64);
        self.op(I32_NE);
        // the source (env) >= 0
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_env);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T3 as u64);
        self.i32c(0);
        self.op(I32_LT_S);
        self.op(I32_OR);
        // no home for its cold part (r_iter_next's deleted-key check)
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_colds_ptr);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_cold);
        self.i32c(l.cold_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.mem(I32_LOAD, 2, l.cold_home);
        self.i32c(0);
        self.op(I32_GE_S);
        self.op(I32_OR);
        self.fail_if(pc);
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        let ok = self.extra;
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_pos);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T4 as u64);
        self.op(I32_EQZ);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        {
            // an array's values (kind 0, iter_next): elems[i] while
            // i < len, i + 1
            self.get(L_T3);
            self.obj_addr();
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T5 as u64);
            self.mem(I32_LOAD, 2, l.off_class);
            self.i32c(C_ARRAY as i64);
            self.op(I32_NE);
            self.fail_if(pc);
            self.get(L_T5);
            self.mem(I32_LOAD, 2, l.off_pos);
            self.i32c(3);
            self.op(I32_EQ);
            self.fail_if(pc);
            self.get(L_T5);
            self.mem(I32_LOAD, 2, l.off_elems);
            self.i32c(l.elems_size as i64);
            self.op(I32_MUL);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_evecs_ptr);
            self.op(I32_ADD);
            self.set(L_T5);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_func);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T6 as u64);
            self.get(L_T5);
            self.mem(I32_LOAD, 2, l.elems_vl);
            self.op(I32_GE_U);
            self.fail_if(pc);
            self.get(L_T2);
            self.get(L_T6);
            self.i32c(1);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, l.off_func);
            self.get(L_T5);
            self.mem(I32_LOAD, 2, l.elems_vp);
            self.get(L_T6);
            self.i32c(VS as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.set(L_T4);
            self.get(L_T4);
            self.mem(I64_LOAD, 3, 0);
            self.set(L_Q0);
            self.store_val(dst, L_Q0);
            self.op(BR);
            self.w.u((self.extra - ok) as u64);
        }
        self.extra -= 1;
        self.op(END);
        // else kind 3, a for-in
        self.get(L_T4);
        self.i32c(3);
        self.op(I32_AND);
        self.i32c(3);
        self.op(I32_NE);
        self.fail_if(pc);
        // the source still has the loop's shape (pos >> 2)
        self.get(L_T4);
        self.i32c(2);
        self.op(I32_SHR_S);
        self.set(L_T4);
        self.get(L_T3);
        self.obj_addr();
        self.mem(I32_LOAD, 2, l.off_shape);
        self.get(L_T4);
        self.op(I32_NE);
        self.fail_if(pc);
        // L_C = nkeys, L_T5 = the holder shape, L_T6 = the next key
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_shapes_ptr);
        self.get(L_T4);
        self.i32c(l.shape_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T5 as u64);
        self.mem(I32_LOAD, 2, l.sh_nkeys);
        self.set(L_C);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_shapes_ptr);
        self.get(L_T5);
        self.mem(I32_LOAD, 2, l.sh_kref);
        self.i32c(l.shape_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T5);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_func);
        self.set(L_T6);
        self.op(LOOP);
        self.op(T_VOID);
        self.extra += 1;
        let again = self.extra;
        self.get(L_T6);
        self.get(L_C);
        self.op(I32_GE_S);
        self.fail_if(pc);
        // a = keys[k] (an int, its low word), its attributes
        self.get(L_T5);
        self.mem(I32_LOAD, 2, l.sh_keys_p);
        self.get(L_T6);
        self.i32c(8);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.mem(I32_LOAD, 2, 0);
        self.set(L_T4);
        self.get(L_T5);
        self.mem(I32_LOAD, 2, l.sh_attrs_p);
        self.get(L_T6);
        self.i32c(8);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.mem(I32_LOAD, 2, 0);
        self.i32c(P_HIDDEN as i64);
        self.op(I32_AND);
        self.get(L_T6);
        self.i32c(1);
        self.op(I32_ADD);
        self.set(L_T6);
        self.op(BR_IF);
        self.w.u((self.extra - again) as u64);
        // its kind (atom_kinds, 0 not known yet: the helper)
        self.get(L_T4);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_akinds_len);
        self.op(I32_GE_U);
        self.fail_if(pc);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_akinds_ptr);
        self.get(L_T4);
        self.i32c(4);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.mem(I32_LOAD, 2, 0);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_TAG as u64);
        self.op(I32_EQZ);
        self.fail_if(pc);
        self.get(L_TAG);
        self.i32c(3);
        self.op(I32_GE_S);
        self.op(BR_IF);
        self.w.u((self.extra - again) as u64);
        // its value (atom_vals, undefined not made yet: the helper)
        self.get(L_T4);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_avals_len);
        self.op(I32_GE_U);
        self.fail_if(pc);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_avals_ptr);
        self.get(L_T4);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T4);
        self.rtag(L_T4, 0);
        self.i32c(l.t_str as i64);
        self.op(I32_NE);
        self.fail_if(pc);
        self.extra -= 1;
        self.op(END);
        self.get(L_T2);
        self.get(L_T6);
        self.mem(I32_STORE, 2, l.off_func);
        self.get(L_T4);
        self.mem(I64_LOAD, 3, 0);
        self.set(L_Q0);
        self.store_val(dst, L_Q0);
        self.extra -= 1;
        self.op(END);
    }
    /// dst = the array at register ob's length
    fn array_len(&mut self, dst: u32, ob: u32, pc: i64) -> bool {
        let l = self.lay;
        self.need_obj(L_SP, ob * VS, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, ob * VS + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_class);
        self.i32c(C_ARRAY as i64);
        self.op(I32_NE);
        self.fail_if(pc);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_elems);
        self.i32c(l.elems_size as i64);
        self.op(I32_MUL);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_evecs_ptr);
        self.op(I32_ADD);
        self.mem(I32_LOAD, 2, l.elems_vl);
        self.op(F64_CONVERT_I32_U);
        self.set(L_A);
        self.store_num(dst, L_A);
        true
    }
    fn get_elem_fast(&mut self, dst: u32, ob: u32, key: Opnd, pc: i64) -> bool {
        let l = self.lay;
        self.need_obj(L_SP, ob * VS, pc);
        let skey = match key {
            Opnd::Reg(r) if MASK.with(|m| m.get()) & (1u64 << 50) == 0 => Some(r),
            _ => None,
        };
        let done = self.extra + 1;
        if let Some(r) = skey {
            // a string key whose atom is known (to_key): rt eget on a plain
            // object and its prototypes
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            self.rtag(L_SP, r * VS);
            self.i32c(l.t_str as i64);
            self.op(I32_EQ);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            let sz = std::mem::size_of::<crate::vm::SEnt>() as i64;
            let o_atom = std::mem::offset_of!(crate::vm::SEnt, atom) as u32;
            self.get(L_SP);
            self.mem(I32_LOAD, 2, r * VS + l.off_int);
            self.i32c(sz);
            self.op(I32_MUL);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_srcs_ptr);
            self.op(I32_ADD);
            self.mem(I32_LOAD, 2, o_atom);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T3 as u64);
            self.i32c(0);
            self.op(I32_LT_S);
            self.fail_if(pc);
            self.get(L_SP);
            self.mem(I32_LOAD, 2, ob * VS + l.off_int);
            self.obj_addr();
            self.get(L_T3);
            self.get(L_VM);
            self.op(CALL);
            self.w.u(F_EGET as u64);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T4 as u64);
            self.op(I32_EQZ);
            self.fail_if(pc);
            self.get(L_T4);
            self.mem(I64_LOAD, 3, 0);
            self.set(L_Q0);
            self.store_val(dst, L_Q0);
            self.op(BR);
            self.w.u((self.extra - done) as u64);
            self.extra -= 1;
            self.op(END);
        }
        self.need_num(key, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, ob * VS + l.off_int);
        self.obj_addr();
        self.set(L_T2);
        let typed = MASK.with(|m| m.get()) & (1u64 << 61) == 0;
        let tdone = self.extra + 1;
        if typed {
            // a typed array's element (Vm::ta_slot's checks, typed_at)
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_class);
            self.i32c(C_TYPED as i64);
            self.op(I32_EQ);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.index_key(key, pc);
            self.typed_at(pc);
            self.get(L_T4);
            self.mem(F64_LOAD, 3, 0);
            self.set(L_A);
            self.store_num(dst, L_A);
            self.op(BR);
            self.w.u((self.extra - tdone) as u64);
            self.extra -= 1;
            self.op(END);
        }
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_class);
        self.i32c(C_ARRAY as i64);
        self.op(I32_NE);
        self.fail_if(pc);
        // pos 3: elements kept as properties (attributes, accessors)
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_pos);
        self.i32c(3);
        self.op(I32_EQ);
        self.fail_if(pc);
        // a whole number 0 <= k < 2^31 in L_T3
        self.num(key);
        self.set(L_A);
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&0f64.to_le_bytes());
        self.op(F64_GE);
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&2147483647f64.to_le_bytes());
        self.op(F64_LE);
        self.op(I32_AND);
        self.op(I32_EQZ);
        self.fail_if(pc);
        self.get(L_A);
        self.op(I32_TRUNC_F64_S);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T3 as u64);
        self.op(F64_CONVERT_I32_S);
        self.get(L_A);
        self.op(F64_NE);
        self.fail_if(pc);
        // its Elems: k < v.len()
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_elems);
        self.i32c(l.elems_size as i64);
        self.op(I32_MUL);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_evecs_ptr);
        self.op(I32_ADD);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T4 as u64);
        self.mem(I32_LOAD, 2, l.elems_vl);
        self.get(L_T3);
        self.op(I32_LE_U);
        self.fail_if(pc);
        self.get(L_T4);
        self.mem(I32_LOAD, 2, l.elems_vp);
        self.get(L_T3);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T4);
        self.get(L_T4);
        self.mem(I64_LOAD, 3, 0);
        self.set(L_Q0);
        self.store_val(dst, L_Q0);
        if typed {
            self.extra -= 1;
            self.op(END);
        }
        if skey.is_some() {
            self.extra -= 1;
            self.op(END);
        }
        true
    }
    /// The key, a whole number 0 <= k < 2^31, into L_T3 (after need_num)
    fn index_key(&mut self, key: Opnd, pc: i64) {
        self.num(key);
        self.set(L_A);
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&0f64.to_le_bytes());
        self.op(F64_GE);
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&2147483647f64.to_le_bytes());
        self.op(F64_LE);
        self.op(I32_AND);
        self.op(I32_EQZ);
        self.fail_if(pc);
        self.get(L_A);
        self.op(I32_TRUNC_F64_S);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T3 as u64);
        self.op(F64_CONVERT_I32_S);
        self.get(L_A);
        self.op(F64_NE);
        self.fail_if(pc);
    }
    /// For the typed array at L_T2 and the index in L_T3: the address of
    /// its element's number in L_T4, as Vm::ta_slot finds it (a fixed-length
    /// view of a buffer neither detached nor resizable that holds this
    /// kind, the index inside both); fails otherwise. Uses L_T5, L_T6.
    fn typed_at(&mut self, pc: i64) {
        let l = self.lay;
        // the buffer: an object, not detached (pos bit 0)
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_env);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T5 as u64);
        self.i32c(0);
        self.op(I32_LT_S);
        self.fail_if(pc);
        self.get(L_T5);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T5 as u64);
        self.mem(I32_LOAD, 2, l.off_pos);
        self.i32c(1);
        self.op(I32_AND);
        self.fail_if(pc);
        // the view's length (Cold.prim a number): index < length
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_colds_ptr);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_cold);
        self.i32c(l.cold_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T6);
        self.not_num(L_T6, l.cold_prim);
        self.fail_if(pc);
        self.get(L_T3);
        self.op(F64_CONVERT_I32_U);
        self.get(L_T6);
        self.mem(F64_LOAD, 3, l.cold_prim + l.off_num);
        self.op(F64_GE);
        self.fail_if(pc);
        // the buffer's cold part: there, not resizable, home == kind
        self.get(L_T5);
        self.mem(I32_LOAD, 2, l.off_cold);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T6 as u64);
        self.i32c(0);
        self.op(I32_LE_S);
        self.fail_if(pc);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_colds_ptr);
        self.get(L_T6);
        self.i32c(l.cold_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T6);
        self.not_num(L_T6, l.cold_prim);
        self.op(I32_EQZ);
        self.fail_if(pc);
        self.get(L_T6);
        self.mem(I32_LOAD, 2, l.cold_home);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_pos);
        self.op(I32_NE);
        self.fail_if(pc);
        // log2 of the element size: (kind >= 3) + (kind >= 5) + (kind >= 8)
        // - 2 (kind == 11, Float16)
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_pos);
        self.set(L_T5);
        for k in [3i64, 5, 8] {
            self.get(L_T5);
            self.i32c(k);
            self.op(I32_GE_S);
        }
        self.op(I32_ADD);
        self.op(I32_ADD);
        self.get(L_T5);
        self.i32c(11);
        self.op(I32_EQ);
        self.i32c(1);
        self.op(I32_SHL);
        self.op(I32_SUB);
        self.set(L_T5);
        // the element's index in nums: byteOffset >> log2 + index
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_func);
        self.get(L_T5);
        self.op(I32_SHR_U);
        self.get(L_T3);
        self.op(I32_ADD);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T5 as u64);
        self.get(L_T6);
        self.mem(I32_LOAD, 2, l.cold_nums_l);
        self.op(I32_GE_U);
        self.fail_if(pc);
        self.get(L_T6);
        self.mem(I32_LOAD, 2, l.cold_nums_p);
        self.get(L_T5);
        self.i32c(3);
        self.op(I32_SHL);
        self.op(I32_ADD);
        self.set(L_T4);
    }
    /// R_SET_ELEM on a typed array by a whole number in range: a number
    /// the element kind holds as it is (Float64 any, the integer kinds a
    /// whole number in their range) stored, the rest to H_SET_ELEM. sub 1:
    /// the value also into w1.
    fn set_elem_typed(&mut self, ob: u32, key: Opnd, val: Opnd, keep: bool, pc: i64) -> bool {
        let l = self.lay;
        if keep && matches!(val, Opnd::Num(_)) {
            return false;
        }
        self.need_obj(L_SP, ob * VS, pc);
        self.need_num(key, pc);
        self.need_num(val, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, ob * VS + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_class);
        self.i32c(C_TYPED as i64);
        self.op(I32_NE);
        self.fail_if(pc);
        self.index_key(key, pc);
        self.typed_at(pc);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_pos);
        self.set(L_T6);
        self.num(val);
        self.set(L_A);
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        let stored = self.extra;
        // Float64: as it is
        self.get(L_T6);
        self.i32c(8);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        self.get(L_T4);
        self.get(L_A);
        self.mem(F64_STORE, 3, 0);
        self.op(BR);
        self.w.u((self.extra - stored) as u64);
        self.extra -= 1;
        self.op(END);
        // the kind's range (an impossible one for Float32 / Float16)
        let f = |cg: &mut Cg<'a>, x: f64| {
            cg.op(F64_CONST);
            cg.w.b.extend_from_slice(&x.to_le_bytes());
        };
        let chain = |cg: &mut Cg<'a>, vals: &[(i64, i64, f64)], dflt: f64| {
            // kind in lo..=hi ? x : ... (selects, innermost last)
            for v in vals.iter() {
                f(cg, v.2);
            }
            f(cg, dflt);
            for v in vals.iter().rev() {
                cg.get(L_T6);
                cg.i32c(v.0);
                cg.op(I32_GE_S);
                cg.get(L_T6);
                cg.i32c(v.1);
                cg.op(I32_LE_S);
                cg.op(I32_AND);
                cg.op(SELECT);
            }
        };
        self.get(L_A);
        chain(self, &[(0, 0, -128.0), (3, 3, -32768.0), (5, 5, -2147483648.0)], 0.0);
        self.op(F64_GE);
        self.get(L_A);
        chain(self, &[(0, 0, 127.0), (1, 2, 255.0), (3, 3, 32767.0), (4, 4, 65535.0), (5, 6, 2147483647.0)], -1.0);
        self.op(F64_LE);
        self.op(I32_AND);
        self.op(I32_EQZ);
        self.fail_if(pc);
        // a whole number: stored as the int's double (-0 as 0)
        self.get(L_A);
        self.op(I32_TRUNC_F64_S);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T5 as u64);
        self.op(F64_CONVERT_I32_S);
        self.get(L_A);
        self.op(F64_NE);
        self.fail_if(pc);
        self.get(L_T4);
        self.get(L_T5);
        self.op(F64_CONVERT_I32_S);
        self.mem(F64_STORE, 3, 0);
        self.extra -= 1;
        self.op(END);
        if keep {
            if !self.load_val(val, L_Q0) {
                return false;
            }
            self.store_val(ob, L_Q0);
        }
        true
    }
    /// fail unless the value at b + off is an object
    fn need_obj(&mut self, b: u32, off: u32, pc: i64) {
        let t = self.lay.t_obj as i64;
        self.rtag(b, off);
        self.i32c(t);
        self.op(I32_NE);
        self.fail_if(pc);
    }
    /// get_prop_ic for the object value at b + off
    fn get_prop_at(&mut self, dst: u32, b: u32, off: u32, c: i32, pc: i64) -> bool {
        let l = self.lay;
        self.need_obj(b, off, pc);
        self.get(b);
        self.mem(I32_LOAD, 2, off + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_shape);
        self.i32c((c >> 16) as i64);
        self.op(I32_NE);
        self.fail_if(pc);
        let slot = (c & 65535) as u32;
        // slots 0 and 1 in the object, the rest in vals
        let at = if slot < 2 {
            l.off_inl0 + slot * VS
        } else {
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_vals);
            self.set(L_T2);
            (slot - 2) * VS
        };
        self.get(L_T2);
        self.mem(I64_LOAD, 3, at);
        self.set(L_Q0);
        self.store_val(dst, L_Q0);
        true
    }
    /// obj.prop = v (R_SET_PROP, its cache `c` warm) where neither v nor
    /// the value it replaces is a string or an object (no counts change)
    fn set_prop_ic(&mut self, obj: u32, v: i32, c: i32, pc: i64) -> bool {
        let l = self.lay;
        let vo = self.raw(v);
        if let Opnd::K(k) = vo {
            match (self.kvals.get(k as usize)).map(|v| v.kind()) {
                Some(VK::Str(_)) | Some(VK::Obj(_)) | None => return false,
                _ => {}
            }
        }
        let (vb, voff) = self.place(vo);
        self.need_obj(L_SP, obj * VS, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, obj * VS + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_shape);
        self.i32c((c >> 16) as i64);
        self.op(I32_NE);
        self.fail_if(pc);
        // &slot: in the object for slots 0 and 1, else in vals
        let slot = (c & 65535) as u32;
        self.get(L_T2);
        if slot < 2 {
            self.i32c((l.off_inl0 + slot * VS) as i64);
        } else {
            self.mem(I32_LOAD, 2, l.off_vals);
            self.i32c(((slot - 2) * VS) as i64);
        }
        self.op(I32_ADD);
        self.set(L_T2);
        self.heap_store(vb, voff, pc);
        true
    }
    /// R_SET_PROP of an existing own data property through the site's
    /// cache at `a` (own ways, h_set_prop fills them), else the add the
    /// transition t names (set_prop_add) when t >= 0, else the helper
    fn set_prop_cached(&mut self, obj: u32, v: i32, a: u32, t: i32, atom: i32, pc: i64) -> bool {
        let l = self.lay;
        let (vb, voff) = self.place(self.raw(v));
        self.need_obj(L_SP, obj * VS, pc);
        // block $done
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.get(L_SP);
        self.mem(I32_LOAD, 2, obj * VS + l.off_int);
        self.obj_addr();
        self.i32c(a as i64);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.off_epoch);
        self.op(CALL);
        self.w.u(F_LOOK as u64);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.op(I32_EQZ);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        if t >= 0 {
            self.set_prop_add(obj, v, t, atom, pc);
            self.op(BR);
            self.w.u(1);
        } else {
            self.fail(pc);
        }
        self.extra -= 1;
        self.op(END);
        self.heap_store(vb, voff, pc);
        self.extra -= 1;
        self.op(END);
        true
    }
    /// slot r = undefined
    fn store_undef(&mut self, r: u32) {
        let u = bytes_of(Val::Undef());
        let lo = i64::from_le_bytes([u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]]);
        self.get(L_SP);
        self.w.byte(I64_CONST);
        self.w.s(lo);
        self.mem(I64_STORE, 3, r * VS);
    }
    /// A plain call's frame pushed as renter pushes it, for the callee the
    /// site's call cache at `a` names (h_call fills it: proto, table
    /// index, nparams, nslots, fsize, entry, strict), then the cell as
    /// direct() sets it; any other case fails to H_CALL.
    fn call_push(&mut self, a: u32, next: i64, dst: i32, f: i32, argc: i32, pc: i64) {
        let l = self.lay;
        let cell = cell_addr() as i64;
        let ld = |cg: &mut Cg<'a>, k: u32| {
            cg.i32c(a as i64);
            cg.mem(I32_LOAD, 2, k * 4);
        };
        let fo = f as u32 * VS;
        // f: a script function of the cached proto
        self.need_obj(L_SP, fo, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, fo + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_class);
        self.i32c(C_FUNCTION as i64);
        self.op(I32_NE);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_func);
        ld(self, 0);
        self.op(I32_NE);
        self.op(I32_OR);
        // no collection due, no slices, not too deep
        self.get(L_VM);
        self.mem(I32_LOAD8_U, 0, l.vm_gc_flag);
        self.op(I32_OR);
        self.get(L_VM);
        self.mem(I32_LOAD8_U, 0, l.vm_slicing);
        self.op(I32_OR);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 36);
        self.i32c(DEPTH_MAX as i64);
        self.op(I32_GE_S);
        self.op(I32_OR);
        // room for the frame
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_len);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_cap);
        self.op(I32_GE_U);
        self.op(I32_OR);
        // the stack as long as the callee's frame: (bp + f + 2) + fsize
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_stack_len);
        self.get(L_BP);
        self.i32c(f as i64 + 2);
        self.op(I32_ADD);
        ld(self, 4);
        self.op(I32_ADD);
        self.op(I32_LT_U);
        self.op(I32_OR);
        // a sloppy function's this must be an object already
        ld(self, 6);
        self.op(I32_EQZ);
        self.rtag(L_SP, fo + VS);
        self.i32c(l.t_obj as i64);
        self.op(I32_NE);
        self.op(I32_AND);
        self.op(I32_OR);
        self.fail_if(pc);
        // the caller goes on after the call
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_ptr);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_len);
        self.i32c(1);
        self.op(I32_SUB);
        self.i32c(l.frame_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.w.byte(I64_CONST);
        self.w.s(next);
        self.mem(I64_STORE, 3, l.fr_pc);
        // the callee's locals past the arguments it got: undefined
        // (L_T3 the slot address, L_T4 the end)
        let keep = |cg: &mut Cg<'a>| {
            // min(argc, nparams)
            ld(cg, 2);
            cg.i32c(argc as i64);
            ld(cg, 2);
            cg.i32c(argc as i64);
            cg.op(I32_LT_S);
            cg.op(SELECT);
        };
        let cbp = (f as u32 + 2) * VS;
        self.get(L_SP);
        self.i32c(cbp as i64);
        self.op(I32_ADD);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_C as u64);
        keep(self);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T3);
        self.get(L_C);
        ld(self, 3);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T4);
        let u = bytes_of(Val::Undef());
        let lo = i64::from_le_bytes([u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]]);
        self.op(BLOCK);
        self.op(T_VOID);
        self.op(LOOP);
        self.op(T_VOID);
        self.get(L_T3);
        self.get(L_T4);
        self.op(I32_GE_U);
        self.op(BR_IF);
        self.w.u(1);
        self.get(L_T3);
        self.w.byte(I64_CONST);
        self.w.s(lo);
        self.mem(I64_STORE, 3, 0);
        self.get(L_T3);
        self.i32c(VS as i64);
        self.op(I32_ADD);
        self.set(L_T3);
        self.op(BR);
        self.w.u(0);
        self.op(END);
        self.op(END);
        // the frame: frames_ptr + len * size (L_T3)
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_ptr);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_len);
        self.i32c(l.frame_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T3);
        let st_i32 = |cg: &mut Cg<'a>, off: u32, v: &dyn Fn(&mut Cg<'a>)| {
            cg.get(L_T3);
            v(cg);
            cg.op(I64_EXTEND_I32_S);
            cg.mem(I64_STORE, 3, off);
        };
        let st_k = |cg: &mut Cg<'a>, off: u32, k: i64| {
            cg.get(L_T3);
            cg.w.byte(I64_CONST);
            cg.w.s(k);
            cg.mem(I64_STORE, 3, off);
        };
        st_i32(self, l.fr_proto, &|cg| ld(cg, 0));
        st_i32(self, l.fr_fobj, &|cg| {
            cg.get(L_SP);
            cg.mem(I32_LOAD, 2, fo + l.off_int);
        });
        st_i32(self, l.fr_pc, &|cg| ld(cg, 5));
        // bp = this frame's bp + f + 2
        st_i32(self, l.fr_bp, &|cg| {
            cg.get(L_BP);
            cg.i32c(f as i64 + 2);
            cg.op(I32_ADD);
        });
        st_i32(self, l.frame_env, &|cg| {
            cg.get(L_T2);
            cg.mem(I32_LOAD, 2, l.off_env);
        });
        // this_val = the slot after f
        self.get(L_T3);
        self.get(L_SP);
        self.mem(I64_LOAD, 3, fo + VS);
        self.mem(I64_STORE, 3, l.frame_this);
        st_k(self, l.fr_new_target, lo);
        st_k(self, l.fr_args_obj, -1);
        self.get(L_T3);
        self.i32c(0);
        self.mem(I32_STORE8, 0, l.fr_construct);
        st_k(self, l.fr_gen, -1);
        st_i32(self, l.fr_ret, &|cg| {
            cg.get(L_BP);
            cg.i32c(dst as i64);
            cg.op(I32_ADD);
        });
        st_k(self, l.fr_mv_from, -1);
        st_k(self, l.fr_mv_to, -1);
        // frames.len + 1
        self.get(L_VM);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_len);
        self.i32c(1);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, l.vm_frames_len);
        // the cell as direct() and fill_from leave it: the callee's sp and
        // this; entry, table index, bp, sp; depth + 1
        self.i32c(cell);
        self.get(L_C);
        self.mem(I32_STORE, 2, 0);
        self.i32c(cell);
        self.get(L_T3);
        self.i32c(l.frame_this as i64);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, 20);
        self.i32c(cell);
        ld(self, 5);
        self.mem(I32_STORE, 2, 8);
        self.i32c(cell);
        ld(self, 1);
        self.mem(I32_STORE, 2, 28);
        self.i32c(cell);
        self.get(L_BP);
        self.i32c(f as i64 + 2);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, 32);
        self.i32c(cell);
        self.get(L_C);
        self.mem(I32_STORE, 2, 40);
        self.i32c(cell);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 36);
        self.i32c(1);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, 36);
    }
    /// R_SET_PROP adding property `atom` in place, as the cached transition
    /// t = before * 65536 + after says (rvm.rs r_set_prop_slow): a plain
    /// extensible object of shape before, no proxy made, no setter for the
    /// atom, room in its vals; else the helper.
    fn set_prop_add(&mut self, obj: u32, v: i32, t: i32, atom: i32, pc: i64) -> bool {
        self.prop_add(obj, v, t, atom, false, pc)
    }
    /// set_prop_add, or for `define` (R_INIT_PROP of a literal) without
    /// the setter, proxy and extensible checks a definition does not make
    fn prop_add(&mut self, obj: u32, v: i32, t: i32, atom: i32, define: bool, pc: i64) -> bool {
        let l = self.lay;
        let vo = self.raw(v);
        if let Opnd::K(k) = vo {
            match (self.kvals.get(k as usize)).map(|v| v.kind()) {
                Some(VK::Str(_)) | Some(VK::Obj(_)) | None => return false,
                _ => {}
            }
        }
        let (vb, voff) = self.place(vo);
        // the value: no string (Vm::inc's other case)
        self.rtag(vb, voff);
        self.i32c(l.t_str as i64);
        self.op(I32_EQ);
        self.fail_if(pc);
        if !define {
        self.get(L_VM);
        self.mem(I32_LOAD8_U, 0, l.vm_proxy_made);
        self.fail_if(pc);
        // setter_atoms[atom]
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_setters_len);
        self.i32c(atom as i64);
        self.op(I32_GT_S);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_setters_ptr);
        self.mem(I32_LOAD8_U, 0, atom as u32);
        self.fail_if(pc);
        self.extra -= 1;
        self.op(END);
        }
        self.need_obj(L_SP, obj * VS, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, obj * VS + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_shape);
        self.i32c((t >> 16) as i64);
        self.op(I32_NE);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_class);
        self.i32c(C_OBJECT as i64);
        self.op(I32_NE);
        self.op(I32_OR);
        self.get(L_T2);
        self.mem(I32_LOAD8_U, 0, l.off_ext);
        self.op(I32_EQZ);
        self.op(I32_OR);
        self.get(L_T2);
        self.vals_hdr();
        self.mem(I32_LOAD, 2, l.off_vals_len);
        self.get(L_T2);
        self.vals_hdr();
        self.mem(I32_LOAD, 2, l.off_vals_cap);
        self.op(I32_GE_U);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_nv);
        self.i32c(2);
        self.op(I32_GE_S);
        self.op(I32_AND);
        self.op(I32_OR);
        self.fail_if(pc);
        // shape = after; slot nv = v (counted): inl0/inl1 while nv < 2,
        // else vals[len] and len + 1; nv + 1
        self.get(L_T2);
        self.i32c((t & 65535) as i64);
        self.mem(I32_STORE, 2, l.off_shape);
        self.rtag(vb, voff);
        self.i32c(l.t_obj as i64);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.get(vb);
        self.mem(I32_LOAD, 2, voff + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T4 as u64);
        self.get(L_T4);
        self.mem(I32_LOAD, 2, l.off_rc);
        self.i32c(1);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, l.off_rc);
        self.op(END);
        // L_T3 = &inl[nv] when nv < 2, else &vals[len] (and len + 1)
        self.get(L_T2);
        self.i32c(l.off_inl0 as i64);
        self.op(I32_ADD);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_nv);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T3);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_nv);
        self.i32c(2);
        self.op(I32_GE_S);
        self.op(IF);
        self.op(T_VOID);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_vals);
        self.get(L_T2);
        self.vals_hdr();
        self.mem(I32_LOAD, 2, l.off_vals_len);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_T3);
        self.get(L_T2);
        self.vals_hdr();
        self.get(L_T2);
        self.vals_hdr();
        self.mem(I32_LOAD, 2, l.off_vals_len);
        self.i32c(1);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, l.off_vals_len);
        self.op(END);
        self.get(L_T3);
        self.get(vb);
        self.mem(I64_LOAD, 3, voff);
        self.mem(I64_STORE, 3, 0);
        self.get(L_T2);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_nv);
        self.i32c(1);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, l.off_nv);
        true
    }
    /// prop_cache through the module's pget: dst = the value at b + off
    /// read through the cache at `a` (flags 0 a property, 1 a method that
    /// must be a function, 2 a method)
    fn pget(&mut self, dst: u32, b: u32, off: u32, a: u32, flags: i64, pc: i64) -> bool {
        let global = self.cur_kind == R_GET_GLOBAL as u32;
        if flags == 0 && (global || MASK.with(|m| m.get()) & (1u64 << 53) != 0) {
            // the cache's first way, an own data property, here: of the
            // global object also under its shape epoch (the global's shape
            // is its own), else of a shared shape (experiment, mask 53);
            // any other case through pget
            let l = self.lay;
            let ai = a as i64;
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            self.rtag(b, off);
            self.i32c(l.t_obj as i64);
            self.op(I32_NE);
            self.op(BR_IF);
            self.w.u(0);
            self.get(b);
            self.mem(I32_LOAD, 2, off + l.off_int);
            self.obj_addr();
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T2 as u64);
            self.mem(I32_LOAD, 2, l.off_shape);
            self.i32c(ai);
            self.mem(I32_LOAD, 2, 0);
            self.op(I32_NE);
            self.i32c(ai);
            self.mem(I32_LOAD, 2, 4);
            self.op(I32_OR);
            self.i32c(ai);
            self.mem(I32_LOAD, 2, 12);
            self.i32c(-1);
            self.op(I32_NE);
            if global {
                self.i32c(ai);
                self.mem(I32_LOAD, 2, 12);
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.off_epoch);
                self.op(I32_NE);
                self.op(I32_AND);
            }
            self.op(I32_OR);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_class);
            self.i32c(ai);
            self.mem(I32_LOAD, 2, 16);
            self.op(I32_NE);
            self.op(I32_OR);
            self.i32c(ai);
            self.mem(I32_LOAD, 2, 8);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T3 as u64);
            self.i32c(0);
            self.op(I32_LT_S);
            self.op(I32_OR);
            self.op(BR_IF);
            self.w.u(0);
            // slot * VS < 2 * VS in the object, else in vals
            self.get(L_T2);
            self.i32c(l.off_inl0 as i64);
            self.op(I32_ADD);
            self.get(L_T3);
            self.op(I32_ADD);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_vals);
            self.get(L_T3);
            self.op(I32_ADD);
            self.i32c(2 * VS as i64);
            self.op(I32_SUB);
            self.get(L_T3);
            self.i32c(2 * VS as i64);
            self.op(I32_LT_U);
            self.op(SELECT);
            self.set(L_T4);
            self.get(L_T4);
            self.mem(I64_LOAD, 3, 0);
            self.set(L_Q0);
            self.store_val(dst, L_Q0);
            self.op(BR);
            self.w.u(1);
            self.extra -= 1;
            self.op(END);
            self.pget_call(dst, b, off, a, flags, pc);
            self.extra -= 1;
            self.op(END);
            return true;
        }
        self.pget_call(dst, b, off, a, flags, pc)
    }
    /// pget of a plain property whose site the interpreter's inline cache
    /// `ic` (shape * 65536 + slot, rrun's R_GET_PROP) saw monomorphic when
    /// this was compiled: that shape's slot read here, any other through
    /// pget
    fn pget_ic(&mut self, dst: u32, b: u32, off: u32, a: u32, ic: i32, pc: i64) -> bool {
        if ic < 65536 || MASK.with(|m| m.get()) & (1u64 << 46) != 0 {
            return self.pget(dst, b, off, a, 0, pc);
        }
        let l = self.lay;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.rtag(b, off);
        self.i32c(l.t_obj as i64);
        self.op(I32_NE);
        self.op(BR_IF);
        self.w.u(0);
        self.get(b);
        self.mem(I32_LOAD, 2, off + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_shape);
        self.i32c((ic >> 16) as i64);
        self.op(I32_NE);
        self.op(BR_IF);
        self.w.u(0);
        let slot = (ic & 65535) as u32;
        let at = if slot < 2 {
            l.off_inl0 + slot * VS
        } else {
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_vals);
            self.set(L_T2);
            (slot - 2) * VS
        };
        self.get(L_T2);
        self.mem(I64_LOAD, 3, at);
        self.set(L_Q0);
        self.store_val(dst, L_Q0);
        self.op(BR);
        self.w.u(1);
        self.extra -= 1;
        self.op(END);
        self.pget(dst, b, off, a, 0, pc);
        self.extra -= 1;
        self.op(END);
        true
    }
    /// A read whose site's interpreter cache knew a data property one
    /// prototype up (Cg.pics): the receiver's shape, its prototype and the
    /// prototype's shape checked here and the slot read, any other case
    /// through pget_ic (ic >= 0, a property) or pget
    fn pget_pic(&mut self, dst: u32, b: u32, off: u32, a: u32, flags: i64, ic: i32, pc: i64) -> bool {
        let at = pc as usize * 4;
        let fallback = |cg: &mut Self| if flags == 0 { cg.pget_ic(dst, b, off, a, ic, pc) } else { cg.pget(dst, b, off, a, flags, pc) };
        if at + 3 >= self.pics.len() || self.pics[at] < 0 || MASK.with(|m| m.get()) & (1u64 << 44) != 0 {
            return fallback(self);
        }
        let (sh, pr, ps, slot) = (self.pics[at], self.pics[at + 1], self.pics[at + 2], self.pics[at + 3] as u32);
        let l = self.lay;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.rtag(b, off);
        self.i32c(l.t_obj as i64);
        self.op(I32_NE);
        self.op(BR_IF);
        self.w.u(0);
        self.get(b);
        self.mem(I32_LOAD, 2, off + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_shape);
        self.i32c(sh as i64);
        self.op(I32_NE);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_proto);
        self.i32c(pr as i64);
        self.op(I32_NE);
        self.op(I32_OR);
        self.op(BR_IF);
        self.w.u(0);
        self.i32c(pr as i64);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_shape);
        self.i32c(ps as i64);
        self.op(I32_NE);
        self.op(BR_IF);
        self.w.u(0);
        let vat = if slot < 2 {
            l.off_inl0 + slot * VS
        } else {
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_vals);
            self.set(L_T2);
            (slot - 2) * VS
        };
        self.get(L_T2);
        self.mem(I64_LOAD, 3, vat);
        self.set(L_Q0);
        if flags == 1 {
            // a script or native function (prop_cache's check)
            self.rtag(L_T2, vat);
            self.i32c(l.t_obj as i64);
            self.op(I32_NE);
            self.op(BR_IF);
            self.w.u(0);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, vat + l.off_int);
            self.obj_addr();
            self.mem(I32_LOAD, 2, l.off_class);
            self.set(L_T3);
            self.get(L_T3);
            self.i32c(C_FUNCTION as i64);
            self.op(I32_NE);
            self.get(L_T3);
            self.i32c(C_NATIVE as i64);
            self.op(I32_NE);
            self.op(I32_AND);
            self.op(BR_IF);
            self.w.u(0);
        }
        if flags != 0 {
            self.get(b);
            self.mem(I64_LOAD, 3, off);
            self.set(L_Q2);
        }
        self.store_val(dst, L_Q0);
        if flags != 0 {
            self.store_val(dst + 1, L_Q2);
        }
        self.op(BR);
        self.w.u(1);
        self.extra -= 1;
        self.op(END);
        fallback(self);
        self.extra -= 1;
        self.op(END);
        true
    }
    fn pget_call(&mut self, dst: u32, b: u32, off: u32, a: u32, flags: i64, pc: i64) -> bool {
        self.get(L_SP);
        if dst != 0 {
            self.i32c(dst as i64 * VS as i64);
            self.op(I32_ADD);
        }
        self.i32c(a as i64);
        self.get(b);
        if off != 0 {
            self.i32c(off as i64);
            self.op(I32_ADD);
        }
        self.i32c(flags);
        self.get(L_VM);
        self.get(L_BP);
        self.op(CALL);
        self.w.u(F_PGET as u64);
        self.fail_if(pc);
        true
    }
    /// The body of pget(dst address, cache, value address, flags, vm, bp):
    /// 0 read, 1 the helper must do it
    fn pget_body(&mut self) -> Vec<u8> {
        let saved_w = std::mem::replace(&mut self.w, WBuf::new());
        let (se, sx, ss) = (self.extra, self.exit_d, self.slow_at);
        self.w.u(4);
        self.w.u(3);
        self.w.byte(T_I32);
        self.w.u(2);
        self.w.byte(T_F64);
        self.w.u(4);
        self.w.byte(T_I64);
        self.w.u(7);
        self.w.byte(T_I32);
        self.extra = 0;
        self.exit_d = 0;
        for flags in 0..3 {
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            self.slow_at = Some(self.extra);
            if flags < 2 {
                self.get(3);
                self.i32c(flags);
                self.op(I32_NE);
                self.op(BR_IF);
                self.w.u(0);
            }
            // parameter 2 is the value's address (L_KEY's place)
            self.prop_cache(0, 2, 0, u32::MAX, flags != 0, if flags == 1 { 0 } else { 1 }, 0);
            self.i32c(0);
            self.op(RETURN);
            self.extra -= 1;
            self.op(END);
            if flags < 2 {
                // a miss of this kind: 1; another kind: the next block
                self.get(3);
                self.i32c(flags);
                self.op(I32_EQ);
                self.op(IF);
                self.op(T_VOID);
                self.i32c(1);
                self.op(RETURN);
                self.op(END);
            }
        }
        self.i32c(1);
        self.op(END);
        let out = std::mem::replace(&mut self.w, saved_w).b;
        self.extra = se;
        self.exit_d = sx;
        self.slow_at = ss;
        out
    }
    /// A property read (R_GET_PROP, R_GET_THIS_PROP) or a method's
    /// (R_GET_METHOD: f and this into dst, dst + 1) of the object value at
    /// b + off through the site's cache at address `a`: ways of shape,
    /// depth (0 own), slot * 16, shape epoch, class, then each prototype
    /// up to the holder and its shape, which the helpers fill on a miss. A method
    /// must be a script function unless sub says it need not be callable.
    fn prop_cache(&mut self, dst: u32, b: u32, off: u32, a: u32, method: bool, sub: int, pc: i64) -> bool {
        let l = self.lay;
        if method {
            // L_T5 = the object, or a string's or number's prototype
            self.tag_at(b, off);
            self.set(L_T6);
            self.get(b);
            self.mem(I32_LOAD, 2, off + l.off_int);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_string_proto);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_number_proto);
            self.get(L_T6);
            self.i32c(l.t_str as i64);
            self.op(I32_EQ);
            self.op(SELECT);
            self.get(L_T6);
            self.i32c(l.t_obj as i64);
            self.op(I32_EQ);
            self.op(SELECT);
            self.set(L_T5);
            self.get(L_T6);
            self.i32c(l.t_obj as i64);
            self.op(I32_NE);
            self.get(L_T6);
            self.i32c(l.t_str as i64);
            self.op(I32_NE);
            self.op(I32_AND);
            self.get(L_T6);
            self.i32c(l.t_num as i64);
            self.op(I32_NE);
            self.op(I32_AND);
            self.fail_if(pc);
        } else {
            self.need_obj(b, off, pc);
            self.get(b);
            self.mem(I32_LOAD, 2, off + l.off_int);
            self.set(L_T5);
        }
        if method {
            self.get(b);
            self.mem(I64_LOAD, 3, off);
            self.set(L_Q2);
        }
        // look(&objs[id], cache, epoch): &value, or 0 (a miss)
        self.get(L_T5);
        self.obj_addr();
        if a == u32::MAX {
            // pget_body: the cache is its parameter 1
            self.get(1);
        } else {
            self.i32c(a as i64);
        }
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.off_epoch);
        self.op(CALL);
        self.w.u(F_LOOK as u64);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T4 as u64);
        self.op(I32_EQZ);
        self.fail_if(pc);
        if method && sub == 0 {
            // a script or native function (is_callable's common cases)
            self.need_obj(L_T4, 0, pc);
            self.get(L_T4);
            self.mem(I32_LOAD, 2, l.off_int);
            self.obj_addr();
            self.mem(I32_LOAD, 2, l.off_class);
            self.set(L_T3);
            self.get(L_T3);
            self.i32c(C_FUNCTION as i64);
            self.op(I32_NE);
            self.get(L_T3);
            self.i32c(C_NATIVE as i64);
            self.op(I32_NE);
            self.op(I32_AND);
            self.fail_if(pc);
        }
        self.get(L_T4);
        self.mem(I64_LOAD, 3, 0);
        self.set(L_Q0);
        self.store_val(dst, L_Q0);
        if method {
            self.store_val(dst + 1, L_Q2);
        }
        true
    }
    /// x == y, x === y and their negations as an i32 on the wasm stack:
    /// numbers, undefined, null, booleans and objects here; two strings
    /// that are not the same handle, and a loose compare across types other
    /// than undefined and null, fail to the slow path
    fn equal(&mut self, c: int, x: i32, y: i32, pc: i64) {
        let strict = c == OP_SEQ || c == OP_SNE;
        let a = self.opnd(x);
        let b = self.opnd(y);
        let l = self.lay;
        self.tag(a);
        self.set(L_TAG);
        self.tag(b);
        self.set(L_T2);
        self.op(BLOCK);
        self.op(T_I32);
        self.extra += 1;
        // two numbers
        self.get(L_TAG);
        self.i32c(l.t_num as i64);
        self.op(I32_EQ);
        self.get(L_T2);
        self.i32c(l.t_num as i64);
        self.op(I32_EQ);
        self.op(I32_AND);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        self.num(a);
        self.num(b);
        self.op(F64_EQ);
        self.op(BR);
        self.w.u(1);
        self.extra -= 1;
        self.op(END);
        // other types
        self.get(L_TAG);
        self.get(L_T2);
        self.op(I32_NE);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        if strict {
            self.i32c(0);
            self.op(BR);
            self.w.u(1);
        } else {
            // undefined or null on either side: equal when both are one
            // of them (null == x holds only for undefined and null);
            // anything else across types is the slow path's
            self.get(L_TAG);
            self.i32c(l.t_undef as i64);
            self.op(I32_EQ);
            self.get(L_TAG);
            self.i32c(l.t_null as i64);
            self.op(I32_EQ);
            self.op(I32_OR);
            self.set(L_C);
            self.get(L_T2);
            self.i32c(l.t_undef as i64);
            self.op(I32_EQ);
            self.get(L_T2);
            self.i32c(l.t_null as i64);
            self.op(I32_EQ);
            self.op(I32_OR);
            self.set(L_T3);
            self.get(L_C);
            self.get(L_T3);
            self.op(I32_OR);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.get(L_C);
            self.get(L_T3);
            self.op(I32_AND);
            self.op(BR);
            self.w.u(2);
            self.extra -= 1;
            self.op(END);
            self.fail(pc);
        }
        self.extra -= 1;
        self.op(END);
        // the same type
        let (ab, ao) = self.place(a);
        let (bb, bo) = self.place(b);
        let num_k = matches!(a, Opnd::Num(_)) || matches!(b, Opnd::Num(_));
        if !num_k {
            // two small BigInts: their bits
            self.get(L_TAG);
            self.i32c(l.t_big as i64);
            self.op(I32_EQ);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.get(ab);
            self.mem(I64_LOAD, 3, ao);
            self.get(bb);
            self.mem(I64_LOAD, 3, bo);
            self.op(I64_EQ);
            self.op(BR);
            self.w.u(1);
            self.extra -= 1;
            self.op(END);
            // bool: its byte
            self.get(L_TAG);
            self.i32c(l.t_bool as i64);
            self.op(I32_EQ);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.get(ab);
            self.mem(I32_LOAD8_U, 0, ao + l.off_bool);
            self.get(bb);
            self.mem(I32_LOAD8_U, 0, bo + l.off_bool);
            self.op(I32_EQ);
            self.op(BR);
            self.w.u(1);
            self.extra -= 1;
            self.op(END);
            // an object or a string: the same id or handle; two string
            // handles that differ go to the slow path
            self.get(L_TAG);
            self.i32c(l.t_obj as i64);
            self.op(I32_EQ);
            self.get(L_TAG);
            self.i32c(l.t_str as i64);
            self.op(I32_EQ);
            self.op(I32_OR);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.get(ab);
            self.mem(I32_LOAD, 2, ao + l.off_int);
            self.get(bb);
            self.mem(I32_LOAD, 2, bo + l.off_int);
            self.op(I32_EQ);
            self.set(L_C);
            self.get(L_C);
            self.get(L_TAG);
            self.i32c(l.t_obj as i64);
            self.op(I32_EQ);
            self.op(I32_OR);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.get(L_C);
            self.op(BR);
            self.w.u(2);
            self.extra -= 1;
            self.op(END);
            // two string handles: unequal lengths, or two known atoms,
            // answer it; else the slow path compares the texts
            let sz = std::mem::size_of::<crate::vm::SEnt>() as i64;
            let o_len = std::mem::offset_of!(crate::vm::SEnt, len) as u32;
            let o_atom = std::mem::offset_of!(crate::vm::SEnt, atom) as u32;
            for (bx, ox, lt) in [(ab, ao, L_T3), (bb, bo, L_T4)] {
                self.get(bx);
                self.mem(I32_LOAD, 2, ox + l.off_int);
                self.i32c(sz);
                self.op(I32_MUL);
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.vm_srcs_ptr);
                self.op(I32_ADD);
                self.set(lt);
            }
            // SEnt.len is the byte length only for a prefix view (up >= 0),
            // 0 for a whole buffer: compare it when both are views
            let o_upv = std::mem::offset_of!(crate::vm::SEnt, up) as u32;
            self.get(L_T3);
            self.mem(I32_LOAD, 2, o_len);
            self.get(L_T4);
            self.mem(I32_LOAD, 2, o_len);
            self.op(I32_NE);
            self.get(L_T3);
            self.mem(I32_LOAD, 2, o_upv);
            self.get(L_T4);
            self.mem(I32_LOAD, 2, o_upv);
            self.op(I32_OR);
            self.i32c(0);
            self.op(I32_GE_S);
            self.op(I32_AND);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.i32c(0);
            self.op(BR);
            self.w.u(2);
            self.extra -= 1;
            self.op(END);
            self.get(L_T3);
            self.mem(I32_LOAD, 2, o_atom);
            self.get(L_T4);
            self.mem(I32_LOAD, 2, o_atom);
            self.op(I32_OR);
            self.i32c(0);
            self.op(I32_GE_S);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.get(L_T3);
            self.mem(I32_LOAD, 2, o_atom);
            self.get(L_T4);
            self.mem(I32_LOAD, 2, o_atom);
            self.op(I32_EQ);
            self.op(BR);
            self.w.u(2);
            self.extra -= 1;
            self.op(END);
            let o_up = std::mem::offset_of!(crate::vm::SEnt, up) as u32;
            if l.str_data != 0 {
                // two whole buffers (up < 0): their bytes (rt memeq)
                self.get(L_T3);
                self.mem(I32_LOAD, 2, o_up);
                self.get(L_T4);
                self.mem(I32_LOAD, 2, o_up);
                self.op(I32_AND);
                self.i32c(0);
                self.op(I32_LT_S);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                // the byte lengths (SEnt.len counts units): String.len
                let o_s = std::mem::offset_of!(crate::vm::SEnt, s) as u32;
                let blen = l.str_blen;
                self.get(L_T3);
                self.mem(I32_LOAD, 2, o_s);
                self.mem(I32_LOAD, 2, blen);
                self.set(L_T2);
                self.get(L_T4);
                self.mem(I32_LOAD, 2, o_s);
                self.mem(I32_LOAD, 2, blen);
                self.get(L_T2);
                self.op(I32_NE);
                self.op(IF);
                self.op(T_VOID);
                self.i32c(0);
                self.op(BR);
                self.w.u(3);
                self.op(END);
                self.get(L_T3);
                self.mem(I32_LOAD, 2, o_s);
                self.mem(I32_LOAD, 2, l.str_data);
                self.get(L_T4);
                self.mem(I32_LOAD, 2, o_s);
                self.mem(I32_LOAD, 2, l.str_data);
                self.get(L_T2);
                self.op(CALL);
                self.w.u(F_MEMEQ as u64);
                self.op(BR);
                self.w.u(2);
                self.extra -= 1;
                self.op(END);
            }
            self.fail(pc);
            self.extra -= 1;
            self.op(END);
        }
        // undefined or null (a number constant never gets here)
        self.i32c(1);
        self.extra -= 1;
        self.op(END);
        if c == OP_SNE || c == OP_NE {
            self.op(I32_EQZ);
        }
    }
    /// a binary number op into dst, or false when this file does not do it
    fn arith(&mut self, code: int, dst: i32, x: i32, y: i32, pc: i64) -> bool {
        let fop = match code {
            OP_ADD => F64_ADD,
            OP_SUB => F64_SUB,
            OP_MUL => F64_MUL,
            OP_DIV => F64_DIV,
            OP_BAND | OP_BOR | OP_BXOR | OP_SHL | OP_SHR => 0,
            _ => return false,
        };
        if dst < 0 {
            return false;
        }
        let a = self.opnd(x);
        let b = self.opnd(y);
        self.need_num(a, pc);
        self.need_num(b, pc);
        if fop != 0 {
            self.num(a);
            self.num(b);
            self.op(fop);
            self.set(L_A);
            self.store_num(dst as u32, L_A);
            return true;
        }
        // int32 operands only (the interpreter's fast path); others return
        self.num(a);
        self.set(L_A);
        self.num(b);
        self.set(L_B);
        for l in [L_A, L_B] {
            // !(v >= -2^31 && v <= 2^31 - 1)
            self.get(l);
            self.op(F64_CONST);
            self.w.b.extend_from_slice(&(-2147483648.0f64).to_le_bytes());
            self.op(F64_GE);
            self.get(l);
            self.op(F64_CONST);
            self.w.b.extend_from_slice(&2147483647.0f64.to_le_bytes());
            self.op(F64_LE);
            self.op(I32_AND);
            self.op(I32_EQZ);
            self.fail_if(pc);
        }
        self.get(L_A);
        self.op(I32_TRUNC_F64_S);
        self.get(L_B);
        self.op(I32_TRUNC_F64_S);
        self.op(match code {
            OP_BAND => I32_AND,
            OP_BOR => I32_OR,
            OP_BXOR => I32_XOR,
            OP_SHL => I32_SHL,
            _ => I32_SHR_S,
        });
        self.op(F64_CONVERT_I32_S);
        self.set(L_A);
        self.store_num(dst as u32, L_A);
        true
    }

    /// A helper for the op at pc: 0 it did it, 1 it did not (return pc),
    /// 2 it did and the interpreter goes on after it (return next).
    fn helper(&mut self, h: u32, pc: i64, next: i64) {
        self.helper_raw(h, pc);
        self.helper_codes(pc, next);
    }
    /// R_CALL: H_CALL, and when it pushed a compiled callee's frame (6) the
    /// call of its code from here and H_RET for what it returned
    fn call_op(&mut self, pc: i64, next: i64, plain: bool, dst: i32, f: i32, argc: i32, sub: int) {
        let inl = if plain && dst >= 0 && f >= 0 && argc >= 0 && MASK.with(|m| m.get()) & (1u64 << 43) == 0 {
            self.inl.get(pc as usize).cloned().flatten()
        } else {
            None
        };
        if let Some(c) = inl {
            let (b0, e0, s0, d0) = (self.w.b.len(), self.extra, self.slow_at, self.done);
            if self.inline_call(&c, pc, next, dst, f, argc, sub) {
                return;
            }
            // not writable after all: as if never tried
            self.w.b.truncate(b0);
            self.extra = e0;
            self.slow_at = s0;
            self.done = d0;
            self.inlining = false;
            self.inl_this = None;
        }
        self.call_op_plain(pc, next, plain, dst, f, argc, sub);
    }

    /// A call to the site's one callee c with its body here (Inl): the
    /// function object checked to be c's, the callee's registers from the
    /// call's f + 2 (sp and bp moved there while its ops are written), its
    /// return value into dst. Any check that fails, before or inside the
    /// body, goes to the normal call (call_op_plain), which starts the
    /// callee afresh: the body writes nothing the call would read. False
    /// when the body cannot be written so.
    fn inline_call(&mut self, c: &Inl, pc: i64, next: i64, dst: i32, f: i32, argc: i32, sub: int) -> bool {
        let l = self.lay;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        let done_at = self.extra;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        let saved_slow = self.slow_at;
        self.slow_at = Some(self.extra);
        self.inlining = true;
        self.inl_bad = false;
        self.get(L_SP);
        self.set(L_SSP);
        self.get(L_BP);
        self.set(L_SBP);
        if sub == 1 {
            self.store_undef(f as u32 + 1);
        }
        // f: c's function object; room on the stack for its frame
        let fo = f as u32 * VS;
        self.need_obj(L_SP, fo, pc);
        self.get(L_SP);
        self.mem(I32_LOAD, 2, fo + l.off_int);
        self.obj_addr();
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T2 as u64);
        self.mem(I32_LOAD, 2, l.off_class);
        self.i32c(C_FUNCTION as i64);
        self.op(I32_NE);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_func);
        self.i32c(c.tp as i64);
        self.op(I32_NE);
        self.op(I32_OR);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_stack_len);
        self.get(L_BP);
        self.i32c(f as i64 + 2 + c.fsize as i64);
        self.op(I32_ADD);
        self.op(I32_LT_U);
        self.op(I32_OR);
        self.fail_if(pc);
        self.get(L_T2);
        self.mem(I32_LOAD, 2, l.off_env);
        self.set(L_SENV);
        // the callee's frame: sp and bp at f + 2
        self.get(L_SP);
        self.i32c((f as i64 + 2) * VS as i64);
        self.op(I32_ADD);
        self.set(L_SP);
        self.get(L_BP);
        self.i32c(f as i64 + 2);
        self.op(I32_ADD);
        self.set(L_BP);
        // its locals past the arguments it got start undefined (renter)
        let keep = if (argc as u32) < c.nparams { argc as u32 } else { c.nparams };
        for r in keep..c.nslots {
            self.store_undef(r);
        }
        let ctx = (self.pi, self.rb, std::mem::take(&mut self.caches), std::mem::take(&mut self.atoms), self.fsize);
        self.pi = c.tp;
        self.rb = c.rb;
        self.caches = c.caches.clone();
        self.atoms = c.atoms.clone();
        self.fsize = c.fsize;
        self.inl_this = Some(((f as u32 + 1) * VS, c.strict));
        let mut cp = c.entry;
        let mut ret = false;
        while cp < c.code.len() && !self.inl_bad {
            let ins = c.code[cp];
            let k = (ins.code & 255) as int;
            if k == R_RET {
                let o = self.raw(ins.a);
                self.load_val(o, L_Q0);
                ret = true;
                break;
            }
            if k == R_RET_UNDEF {
                let u = bytes_of(Val::Undef());
                self.w.byte(I64_CONST);
                self.w.s(i64::from_le_bytes([u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]]));
                self.set(L_Q0);
                ret = true;
                break;
            }
            if k == R_GET_THIS_PROP && c.fwd.get(cp).map_or(false, |&v| v != i32::MIN) && ins.a >= 0 {
                // the property just stored on this: its value
                let o = self.raw(c.fwd[cp]);
                self.load_val(o, L_Q0);
                self.store_val(ins.a as u32, L_Q0);
                cp += 1;
                continue;
            }
            self.cur_kind = k as u32;
            if !self.one(cp, ins) {
                self.inl_bad = true;
            }
            cp += 1 + if k == R_NOP { (ins.code >> 16) as usize } else { 0 };
        }
        self.cur_kind = R_CALL as u32;
        self.inl_this = None;
        self.pi = ctx.0;
        self.rb = ctx.1;
        self.caches = ctx.2;
        self.atoms = ctx.3;
        self.fsize = ctx.4;
        if !ret || self.inl_bad {
            return false;
        }
        // back to the caller's frame; the value into dst
        self.get(L_SSP);
        self.set(L_SP);
        self.get(L_SBP);
        self.set(L_BP);
        self.store_val(dst as u32, L_Q0);
        self.inlining = false;
        self.op(BR);
        self.w.u((self.extra - done_at) as u64);
        self.op(END);
        self.extra -= 1;
        self.slow_at = saved_slow;
        // a check failed: the caller's frame again and the call itself
        self.get(L_SSP);
        self.set(L_SP);
        self.get(L_SBP);
        self.set(L_BP);
        self.call_op_plain(pc, next, true, dst, f, argc, 0);
        self.op(END);
        self.extra -= 1;
        true
    }

    fn call_op_plain(&mut self, pc: i64, next: i64, plain: bool, dst: i32, f: i32, argc: i32, sub: int) {
        let cell = cell_addr() as i64;
        let l = self.lay;
        let a = self.caches.get(pc as usize).copied().unwrap_or(0);
        if plain && a != 0 && dst >= 0 && f >= 0 && argc >= 0 && MASK.with(|m| m.get()) & (1u64 << 57) == 0 && MASK.with(|m| m.get()) & (1u64 << 56) == 0 {
            // the call in the module's callx (one copy of the frame push,
            // the callee's call and the return), its operands in CARGS
            if sub == 1 {
                self.store_undef(f as u32 + 1);
            }
            // the operands in the site's record after its cache (call_rec),
            // the cache's address in CARGS[6]
            let ca = cargs_addr() as i64;
            let rb = self.rb as i64;
            for (k, v) in [f as i64, dst as i64, argc as i64, next, pc, rb + pc, a as i64, self.pi as i64, self.fsize as i64].iter().enumerate() {
                // SAFETY: site_caches made the record CALL_REC words long
                unsafe { *((a as usize + CACHE_N * 4 + k * 4) as *mut i32) = *v as i32 };
            }
            self.i32c(ca);
            self.i32c(a as i64);
            self.mem(I32_STORE, 2, 6 * 4);
            for l in [L_SP, L_KV, L_KEY, L_BUDGET, L_VM, L_BP] {
                self.get(l);
            }
            self.op(CALL);
            self.w.u(F_CALLX as u64);
            self.set(L_TAG);
            self.reload();
            self.callx = true;
            self.helper_codes(pc, next);
            return;
        }
        if plain && a != 0 && dst >= 0 && f >= 0 && argc >= 0 && MASK.with(|m| m.get()) & (1u64 << 57) == 0 {
            if sub == 1 {
                self.store_undef(f as u32 + 1);
            }
            // block $done: the frame pushed here (L_TAG 6) or by H_CALL
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            let saved = self.slow_at;
            self.slow_at = Some(self.extra);
            self.call_push(a, next, dst, f, argc, pc);
            self.slow_at = saved;
            self.i32c(6);
            self.set(L_TAG);
            self.op(BR);
            self.w.u(1);
            self.extra -= 1;
            self.op(END);
            self.helper_raw(H_CALL, pc);
            self.extra -= 1;
            self.op(END);
        } else {
            self.helper_raw(H_CALL, pc);
        }
        self.get(L_TAG);
        self.i32c(6);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        // the handlers before the call: a return with more needs H_RET
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_handlers_len);
        self.set(L_T3);
        // f(sp, kv, pc, budget, vm, bp) of the callee
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 40);
        self.get(L_KV);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 8);
        self.get(L_BUDGET);
        self.get(L_VM);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 32);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 28);
        self.op(CALL_INDIRECT);
        self.w.u(0);
        self.w.u(0);
        self.set(L_T2);
        if plain && dst >= 0 {
            // a return op, no handler left, the stack as long as this
            // frame, no collection due: the frame popped here
            self.get(L_T2);
            self.i32c(OUT_RET as i64);
            self.op(I32_EQ);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_handlers_len);
            self.get(L_T3);
            self.op(I32_EQ);
            self.op(I32_AND);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_stack_len);
            self.get(L_BP);
            self.i32c(self.fsize as i64);
            self.op(I32_ADD);
            self.op(I32_GE_U);
            self.op(I32_AND);
            self.get(L_VM);
            self.mem(I32_LOAD8_U, 0, l.vm_gc_flag);
            self.op(I32_EQZ);
            self.op(I32_AND);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            // frames.len -= 1; depth -= 1
            self.get(L_VM);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_len);
            self.i32c(1);
            self.op(I32_SUB);
            self.mem(I32_STORE, 2, l.vm_frames_len);
            self.i32c(cell);
            self.i32c(cell);
            self.mem(I32_LOAD, 2, 36);
            self.i32c(1);
            self.op(I32_SUB);
            self.mem(I32_STORE, 2, 36);
            // this frame's registers, the constants, its this (cell[5])
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_stack_ptr);
            self.get(L_BP);
            self.i32c(VS as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.set(L_SP);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_kvals_ptr);
            self.set(L_KV);
            self.i32c(cell);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_ptr);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_len);
            self.i32c(1);
            self.op(I32_SUB);
            self.i32c(l.frame_size as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.i32c(l.frame_this as i64);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, 20);
            // the value
            self.get(L_SP);
            self.i32c(cell);
            self.mem(I64_LOAD, 3, 48);
            self.mem(I64_STORE, 3, dst as u32 * VS);
            self.i32c(0);
            self.set(L_TAG);
            self.op(ELSE);
        }
        self.get(L_VM);
        self.get(L_BP);
        self.get(L_T2);
        self.i32c(pc);
        let rb = self.rb as i64;
        self.i32c(rb + pc);
        self.op(CALL);
        self.w.u(H_RET as u64);
        self.set(L_TAG);
        self.reload();
        if plain && dst >= 0 {
            self.extra -= 1;
            self.op(END);
        }
        self.extra -= 1;
        self.op(END);
        self.helper_codes(pc, next);
    }
    /// The module's callx(sp, kv, key, budget, vm, bp) -> helper code: a
    /// plain call with its operands in CARGS (call_op): the frame pushed
    /// here (call_push's checks) or by H_CALL, a compiled callee called,
    /// its return popped here or by H_RET; the caller reloads sp and kv.
    fn callx_body(&mut self) -> Vec<u8> {
        let saved_w = std::mem::replace(&mut self.w, WBuf::new());
        let (se, sx, ss) = (self.extra, self.exit_d, self.slow_at);
        self.extra = 0;
        self.exit_d = 0;
        let l = self.lay;
        let cell = cell_addr() as i64;
        let ca = cargs_addr() as i64;
        // locals as the main function's, then f, dst, argc, next, pc, ri,
        // a, fb, db
        self.w.u(5);
        self.w.u(3);
        self.w.byte(T_I32);
        self.w.u(2);
        self.w.byte(T_F64);
        self.w.u(4);
        self.w.byte(T_I64);
        self.w.u(7);
        self.w.byte(T_I32);
        self.w.u(12);
        self.w.byte(T_I32);
        // the site's cache from CARGS[6], the operands from its record
        self.i32c(ca);
        self.mem(I32_LOAD, 2, 6 * 4);
        self.set(L_CA);
        for (k, lc) in [L_F, L_DST, L_ARGC, L_NEXT, L_PC, L_RI, L_CA, L_CPI, L_CFS].iter().enumerate() {
            if k == 6 {
                continue;
            }
            self.get(L_CA);
            self.mem(I32_LOAD, 2, (CACHE_N + k) as u32 * 4);
            self.set(*lc);
        }
        // fb = &f, db = &dst
        for (lb, lr) in [(L_FB, L_F), (L_DB, L_DST)] {
            self.get(L_SP);
            self.get(lr);
            self.i32c(VS as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.set(lb);
        }
        let ld = |cg: &mut Cg<'a>, k: u32| {
            cg.get(L_CA);
            cg.mem(I32_LOAD, 2, k * 4);
        };
        // f + 2 on the stack
        let f2 = |cg: &mut Cg<'a>| {
            cg.get(L_F);
            cg.i32c(2);
            cg.op(I32_ADD);
        };
        // block $done / block $slow
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        self.slow_at = Some(self.extra);
        // f not an object: the slow call; f's object at L_FO, a native
        // one into block $nat (its fast paths), else on past it
        let f_obj = |cg: &mut Cg<'a>, out: u64| {
            cg.need_obj(L_FB, 0, 0);
            cg.get(L_FB);
            cg.mem(I32_LOAD, 2, l.off_int);
            cg.obj_addr();
            cg.w.byte(LOCAL_TEE);
            cg.w.u(L_FO as u64);
            cg.mem(I32_LOAD, 2, l.off_class);
            cg.i32c(C_NATIVE as i64);
            cg.op(I32_NE);
            cg.op(BR_IF);
            cg.w.u(out);
        };
        self.op(BLOCK);
        self.op(T_VOID);
        self.extra += 1;
        f_obj(self, 0);
        // f.call(this, a...): [call, f, this, a...] is f's call one slot
        // on (r_call_slow); H_CALL still reads the op as it is
        {
            self.op(BLOCK);
            self.op(T_VOID);
            self.extra += 1;
            self.get(L_ARGC);
            self.i32c(1);
            self.op(I32_LT_S);
            self.get(L_FO);
            self.mem(I32_LOAD, 2, l.off_func);
            self.i32c(crate::builtins::NF_CALL as i64);
            self.op(I32_NE);
            self.op(I32_OR);
            self.op(BR_IF);
            self.w.u(0);
            self.get(L_F);
            self.i32c(1);
            self.op(I32_ADD);
            self.set(L_F);
            self.get(L_ARGC);
            self.i32c(1);
            self.op(I32_SUB);
            self.set(L_ARGC);
            self.get(L_FB);
            self.i32c(VS as i64);
            self.op(I32_ADD);
            self.set(L_FB);
            // the function it calls, as f above
            f_obj(self, 1);
            self.extra -= 1;
            self.op(END);
        }
        // Math.abs, floor, ceil, round, sqrt of a number, min and max of
        // two (builtins.rs math1; wasm's min and max order -0 and NaN as
        // JavaScript does): the result into dst, done (0)
        {
            let m = crate::builtins::NF_MATH as i64;
            let tn = l.t_num as i64;
            let on = l.off_num;
            self.op(BLOCK);
            self.op(T_VOID);
            self.get(L_FO);
            self.mem(I32_LOAD, 2, l.off_func);
            self.i32c(m);
            self.op(I32_SUB);
            self.set(L_T3);
            // the first argument a number (L_A)
            self.get(L_ARGC);
            self.i32c(1);
            self.op(I32_LT_S);
            self.not_num(L_FB, 2 * VS);
            self.op(I32_OR);
            self.op(BR_IF);
            self.w.u(0);
            self.get(L_FB);
            self.mem(F64_LOAD, 3, 2 * VS + on);
            self.set(L_A);
            let done = |cg: &mut Cg<'a>| {
                // L_A into dst; 0 (the caller reloads its registers' address
                // from cell[0], which a callee's code may have moved on)
                cg.set(L_A);
                cg.i32c(cell);
                cg.get(L_SP);
                cg.mem(I32_STORE, 2, 0);
                cg.get(L_DB);
                cg.get(L_A);
                cg.mem(F64_STORE, 3, on);
                cg.i32c(0);
                cg.op(RETURN);
            };
            let is = |cg: &mut Cg<'a>, k: i64| {
                cg.get(L_T3);
                cg.i32c(k);
                cg.op(I32_EQ);
                cg.op(IF);
                cg.op(T_VOID);
            };
            for (k, op) in [(0i64, F64_ABS), (1, F64_FLOOR), (2, F64_CEIL), (6, F64_SQRT)] {
                is(self, k);
                self.get(L_A);
                self.op(op);
                done(self);
                self.op(END);
            }
            // round: x when not finite or 0, 0 / -0 within half of 0,
            // else floor(x + 0.5)
            is(self, 3);
            {
                let f = |cg: &mut Cg<'a>, x: f64| {
                    cg.op(F64_CONST);
                    cg.w.b.extend_from_slice(&x.to_le_bytes());
                };
                // x - x != 0 (inf, nan) or x == 0: x
                self.get(L_A);
                self.get(L_A);
                self.op(F64_SUB);
                f(self, 0.0);
                self.op(F64_NE);
                self.get(L_A);
                f(self, 0.0);
                self.op(F64_EQ);
                self.op(I32_OR);
                self.op(IF);
                self.op(T_VOID);
                self.get(L_A);
                done(self);
                self.op(END);
                self.get(L_A);
                f(self, 0.0);
                self.op(F64_GT);
                self.get(L_A);
                f(self, 0.5);
                self.op(F64_LT);
                self.op(I32_AND);
                self.op(IF);
                self.op(T_VOID);
                f(self, 0.0);
                done(self);
                self.op(END);
                self.get(L_A);
                f(self, 0.0);
                self.op(F64_LT);
                self.get(L_A);
                f(self, -0.5);
                self.op(F64_GE);
                self.op(I32_AND);
                self.op(IF);
                self.op(T_VOID);
                f(self, -0.0);
                done(self);
                self.op(END);
                self.get(L_A);
                f(self, 0.5);
                self.op(F64_ADD);
                self.op(F64_FLOOR);
                done(self);
            }
            self.op(END);
            // min, max of exactly two numbers
            self.get(L_ARGC);
            self.i32c(2);
            self.op(I32_NE);
            self.not_num(L_FB, 3 * VS);
            self.op(I32_OR);
            self.op(BR_IF);
            self.w.u(0);
            for (k, op) in [(31i64, F64_MIN), (32, F64_MAX)] {
                is(self, k);
                self.get(L_A);
                self.get(L_FB);
                self.mem(F64_LOAD, 3, 3 * VS + on);
                self.op(op);
                done(self);
                self.op(END);
            }
            self.op(END);
        }
        // __isRegExp, Array.isArray of one argument and hasOwnProperty of a
        // plain object and a string whose atom is known (builtins.rs,
        // proxy.rs, Vm::has_own): a boolean into dst, done (0)
        if MASK.with(|m| m.get()) & (1u64 << 51) == 0 {
            let to = l.t_obj as i64;
            let done = |cg: &mut Cg<'a>| {
                cg.set(L_TAG);
                cg.i32c(cell);
                cg.get(L_SP);
                cg.mem(I32_STORE, 2, 0);
                cg.get(L_DB);
                cg.get(L_TAG);
                cg.op(I64_EXTEND_I32_U);
                cg.i64c(crate::value::V_BOOL as i64);
                cg.op(I64_OR);
                cg.mem(I64_STORE, 3, 0);
                cg.i32c(0);
                cg.op(RETURN);
            };
            // slot k of the call (0 f, 1 this, 2 the first argument) not an
            // object: br_if out of the innermost block
            let not_obj = |cg: &mut Cg<'a>, k: u32| {
                cg.rtag(L_FB, k * VS);
                cg.i32c(to);
                cg.op(I32_NE);
            };
            self.op(BLOCK);
            self.op(T_VOID);
            // one argument or more: the function's id in L_T3
            self.get(L_ARGC);
            self.i32c(1);
            self.op(I32_LT_S);
            self.op(BR_IF);
            self.w.u(0);
            self.get(L_FO);
            self.mem(I32_LOAD, 2, l.off_func);
            self.set(L_T3);
            for (id, class) in [(crate::proxy::NF_IS_REGEXP, C_REGEXP), (crate::builtins::NF_A_ISARRAY, C_ARRAY)] {
                self.get(L_T3);
                self.i32c(id as i64);
                self.op(I32_EQ);
                self.op(IF);
                self.op(T_VOID);
                not_obj(self, 2);
                self.op(IF);
                self.op(T_VOID);
                self.i32c(0);
                done(self);
                self.op(END);
                self.get(L_FB);
                self.mem(I32_LOAD, 2, 2 * VS + l.off_int);
                self.obj_addr();
                self.mem(I32_LOAD, 2, l.off_class);
                self.i32c(class as i64);
                self.op(I32_EQ);
                done(self);
                self.op(END);
            }
            // Array push of one value onto an extensible array with room in
            // its elements and no others (Vm::push_elem): the new length
            {
                self.op(BLOCK);
                self.op(T_VOID);
                self.get(L_T3);
                self.i32c(crate::builtins::NF_AP_PUSH as i64);
                self.op(I32_NE);
                self.get(L_ARGC);
                self.i32c(1);
                self.op(I32_NE);
                self.op(I32_OR);
                not_obj(self, 1);
                self.op(I32_OR);
                self.op(BR_IF);
                self.w.u(0);
                self.get(L_FB);
                self.mem(I32_LOAD, 2, VS + l.off_int);
                self.obj_addr();
                self.w.byte(LOCAL_TEE);
                self.w.u(L_T2 as u64);
                self.mem(I32_LOAD, 2, l.off_class);
                self.i32c(C_ARRAY as i64);
                self.op(I32_NE);
                // not one with a read-only length (pos 3)
                self.get(L_T2);
                self.mem(I32_LOAD, 2, l.off_pos);
                self.i32c(3);
                self.op(I32_EQ);
                self.op(I32_OR);
                self.get(L_T2);
                self.mem(I32_LOAD8_U, 0, l.off_ext);
                self.op(I32_EQZ);
                self.op(I32_OR);
                self.get(L_T2);
                self.mem(I32_LOAD, 2, l.off_elems);
                self.op(I32_EQZ);
                self.op(I32_OR);
                // colds[cold].elems2 empty
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.vm_colds_ptr);
                self.get(L_T2);
                self.mem(I32_LOAD, 2, l.off_cold);
                self.i32c(l.cold_size as i64);
                self.op(I32_MUL);
                self.op(I32_ADD);
                self.mem(I32_LOAD, 2, l.cold_e2_len);
                self.op(I32_OR);
                self.op(BR_IF);
                self.w.u(0);
                // L_T5 = &evecs[elems], L_T6 = its length < its capacity
                self.get(L_T2);
                self.mem(I32_LOAD, 2, l.off_elems);
                self.i32c(l.elems_size as i64);
                self.op(I32_MUL);
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.vm_evecs_ptr);
                self.op(I32_ADD);
                self.w.byte(LOCAL_TEE);
                self.w.u(L_T5 as u64);
                self.mem(I32_LOAD, 2, l.elems_vl);
                self.w.byte(LOCAL_TEE);
                self.w.u(L_T6 as u64);
                self.get(L_T5);
                self.mem(I32_LOAD, 2, l.elems_vc);
                self.op(I32_GE_U);
                self.op(BR_IF);
                self.w.u(0);
                // the value's reference (Vm::inc): an object's rc, a
                // string handle's (> 0) rc
                self.tag_at(L_FB, 2 * VS);
                self.w.byte(LOCAL_TEE);
                self.w.u(L_TAG as u64);
                self.i32c(to);
                self.op(I32_EQ);
                self.op(IF);
                self.op(T_VOID);
                self.get(L_FB);
                self.mem(I32_LOAD, 2, 2 * VS + l.off_int);
                self.obj_addr();
                self.w.byte(LOCAL_TEE);
                self.w.u(L_T4 as u64);
                self.get(L_T4);
                self.mem(I32_LOAD, 2, l.off_rc);
                self.i32c(1);
                self.op(I32_ADD);
                self.mem(I32_STORE, 2, l.off_rc);
                self.op(END);
                let o_src = std::mem::offset_of!(crate::vm::SEnt, rc) as u32;
                let ssz = std::mem::size_of::<crate::vm::SEnt>() as i64;
                self.get(L_TAG);
                self.i32c(l.t_str as i64);
                self.op(I32_EQ);
                self.get(L_FB);
                self.mem(I32_LOAD, 2, 2 * VS + l.off_int);
                self.i32c(0);
                self.op(I32_GT_S);
                self.op(I32_AND);
                self.op(IF);
                self.op(T_VOID);
                self.get(L_FB);
                self.mem(I32_LOAD, 2, 2 * VS + l.off_int);
                self.i32c(ssz);
                self.op(I32_MUL);
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.vm_srcs_ptr);
                self.op(I32_ADD);
                self.w.byte(LOCAL_TEE);
                self.w.u(L_T4 as u64);
                self.get(L_T4);
                self.mem(I32_LOAD, 2, o_src);
                self.i32c(1);
                self.op(I32_ADD);
                self.mem(I32_STORE, 2, o_src);
                self.op(END);
                // v[len] = the value, len + 1
                self.get(L_T5);
                self.mem(I32_LOAD, 2, l.elems_vp);
                self.get(L_T6);
                self.i32c(VS as i64);
                self.op(I32_MUL);
                self.op(I32_ADD);
                self.w.byte(LOCAL_TEE);
                self.w.u(L_T4 as u64);
                self.get(L_FB);
                self.mem(I64_LOAD, 3, 2 * VS);
                self.mem(I64_STORE, 3, 0);
                self.get(L_T6);
                self.i32c(1);
                self.op(I32_ADD);
                self.set(L_T6);
                self.get(L_T5);
                self.get(L_T6);
                self.mem(I32_STORE, 2, l.elems_vl);
                // the length into dst
                self.i32c(cell);
                self.get(L_SP);
                self.mem(I32_STORE, 2, 0);
                self.get(L_DB);
                self.get(L_T6);
                self.op(F64_CONVERT_I32_S);
                self.mem(F64_STORE, 3, l.off_num);
                self.i32c(0);
                self.op(RETURN);
                self.op(END);
            }
            // hasOwnProperty: this a C_OBJECT, the key a string with its atom
            self.get(L_T3);
            self.i32c(crate::builtins::NF_OP_HASOWN as i64);
            self.op(I32_NE);
            not_obj(self, 1);
            self.op(I32_OR);
            self.rtag(L_FB, 2 * VS);
            self.i32c(l.t_str as i64);
            self.op(I32_NE);
            self.op(I32_OR);
            self.op(BR_IF);
            self.w.u(0);
            self.get(L_FB);
            self.mem(I32_LOAD, 2, VS + l.off_int);
            self.obj_addr();
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T2 as u64);
            self.mem(I32_LOAD, 2, l.off_class);
            self.i32c(C_OBJECT as i64);
            self.op(I32_NE);
            self.op(BR_IF);
            self.w.u(0);
            // L_T4 = the atom (SEnt.atom, -1 not known: the slow path)
            let sz = std::mem::size_of::<crate::vm::SEnt>() as i64;
            let o_atom = std::mem::offset_of!(crate::vm::SEnt, atom) as u32;
            self.get(L_FB);
            self.mem(I32_LOAD, 2, 2 * VS + l.off_int);
            self.i32c(sz);
            self.op(I32_MUL);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_srcs_ptr);
            self.op(I32_ADD);
            self.mem(I32_LOAD, 2, o_atom);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T4 as u64);
            self.i32c(0);
            self.op(I32_LT_S);
            self.op(BR_IF);
            self.w.u(0);
            // L_C = the shape's nkeys, L_T5 = the holder's keys (Vm::find)
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_shapes_ptr);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_shape);
            self.i32c(l.shape_size as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T5 as u64);
            self.mem(I32_LOAD, 2, l.sh_nkeys);
            self.set(L_C);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_shapes_ptr);
            self.get(L_T5);
            self.mem(I32_LOAD, 2, l.sh_kref);
            self.i32c(l.shape_size as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.mem(I32_LOAD, 2, l.sh_keys_p);
            self.set(L_T5);
            self.i32c(0);
            self.set(L_T6);
            self.op(LOOP);
            self.op(T_VOID);
            self.get(L_T6);
            self.get(L_C);
            self.op(I32_GE_S);
            self.op(IF);
            self.op(T_VOID);
            self.i32c(0);
            done(self);
            self.op(END);
            // keys[i] (an int, its low word) == the atom
            self.get(L_T5);
            self.get(L_T6);
            self.i32c(8);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.mem(I32_LOAD, 2, 0);
            self.get(L_T4);
            self.op(I32_EQ);
            self.op(IF);
            self.op(T_VOID);
            self.i32c(1);
            done(self);
            self.op(END);
            self.get(L_T6);
            self.i32c(1);
            self.op(I32_ADD);
            self.set(L_T6);
            self.op(BR);
            self.w.u(0);
            self.op(END);
            self.op(END);
        }
        self.extra -= 1;
        self.op(END); // $nat
        {
            // f: a script function whose proto has compiled code it can be
            // called with (Vm.jit_calls; L_CA its words)
            self.get(L_FO);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T2 as u64);
            self.mem(I32_LOAD, 2, l.off_class);
            self.i32c(C_FUNCTION as i64);
            self.op(I32_NE);
            self.fail_if(0);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_func);
            self.i32c(8);
            self.op(I32_MUL);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_CA as u64);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_jit_calls_len);
            self.op(I32_GE_U);
            self.fail_if(0);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_jit_calls_ptr);
            self.get(L_CA);
            self.i32c(4);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.set(L_CA);
            // a sloppy callee's this: undefined or null is the global object
            // (sloppy_this), written into the this slot of the call
            {
                let ob = bytes_of(Val::Obj(0));
                let lo = i64::from_le_bytes([ob[0], ob[1], ob[2], ob[3], ob[4], ob[5], ob[6], ob[7]]);
                ld(self, 6);
                self.op(I32_EQZ);
                self.tag_at(L_FB, VS);
                self.w.byte(LOCAL_TEE);
                self.w.u(L_T3 as u64);
                self.i32c(l.t_undef as i64);
                self.op(I32_EQ);
                self.get(L_T3);
                self.i32c(l.t_null as i64);
                self.op(I32_EQ);
                self.op(I32_OR);
                self.op(I32_AND);
                self.op(IF);
                self.op(T_VOID);
                self.get(L_FB);
                self.w.byte(I64_CONST);
                self.w.s(lo);
                self.mem(I64_STORE, 3, VS);
                self.get(L_FB);
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.vm_global);
                self.mem(I32_STORE, 2, VS + l.off_int);
                self.op(END);
            }
            ld(self, 1);
            self.i32c(0);
            self.op(I32_LT_S);
            self.get(L_VM);
            self.mem(I32_LOAD8_U, 0, l.vm_gc_flag);
            self.op(I32_OR);
            self.get(L_VM);
            self.mem(I32_LOAD8_U, 0, l.vm_slicing);
            self.op(I32_OR);
            self.i32c(cell);
            self.mem(I32_LOAD, 2, 36);
            self.i32c(DEPTH_MAX as i64);
            self.op(I32_GE_S);
            self.op(I32_OR);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_len);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_cap);
            self.op(I32_GE_U);
            self.op(I32_OR);
            // the stack's room for the callee's frame (filled below)
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_stack_cap);
            self.get(L_BP);
            f2(self);
            self.op(I32_ADD);
            ld(self, 4);
            self.op(I32_ADD);
            self.op(I32_LT_U);
            self.op(I32_OR);
            ld(self, 6);
            self.op(I32_EQZ);
            self.rtag(L_FB, VS);
            self.i32c(l.t_obj as i64);
            self.op(I32_NE);
            self.op(I32_AND);
            self.op(I32_OR);
            self.fail_if(0);
            // an arrow with a new.target (renter: cold elems2[0])
            ld(self, 7);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_colds_ptr);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_cold);
            self.i32c(l.cold_size as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.mem(I32_LOAD, 2, l.cold_e2_len);
            self.fail_if(0);
            self.extra -= 1;
            self.op(END);
            // a stack shorter than the callee's frame: undefined up to
            // its end (L_T3 from, L_T4 to), as grow_stack
            self.get(L_BP);
            f2(self);
            self.op(I32_ADD);
            ld(self, 4);
            self.op(I32_ADD);
            self.set(L_T4);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_stack_len);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T3 as u64);
            self.get(L_T4);
            self.op(I32_LT_U);
            self.op(IF);
            self.op(T_VOID);
            {
                let u = bytes_of(Val::Undef());
                let lo = i64::from_le_bytes([u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]]);
                // L_T5 = &stack[len], L_T6 = &stack[need]
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.vm_stack_ptr);
                self.set(L_T6);
                self.get(L_T6);
                self.get(L_T3);
                self.i32c(VS as i64);
                self.op(I32_MUL);
                self.op(I32_ADD);
                self.set(L_T5);
                self.get(L_T6);
                self.get(L_T4);
                self.i32c(VS as i64);
                self.op(I32_MUL);
                self.op(I32_ADD);
                self.set(L_T6);
                self.op(BLOCK);
                self.op(T_VOID);
                self.op(LOOP);
                self.op(T_VOID);
                self.get(L_T5);
                self.get(L_T6);
                self.op(I32_GE_U);
                self.op(BR_IF);
                self.w.u(1);
                self.get(L_T5);
                self.w.byte(I64_CONST);
                self.w.s(lo);
                self.mem(I64_STORE, 3, 0);
                self.get(L_T5);
                self.i32c(VS as i64);
                self.op(I32_ADD);
                self.set(L_T5);
                self.op(BR);
                self.w.u(0);
                self.op(END);
                self.op(END);
                self.get(L_VM);
                self.get(L_T4);
                self.mem(I32_STORE, 2, l.vm_stack_len);
            }
            self.op(END);
            // the caller goes on after the call (L_T5 its frame)
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_ptr);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_len);
            self.i32c(1);
            self.op(I32_SUB);
            self.i32c(l.frame_size as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_T5 as u64);
            self.get(L_NEXT);
            self.op(I64_EXTEND_I32_S);
            self.mem(I64_STORE, 3, l.fr_pc);
            // locals past the arguments: undefined (L_C the callee's sp)
            self.get(L_FB);
            self.i32c(2 * VS as i64);
            self.op(I32_ADD);
            self.w.byte(LOCAL_TEE);
            self.w.u(L_C as u64);
            ld(self, 2);
            self.get(L_ARGC);
            ld(self, 2);
            self.get(L_ARGC);
            self.op(I32_LT_S);
            self.op(SELECT);
            self.i32c(VS as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.set(L_T3);
            self.get(L_C);
            ld(self, 3);
            self.i32c(VS as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.set(L_T4);
            let u = bytes_of(Val::Undef());
            let lo = i64::from_le_bytes([u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]]);
            self.op(BLOCK);
            self.op(T_VOID);
            self.op(LOOP);
            self.op(T_VOID);
            self.get(L_T3);
            self.get(L_T4);
            self.op(I32_GE_U);
            self.op(BR_IF);
            self.w.u(1);
            self.get(L_T3);
            self.w.byte(I64_CONST);
            self.w.s(lo);
            self.mem(I64_STORE, 3, 0);
            self.get(L_T3);
            self.i32c(VS as i64);
            self.op(I32_ADD);
            self.set(L_T3);
            self.op(BR);
            self.w.u(0);
            self.op(END);
            self.op(END);
            // the frame (L_T3), after the caller's
            self.get(L_T5);
            self.i32c(l.frame_size as i64);
            self.op(I32_ADD);
            self.set(L_T3);
            let st_i32 = |cg: &mut Cg<'a>, off: u32, v: &dyn Fn(&mut Cg<'a>)| {
                cg.get(L_T3);
                v(cg);
                cg.op(I64_EXTEND_I32_S);
                cg.mem(I64_STORE, 3, off);
            };
            let st_k = |cg: &mut Cg<'a>, off: u32, k: i64| {
                cg.get(L_T3);
                cg.w.byte(I64_CONST);
                cg.w.s(k);
                cg.mem(I64_STORE, 3, off);
            };
            st_i32(self, l.fr_proto, &|cg| ld(cg, 0));
            st_i32(self, l.fr_fobj, &|cg| {
                cg.get(L_FB);
                cg.mem(I32_LOAD, 2, l.off_int);
            });
            st_i32(self, l.fr_pc, &|cg| ld(cg, 5));
            st_i32(self, l.fr_bp, &|cg| {
                cg.get(L_BP);
                f2(cg);
                cg.op(I32_ADD);
            });
            st_i32(self, l.frame_env, &|cg| {
                cg.get(L_T2);
                cg.mem(I32_LOAD, 2, l.off_env);
            });
            // this: the call's, an arrow's from its cold part (renter;
            // L_T4 where from)
            ld(self, 7);
            self.op(IF);
            self.w.byte(T_I32);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_colds_ptr);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, l.off_cold);
            self.i32c(l.cold_size as i64);
            self.op(I32_MUL);
            self.op(I32_ADD);
            self.i32c(l.cold_prim as i64);
            self.op(I32_ADD);
            self.op(ELSE);
            self.get(L_FB);
            self.i32c(VS as i64);
            self.op(I32_ADD);
            self.op(END);
            self.set(L_T4);
            self.get(L_T3);
            self.get(L_T4);
            self.mem(I64_LOAD, 3, 0);
            self.mem(I64_STORE, 3, l.frame_this);
            st_k(self, l.fr_new_target, lo);
            st_k(self, l.fr_args_obj, -1);
            self.get(L_T3);
            self.i32c(0);
            self.mem(I32_STORE8, 0, l.fr_construct);
            st_k(self, l.fr_gen, -1);
            st_i32(self, l.fr_ret, &|cg| {
                cg.get(L_BP);
                cg.get(L_DST);
                cg.op(I32_ADD);
            });
            st_k(self, l.fr_mv_from, -1);
            st_k(self, l.fr_mv_to, -1);
            self.get(L_VM);
            self.get(L_VM);
            self.mem(I32_LOAD, 2, l.vm_frames_len);
            self.i32c(1);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, l.vm_frames_len);
            self.i32c(cell);
            self.get(L_C);
            self.mem(I32_STORE, 2, 0);
            self.i32c(cell);
            self.get(L_T3);
            self.i32c(l.frame_this as i64);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, 20);
            self.i32c(cell);
            ld(self, 5);
            self.mem(I32_STORE, 2, 8);
            self.i32c(cell);
            ld(self, 1);
            self.mem(I32_STORE, 2, 28);
            self.i32c(cell);
            self.get(L_BP);
            f2(self);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, 32);
            self.i32c(cell);
            self.get(L_C);
            self.mem(I32_STORE, 2, 40);
            self.i32c(cell);
            self.i32c(cell);
            self.mem(I32_LOAD, 2, 36);
            self.i32c(1);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, 36);
        }
        self.slow_at = None;
        self.i32c(6);
        self.set(L_TAG);
        self.op(BR);
        self.w.u(1);
        self.extra -= 1;
        self.op(END);
        if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
            let at = std::ptr::addr_of!(OPCOUNT) as usize as i64 + 840;
            self.i32c(at);
            self.i32c(at);
            self.mem(I32_LOAD, 2, 0);
            self.i32c(1);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, 0);
        }
        // H_CALL(vm, bp, pi, pc, ri)
        self.get(L_VM);
        self.get(L_BP);
        self.get(L_CPI);
        self.get(L_PC);
        self.get(L_RI);
        self.op(CALL);
        self.w.u(H_CALL as u64);
        self.set(L_TAG);
        self.reload();
        self.extra -= 1;
        self.op(END);
        // a compiled callee's frame: call its code
        self.get(L_TAG);
        self.i32c(6);
        self.op(I32_EQ);
        self.op(IF);
        self.op(T_VOID);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_handlers_len);
        self.set(L_T3);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 40);
        self.get(L_KV);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 8);
        self.get(L_BUDGET);
        self.get(L_VM);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 32);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 28);
        self.op(CALL_INDIRECT);
        self.w.u(0);
        self.w.u(0);
        self.set(L_T2);
        // a return op, no handler left, the stack as long as this frame,
        // no collection due: the frame popped here
        self.get(L_T2);
        self.i32c(OUT_RET as i64);
        self.op(I32_EQ);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_handlers_len);
        self.get(L_T3);
        self.op(I32_EQ);
        self.op(I32_AND);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_stack_len);
        self.get(L_BP);
        self.get(L_CFS);
        self.op(I32_ADD);
        self.op(I32_GE_U);
        self.op(I32_AND);
        self.get(L_VM);
        self.mem(I32_LOAD8_U, 0, l.vm_gc_flag);
        self.op(I32_EQZ);
        self.op(I32_AND);
        self.op(IF);
        self.op(T_VOID);
        // frames.len -= 1 (L_T4 the new length)
        self.get(L_VM);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_len);
        self.i32c(1);
        self.op(I32_SUB);
        self.w.byte(LOCAL_TEE);
        self.w.u(L_T4 as u64);
        self.mem(I32_STORE, 2, l.vm_frames_len);
        self.i32c(cell);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 36);
        self.i32c(1);
        self.op(I32_SUB);
        self.mem(I32_STORE, 2, 36);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_stack_ptr);
        self.get(L_BP);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_SP);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_kvals_ptr);
        self.set(L_KV);
        self.i32c(cell);
        self.get(L_VM);
        self.mem(I32_LOAD, 2, l.vm_frames_ptr);
        self.get(L_T4);
        self.i32c(1);
        self.op(I32_SUB);
        self.i32c(l.frame_size as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.i32c(l.frame_this as i64);
        self.op(I32_ADD);
        self.mem(I32_STORE, 2, 20);
        // the cell's sp is this frame's again (the caller reloads it)
        self.i32c(cell);
        self.get(L_SP);
        self.mem(I32_STORE, 2, 0);
        self.i32c(cell);
        self.get(L_KV);
        self.mem(I32_STORE, 2, 4);
        // the value into dst (db is from the sp before the call)
        self.get(L_SP);
        self.get(L_DST);
        self.i32c(VS as i64);
        self.op(I32_MUL);
        self.op(I32_ADD);
        self.set(L_DB);
        self.get(L_DB);
        self.i32c(cell);
        self.mem(I64_LOAD, 3, 48);
        self.mem(I64_STORE, 3, 0);
        self.i32c(0);
        self.set(L_TAG);
        self.op(ELSE);
        self.get(L_VM);
        self.get(L_BP);
        self.get(L_T2);
        self.get(L_PC);
        self.get(L_RI);
        self.op(CALL);
        self.w.u(H_RET as u64);
        self.set(L_TAG);
        self.op(END);
        self.op(END);
        if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
            // debug: OPCOUNT[200 + the code callx returns]
            let at = std::ptr::addr_of!(OPCOUNT) as usize as i64 + 800;
            self.get(L_TAG);
            self.i32c(4);
            self.op(I32_MUL);
            self.i32c(at);
            self.op(I32_ADD);
            self.set(L_T2);
            self.get(L_T2);
            self.get(L_T2);
            self.mem(I32_LOAD, 2, 0);
            self.i32c(1);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, 0);
        }
        self.get(L_TAG);
        self.op(END);
        let out = std::mem::replace(&mut self.w, saved_w).b;
        self.extra = se;
        self.exit_d = sx;
        self.slow_at = ss;
        out
    }
    fn reload(&mut self) {
        let cell = cell_addr() as i64;
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 0);
        self.set(L_SP);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 4);
        self.set(L_KV);
    }
    fn helper_raw(&mut self, h: u32, pc: i64) {
        if self.inlining {
            self.inl_bad = true;
        }
        self.get(L_VM);
        self.get(L_BP);
        let (pi, rb) = (self.pi as i64, self.rb as i64);
        self.i32c(pi);
        self.i32c(pc);
        self.i32c(rb + pc);
        self.op(CALL);
        self.w.u(h as u64);
        self.set(L_TAG);
        if h != H_CALL && h != H_RET && !goes_on(self.cur_kind as int) {
            // a helper that moved the stack or the constants leaves
            // (export_helper's code 2)
            return;
        }
        // the stack and the constants may have moved (a native that ran
        // JavaScript, a function compiled): their addresses again
        let cell = cell_addr() as i64;
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 0);
        self.set(L_SP);
        self.i32c(cell);
        self.mem(I32_LOAD, 2, 4);
        self.set(L_KV);
    }
    fn helper_codes(&mut self, pc: i64, next: i64) {
        // any code but 0 leaves: codes(tag, pc, next) is the pc to return
        // (1 pc, 2 next, 3 cell[2], 4 OUT_THROW, else OUT_FRAMES)
        self.get(L_TAG);
        self.op(IF);
        self.op(T_VOID);
        self.extra += 1;
        if MASK.with(|m| m.get()) & (1u64 << 54) != 0 && self.cur_kind < 60 {
            // debug: OPCOUNT[100 + kind] for a frames exit (5)
            let at = std::ptr::addr_of!(OPCOUNT) as usize as i64 + (100 + self.cur_kind as i64) * 4;
            self.get(L_TAG);
            self.i32c(5);
            self.op(I32_EQ);
            self.op(IF);
            self.op(T_VOID);
            self.i32c(at);
            self.i32c(at);
            self.mem(I32_LOAD, 2, 0);
            self.i32c(1);
            self.op(I32_ADD);
            self.mem(I32_STORE, 2, 0);
            self.op(END);
        }
        self.get(L_TAG);
        self.i32c(pc);
        self.i32c(next);
        self.op(CALL);
        self.w.u(F_CODES as u64);
        self.set(L_OUT);
        let d = self.exit_depth();
        self.op(BR);
        self.w.u(d as u64);
        self.extra -= 1;
        self.op(END);
    }
    /// x % y for whole non-negative int32s (the interpreter's fast path)
    fn modulo(&mut self, dst: i32, x: i32, y: i32, pc: i64) -> bool {
        if dst < 0 {
            return false;
        }
        let a = self.opnd(x);
        let b = self.opnd(y);
        self.need_num(a, pc);
        self.need_num(b, pc);
        self.num(a);
        self.set(L_A);
        self.num(b);
        self.set(L_B);
        // !(a >= 1 && b > 0 && a <= 2^31-1 && b <= 2^31-1 && whole): a
        // zero keeps its sign the slow way (-0 % 5 is -0)
        self.get(L_A);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&1.0f64.to_le_bytes());
        self.op(F64_GE);
        self.get(L_B);
        self.op(F64_CONST);
        self.w.b.extend_from_slice(&0.0f64.to_le_bytes());
        self.op(F64_GT);
        self.op(I32_AND);
        for l in [L_A, L_B] {
            self.get(l);
            self.op(F64_CONST);
            self.w.b.extend_from_slice(&2147483647.0f64.to_le_bytes());
            self.op(F64_LE);
            self.op(I32_AND);
            self.get(l);
            self.op(F64_TRUNC);
            self.get(l);
            self.op(F64_EQ);
            self.op(I32_AND);
        }
        self.op(I32_EQZ);
        self.fail_if(pc);
        self.get(L_A);
        self.op(I32_TRUNC_F64_S);
        self.get(L_B);
        self.op(I32_TRUNC_F64_S);
        self.op(I32_REM_S);
        self.op(F64_CONVERT_I32_S);
        self.set(L_A);
        self.store_num(dst as u32, L_A);
        true
    }

    /// One op; false when it is written as an exit.
    fn one(&mut self, pc: usize, ins: Op) -> bool {
        let k = (ins.code & 63) as u32;
        if MASK.with(|m| m.get()) & (1u64 << k) != 0 && (ins.code & 255) < 64 {
            return false;
        }
        if self.inlining {
            return self.one_op(pc, ins);
        }
        let kind = (ins.code & 255) as int;
        let sub = ((ins.code >> 8) & 255) as int;
        if pc < self.spec.len() && self.spec[pc] && ins.a >= 0 {
            let code = match kind {
                R_ADD => OP_ADD,
                R_SUB => OP_SUB,
                R_MUL => OP_MUL,
                R_ARITH if sub == OP_SUB || sub == OP_MUL || sub == OP_DIV => sub,
                _ => -1,
            };
            if code >= 0 {
                return self.spec_arith(code, ins.a, ins.b, ins.c, pc as i64);
            }
        }
        let wrote = self.one_op(pc, ins);
        match kn_fx(ins, false) {
            KnFx::Same | KnFx::Spec(_, _, _) => {}
            KnFx::Kill(a, b) => {
                for r in [a, b] {
                    if r >= 0 {
                        self.kn_forget(r as u32);
                    }
                }
            }
            KnFx::All => self.kn_clear(),
        }
        wrote
    }

    /// A block's start: the registers of `set` (kn_plan's bits) are known.
    /// Entered from outside (L_FRESH), their tags are checked (a number
    /// else back to the interpreter at pc) and their numbers loaded.
    fn kn_block(&mut self, pc: usize, set: u32) {
        self.kn_clear();
        if set != 0 {
            let mut regs: Vec<u32> = Vec::new();
            for r in 0..self.kn_fix.len() {
                let f = self.kn_fix[r];
                if f >= 0 && set & (1u32 << (f as u32 - KN0)) != 0 {
                    regs.push(r as u32);
                }
            }
            self.get(L_FRESH);
            self.op(IF);
            self.op(T_VOID);
            self.extra += 1;
            for r in regs.iter() {
                let o = Opnd::Reg(*r);
                self.not_num_o(o);
                self.exit_if(pc as i64);
                self.num(o);
                let f = self.kn_fix[*r as usize] as u32;
                self.set(f);
            }
            self.extra -= 1;
            self.op(END);
            for r in regs.iter() {
                self.kn[*r as usize] = self.kn_fix[*r as usize];
                self.kn_regs.push(*r);
            }
        }
        self.i32c(0);
        self.set(L_FRESH);
    }

    /// nothing known about any register (Cg.kn)
    fn kn_clear(&mut self) {
        for r in self.kn_regs.iter() {
            self.kn[*r as usize] = -1;
        }
        self.kn_regs.clear();
    }
    fn kn_forget(&mut self, r: u32) {
        if (r as usize) < self.kn.len() && self.kn[r as usize] >= 0 {
            self.kn[r as usize] = -1;
            self.kn_regs.retain(|x| *x != r);
        }
    }
    /// a local for register r's number from now on (the oldest one is
    /// taken back when all are in use)
    fn kn_take(&mut self, r: u32) -> u32 {
        self.kn_forget(r);
        if !self.kn_fix.is_empty() {
            // fixed locals (kn_plan): a register without one is not kept
            let f = if (r as usize) < self.kn_fix.len() { self.kn_fix[r as usize] } else { -1 };
            if f < 0 {
                return L_A;
            }
            self.kn[r as usize] = f;
            self.kn_regs.push(r);
            return f as u32;
        }
        let l = KN0 + self.kn_next % KN_N;
        self.kn_next += 1;
        if let Some(k) = self.kn_regs.iter().position(|x| self.kn[*x as usize] == l as i32) {
            let old = self.kn_regs.remove(k);
            self.kn[old as usize] = -1;
        }
        if (r as usize) < self.kn.len() {
            self.kn[r as usize] = l as i32;
            self.kn_regs.push(r);
        }
        l
    }
    /// an operand's number on the wasm stack, its tag checked unless it is
    /// known (a failed check returns pc); a register read from memory is
    /// known after this
    fn spec_num(&mut self, o: Opnd, pc: i64) {
        match o {
            Opnd::Num(n) => {
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&n.to_le_bytes());
            }
            Opnd::K(_) => {
                self.exit_at(pc);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&0f64.to_le_bytes());
            }
            Opnd::Reg(r) => {
                if (r as usize) < self.kn.len() && self.kn[r as usize] >= 0 {
                    let l = self.kn[r as usize] as u32;
                    self.get(l);
                    return;
                }
                self.not_num_o(o);
                self.exit_if(pc);
                self.num(o);
                if (r as usize) < self.kn.len() {
                    let l = self.kn_take(r);
                    self.w.byte(LOCAL_TEE);
                    self.w.u(l as u64);
                }
            }
        }
    }
    /// A speculated + - * /: numbers or back to the interpreter at pc; the
    /// result is written to its register and kept known.
    fn spec_arith(&mut self, code: int, dst: i32, x: i32, y: i32, pc: i64) -> bool {
        let fop = match code {
            OP_ADD => F64_ADD,
            OP_SUB => F64_SUB,
            OP_MUL => F64_MUL,
            _ => F64_DIV,
        };
        let a = self.opnd(x);
        let b = self.opnd(y);
        self.spec_num(a, pc);
        self.spec_num(b, pc);
        self.op(fop);
        // a register known to hold a number has its tag in memory already
        let was = (dst as usize) < self.kn.len() && self.kn[dst as usize] >= 0;
        let l = self.kn_take(dst as u32);
        self.set(l);
        if was {
            self.get(L_SP);
            self.get(l);
            let on = self.lay.off_num;
            self.mem(F64_STORE, 3, dst as u32 * VS + on);
        } else {
            self.store_num(dst as u32, l);
        }
        true
    }

    /// One op as it is written without speculation.
    fn one_op(&mut self, pc: usize, ins: Op) -> bool {
        let w0 = ins.code;
        let (w1, w2, w3) = (ins.a, ins.b, ins.c);
        let kind = (w0 & 255) as int;
        let sub = ((w0 >> 8) & 255) as int;
        let p = pc as i64;
        let next = p + 1 + ((w0 >> 16) as i64);
        let cmp_ok = |c: int| c >= OP_EQ && c <= OP_GE;
        match kind {
            R_ADD | R_SUB | R_MUL | R_ARITH | R_CMP | R_INC_LOCAL | R_POSTINC | R_INC | R_TONUM | R_ADD_LOCAL_POP => {
                let can = match kind {
                    R_ARITH => w1 >= 0 && matches!(sub, OP_SUB | OP_MUL | OP_DIV | OP_MOD | OP_BAND | OP_BOR | OP_BXOR | OP_SHL | OP_SHR),
                    R_CMP => w1 >= 0 && cmp_ok(sub),
                    R_POSTINC => w1 >= 0 && w2 >= 0,
                    _ => w1 >= 0,
                };
                if can {
                    self.with_slow(p, next, false, &|cg: &mut Cg<'a>| cg.fast_op(pc, ins))
                } else {
                    self.helper(H_SLOW, p, next);
                    true
                }
            }
            R_THIS if w1 >= 0 => {
                if let Some((t, strict)) = self.inl_this {
                    // an inlined callee's this: the call's slot after f; a
                    // sloppy one's must be an object already
                    self.get(L_SSP);
                    self.i32c(t as i64);
                    self.op(I32_ADD);
                    self.set(L_T2);
                    if !strict {
                        self.rtag(L_T2, 0);
                        self.i32c(self.lay.t_obj as i64);
                        self.op(I32_NE);
                        self.fail_if(p);
                    }
                } else {
                    // the frame's this, through cell[5]
                    let cell = cell_addr() as i64;
                    self.i32c(cell);
                    self.mem(I32_LOAD, 2, 20);
                    self.set(L_T2);
                }
                self.get(L_T2);
                self.mem(I64_LOAD, 3, 0);
                self.set(L_Q0);
                self.store_val(w1 as u32, L_Q0);
                true
            }
            R_ITER_NEXT if w1 >= 0 && w3 >= 0 && w2 >= 0 => {
                // a for-in step here (for_in_step); else the helper steps
                // the iterator, 7 when it is done
                let fast = self.lay.obj_ok && MASK.with(|m| m.get()) & (1u64 << 52) == 0;
                if fast {
                    self.op(BLOCK);
                    self.op(T_VOID);
                    self.extra += 1;
                    self.op(BLOCK);
                    self.op(T_VOID);
                    self.extra += 1;
                    let saved = self.slow_at;
                    self.slow_at = Some(self.extra);
                    self.for_in_step(w1 as u32, w3 as u32, p);
                    self.slow_at = saved;
                    self.op(BR);
                    self.w.u(1);
                    self.extra -= 1;
                    self.op(END);
                }
                self.helper_raw(H_OP, p);
                self.get(L_TAG);
                self.i32c(7);
                self.op(I32_EQ);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w2 as i64, (w2 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                self.helper_codes(p, next);
                if fast {
                    self.extra -= 1;
                    self.op(END);
                }
                true
            }
            R_TYPEOF if self.lay.obj_ok && w1 >= 0 && self.typeof_ok() => {
                // the tag is type_index for all but objects (vm.rs)
                let l = self.lay;
                let o = self.raw(w2);
                let (b, off) = self.place(o);
                self.tag_at(b, off);
                self.set(L_T2);
                self.get(L_T2);
                self.i32c(l.t_obj as i64);
                self.op(I32_EQ);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.get(b);
                self.mem(I32_LOAD, 2, off + l.off_int);
                self.obj_addr();
                self.mem(I32_LOAD, 2, l.off_class);
                self.set(L_T3);
                // a proxy, symbol or bigint: the helper
                for c in [crate::value::C_PROXY, crate::value::C_SYMBOL, crate::value::C_BIGINT] {
                    self.get(L_T3);
                    self.i32c(c as i64);
                    self.op(I32_EQ);
                }
                self.op(I32_OR);
                self.op(I32_OR);
                self.exit_if(p);
                self.i32c(5);
                self.i32c(1);
                for c in [crate::value::C_FUNCTION, crate::value::C_NATIVE, crate::value::C_BOUND] {
                    self.get(L_T3);
                    self.i32c(c as i64);
                    self.op(I32_EQ);
                }
                self.op(I32_OR);
                self.op(I32_OR);
                self.op(SELECT);
                self.set(L_T2);
                self.extra -= 1;
                self.op(END);
                // number .. string (0xFFF8 ..= 0xFFFC): its index from
                // TYPE_NIB's nibbles
                self.get(L_T2);
                self.i32c(0xFFF8);
                self.op(I32_SUB);
                self.set(L_T3);
                self.i32c(TYPE_NIB);
                self.get(L_T3);
                self.i32c(4);
                self.op(I32_MUL);
                self.op(I32_SHR_U);
                self.i32c(15);
                self.op(I32_AND);
                self.get(L_T2);
                self.get(L_T3);
                self.i32c(5);
                self.op(I32_LT_U);
                self.op(SELECT);
                self.set(L_T2);
                // a small BigInt: 7 (its tag is not its type_index)
                self.i32c(7);
                self.get(L_T2);
                self.get(L_T2);
                self.i32c(l.t_big as i64);
                self.op(I32_EQ);
                self.op(SELECT);
                self.set(L_T2);
                self.get(L_VM);
                self.mem(I32_LOAD, 2, l.vm_type_vals_ptr);
                self.get(L_T2);
                self.i32c(VS as i64);
                self.op(I32_MUL);
                self.op(I32_ADD);
                self.set(L_T2);
                self.get(L_T2);
                self.mem(I64_LOAD, 3, 0);
                self.set(L_Q0);
                self.store_val(w1 as u32, L_Q0);
                true
            }
            R_CHECK if self.lay.obj_ok && w1 >= 0 => {
                // the TDZ marker (sub 0) or undefined / null: the
                // interpreter throws
                let l = self.lay;
                let o = Opnd::Reg(w1 as u32);
                let (b, off) = self.place(o);
                if sub == 0 {
                    self.rtag(b, off);
                    self.i32c(l.t_obj as i64);
                    self.op(I32_EQ);
                    self.get(b);
                    self.mem(I32_LOAD, 2, off + l.off_int);
                    self.get(L_VM);
                    self.mem(I32_LOAD, 2, l.vm_tdz);
                    self.op(I32_EQ);
                    self.op(I32_AND);
                } else {
                    self.rtag(b, off);
                    self.i32c(l.t_undef as i64);
                    self.op(I32_EQ);
                    self.rtag(b, off);
                    self.i32c(l.t_null as i64);
                    self.op(I32_EQ);
                    self.op(I32_OR);
                }
                self.exit_if(p);
                true
            }
            R_GET_ENV if self.lay.obj_ok && w1 >= 0 && w2 >= 0 && w3 >= 0 && sub <= 1 => {
                self.env_slot_addr(w2, w3);
                self.get(L_T2);
                self.mem(I64_LOAD, 3, 0);
                self.set(L_Q0);
                self.store_val(w1 as u32, L_Q0);
                if sub == 1 {
                    // undefined in dst + 1: the bytes of Val::Undef()
                    let u = bytes_of(Val::Undef());
                    let lo = i64::from_le_bytes([u[0], u[1], u[2], u[3], u[4], u[5], u[6], u[7]]);
                    self.get(L_SP);
                    self.w.byte(I64_CONST);
                    self.w.s(lo);
                    self.mem(I64_STORE, 3, (w1 as u32 + 1) * VS);
                }
                true
            }
            R_SET_ENV if self.lay.obj_ok && w2 >= 0 && w3 >= 0 => {
                // the value (a register or constant) into the slot,
                // counted (heap_store), else the helper
                let (b, off) = self.place(self.raw(w1));
                self.with_helper(H_OP, p, next, false, &|cg: &mut Cg<'a>| {
                    cg.env_slot_addr(w2, w3);
                    cg.heap_store(b, off, p);
                    true
                })
            }
            R_ADD_ENV if self.lay.obj_ok && w1 >= 0 && w3 >= 0 => {
                // x += v on a scope slot, both numbers (rvm.rs R_ADD_ENV):
                // the slot's old value must be an uncounted number or
                // undefined, else the helper
                self.with_helper(H_OP, p, next, false, &|cg: &mut Cg<'a>| {
                    let l = cg.lay;
                    let a = cg.opnd(w1);
                    let b = cg.opnd(w2);
                    cg.need_num(a, p);
                    cg.need_num(b, p);
                    cg.num(a);
                    cg.num(b);
                    cg.op(F64_ADD);
                    cg.set(L_A);
                    cg.env_slot_addr(sub as i32, w3);
                    cg.tag_at(L_T2, 0);
                    cg.set(L_TAG);
                    cg.get(L_TAG);
                    cg.i32c(l.t_num as i64);
                    cg.op(I32_NE);
                    cg.get(L_TAG);
                    cg.i32c(l.t_undef as i64);
                    cg.op(I32_NE);
                    cg.op(I32_AND);
                    cg.fail_if(p);
                    cg.get(L_T2);
                    cg.get(L_A);
                    cg.mem(F64_STORE, 3, l.off_num);
                    true
                })
            }
            R_GET_GLOBAL if self.lay.obj_ok && w1 >= 0 && self.caches[pc] != 0 => {
                let a = self.caches[pc];
                self.with_helper(H_OP, p, next, false, &|cg: &mut Cg<'a>| {
                    cg.i32c(a as i64 + CACHE_N as i64 * 4);
                    cg.set(L_T4);
                    cg.pget(w1 as u32, L_T4, 0, a, 0, p)
                })
            }
            R_GET_THIS_PROP if self.lay.obj_ok && w1 >= 0 && self.caches[pc] != 0 => {
                let a = self.caches[pc];
                self.with_helper(H_GET_PROP, p, next, false, &|cg: &mut Cg<'a>| {
                    if let Some((t, _)) = cg.inl_this {
                        // an inlined callee's this (not an object: the call)
                        cg.get(L_SSP);
                        cg.i32c(t as i64);
                        cg.op(I32_ADD);
                    } else {
                        let cell = cell_addr() as i64;
                        cg.i32c(cell);
                        cg.mem(I32_LOAD, 2, 20);
                    }
                    cg.set(L_T4);
                    cg.pget_ic(w1 as u32, L_T4, 0, a, w3, p)
                })
            }
            R_SET_PROP if self.lay.obj_ok && w1 >= 0 && sub == 0 && self.caches[pc] != 0 => {
                let raw_ok = match self.raw(w2) {
                    Opnd::K(k) => !matches!((self.kvals.get(k as usize)).map(|v| v.kind()), Some(VK::Str(_)) | Some(VK::Obj(_)) | None),
                    _ => true,
                };
                if raw_ok {
                    let a = self.caches[pc];
                    // the transition cached when this was compiled, for an add
                    let t = if w3 <= -2 && ((-2 - w3) & 65535) != 0 && self.atoms[pc] >= 0 { -2 - w3 } else { -1 };
                    let atom = self.atoms[pc];
                    self.with_helper(H_SET_PROP, p, next, false, &|cg: &mut Cg<'a>| cg.set_prop_cached(w1 as u32, w2, a, t, atom, p))
                } else {
                    self.helper(H_SET_PROP, p, next);
                    true
                }
            }
            R_INIT_PROP if self.lay.obj_ok && w1 >= 0 && self.atoms[pc] > 0 && (self.atoms[pc] & 65535) != 0 => {
                let raw_ok = match self.raw(w2) {
                    Opnd::K(k) => !matches!((self.kvals.get(k as usize)).map(|v| v.kind()), Some(VK::Str(_)) | Some(VK::Obj(_)) | None),
                    _ => true,
                };
                if raw_ok {
                    let t = self.atoms[pc];
                    self.with_helper(H_OP, p, next, false, &|cg: &mut Cg<'a>| cg.prop_add(w1 as u32, w2, t, w3, true, p))
                } else {
                    self.helper(H_OP, p, next);
                    true
                }
            }
            R_GET_METHOD if self.lay.obj_ok && w1 >= 0 && w2 >= 0 && self.caches[pc] != 0 => {
                let a = self.caches[pc];
                self.with_helper(H_OP, p, next, false, &|cg: &mut Cg<'a>| cg.pget_pic(w1 as u32, L_SP, w2 as u32 * VS, a, if sub == 0 { 1 } else { 2 }, -1, p))
            }
            R_GET_ELEM if self.lay.obj_ok && w1 >= 0 && w2 >= 0 => {
                let key = self.opnd(w3);
                if key.is_k() {
                    self.helper(H_GET_ELEM, p, next);
                    return true;
                }
                self.with_helper(H_GET_ELEM, p, next, false, &|cg: &mut Cg<'a>| cg.get_elem_fast(w1 as u32, w2 as u32, key, p))
            }
            R_SET_ELEM if self.lay.obj_ok && w1 >= 0 && MASK.with(|m| m.get()) & (1u64 << 61) == 0 => {
                // a typed array's element (set_elem_typed), else the helper
                let key = self.opnd(w2);
                let val = self.opnd(w3);
                if key.is_k() || val.is_k() {
                    self.helper(H_SET_ELEM, p, next);
                    return true;
                }
                self.with_helper(H_SET_ELEM, p, next, false, &|cg: &mut Cg<'a>| cg.set_elem_typed(w1 as u32, key, val, sub == 1, p))
            }
            R_GET_PROP if self.lay.obj_ok && w2 >= 0 && w1 >= 0 && w3 == -4 => {
                // a site that reads an array's length (rrun's c = -4)
                self.with_helper(H_GET_PROP, p, next, false, &|cg: &mut Cg<'a>| cg.array_len(w1 as u32, w2 as u32, p))
            }
            R_GET_PROP if self.lay.obj_ok && w2 >= 0 && w1 >= 0 && self.caches[pc] != 0 => {
                let a = self.caches[pc];
                self.with_helper(H_GET_PROP, p, next, false, &|cg: &mut Cg<'a>| cg.pget_pic(w1 as u32, L_SP, w2 as u32 * VS, a, 0, w3, p))
            }
            R_CMP_JF | R_CMP_JT | R_LOOP | R_CMP_JFK | R_CMP_JTK => {
                let (x, y) = if kind == R_CMP_JFK || kind == R_CMP_JTK { (w2, w3) } else { (w1, w2) };
                if cmp_ok(sub) {
                    let jfk = kind == R_CMP_JFK || kind == R_CMP_JTK;
                    self.with_slow(p, next, true, &|cg: &mut Cg<'a>| {
                        cg.cmp(sub as i32, x, y, p);
                        cg.set(L_C);
                        if jfk {
                            cg.get(L_C);
                            cg.store_bool((w1 & 65535) as u32);
                        }
                        true
                    });
                } else {
                    self.helper(H_SLOW, p, next);
                    let cell = cell_addr() as i64;
                    self.i32c(cell);
                    self.mem(I32_LOAD, 2, 12);
                    self.set(L_C);
                }
                // the answer is in L_C
                match kind {
                    R_LOOP => {
                        let body = (w0 >> 16) as i64;
                        self.get(L_C);
                        self.op(IF);
                        self.op(T_VOID);
                        self.extra += 1;
                        self.jump(body, body <= p);
                        self.op(ELSE);
                        self.jump(w3 as i64, (w3 as i64) <= p);
                        self.extra -= 1;
                        self.op(END);
                        self.op(UNREACHABLE);
                    }
                    _ => {
                        let to = if kind == R_CMP_JFK || kind == R_CMP_JTK { (w1 >> 16) as i64 } else { w3 as i64 };
                        self.get(L_C);
                        if kind == R_CMP_JF || kind == R_CMP_JFK {
                            self.op(I32_EQZ);
                        }
                        self.op(IF);
                        self.op(T_VOID);
                        self.extra += 1;
                        self.jump(to, to <= p);
                        self.extra -= 1;
                        self.op(END);
                    }
                }
                true
            }
            R_CASE_JT => {
                // x === y, the interpreter's rcmp when the fast path fails
                self.with_slow(p, next, true, &|cg: &mut Cg<'a>| {
                    cg.cmp(OP_SEQ as i32, w1, w2, p);
                    cg.set(L_C);
                    true
                });
                self.get(L_C);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w3 as i64, (w3 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            _ => self.fast_op(pc, ins),
        }
    }

    /// One op's code (fast paths fail to the slow block when inside
    /// with_slow); false when it is not written.
    fn fast_op(&mut self, pc: usize, ins: Op) -> bool {
        let w0 = ins.code;
        let (w1, w2, w3) = (ins.a, ins.b, ins.c);
        let kind = (w0 & 255) as int;
        let sub = ((w0 >> 8) & 255) as int;
        let p = pc as i64;
        match kind {
            R_NOP => true,
            R_MOVE => {
                if w1 < 0 {
                    return false;
                }
                let o = self.raw(w2);
                self.load_val(o, L_Q0);
                self.store_val(w1 as u32, L_Q0);
                true
            }
            R_LOAD2 => {
                if w1 < 0 {
                    return false;
                }
                let a = self.raw(w2);
                let b = self.raw(w3);
                self.load_val(a, L_Q0);
                self.load_val(b, L_Q2);
                self.store_val(w1 as u32, L_Q0);
                self.store_val(w1 as u32 + 1, L_Q2);
                true
            }
            R_ADD => self.arith(OP_ADD, w1, w2, w3, p),
            R_SUB => self.arith(OP_SUB, w1, w2, w3, p),
            R_MUL => self.arith(OP_MUL, w1, w2, w3, p),
            R_ARITH => {
                if sub == OP_MOD {
                    self.modulo(w1, w2, w3, p)
                } else {
                    self.arith(sub, w1, w2, w3, p)
                }
            }
            R_GET_GLOBAL | R_GET_ENV | R_SET_ENV | R_THIS | R_TYPEOF | R_CHECK | R_STEP_PUSH | R_STEP_K | R_STEP0 | R_GET_METHOD
            | R_GET_METHOD_ELEM | R_ADD_ENV | R_NEW_OBJECT | R_NEW_ARRAY | R_SET_GLOBAL | R_CLOSURE | R_INIT_PROP | R_WIN => {
                let next = p + 1 + ((w0 >> 16) as i64);
                self.helper(H_OP, p, next);
                true
            }
            R_CALL | R_NEW => {
                let next = p + 1 + ((w0 >> 16) as i64);
                self.call_op(p, next, kind == R_CALL, if w3 > 0 { w3 - 1 } else { w1 }, w1, w2, sub);
                true
            }
            R_RET | R_RET_UNDEF if self.lay.obj_ok && (kind == R_RET_UNDEF || w1 >= 0 || self.raw(w1).is_k()) => {
                // the value into the cell, OUT_RET: a compiled caller
                // pops the frame (call_op), anyone else runs the op
                let cell = cell_addr() as i64;
                if kind == R_RET {
                    let o = self.raw(w1);
                    let (b, off) = self.place(o);
                    self.i32c(cell);
                    self.get(b);
                    self.mem(I64_LOAD, 3, off);
                    self.mem(I64_STORE, 3, 48);
                } else {
                    self.i32c(cell);
                    self.i64c(crate::value::V_UNDEF as i64);
                    self.mem(I64_STORE, 3, 48);
                }
                self.i32c(cell);
                self.i32c(p);
                self.mem(I32_STORE, 2, 8);
                self.exit_at(OUT_RET as i64);
                true
            }
            R_CMP_JFK | R_CMP_JTK => {
                let dst = w1 & 65535;
                let to = (w1 >> 16) as i64;
                if !self.cmp(sub as i32, w2, w3, p) {
                    return false;
                }
                self.set(L_TAG);
                self.get(L_TAG);
                self.store_bool(dst as u32);
                self.get(L_TAG);
                if kind == R_CMP_JFK {
                    self.op(I32_EQZ);
                }
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(to, to <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_JNN_KEEP if sub == 1 => {
                // `x?.`: undefined or null becomes undefined, and jumps
                if w1 < 0 {
                    return false;
                }
                let o = self.raw(w1);
                self.tag(o);
                self.set(L_TAG);
                self.get(L_TAG);
                self.i32c(self.lay.t_undef as i64);
                self.op(I32_EQ);
                self.get(L_TAG);
                self.i32c(self.lay.t_null as i64);
                self.op(I32_EQ);
                self.op(I32_OR);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.store_undef(w1 as u32);
                self.jump(w2 as i64, (w2 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_JNN_KEEP => {
                // not null or undefined: jump
                let o = self.raw(w1);
                if let Opnd::K(k) = o {
                    let nullish = matches!((self.kvals.get(k as usize)).map(|v| v.kind()), Some(VK::Undef) | Some(VK::Null));
                    if !nullish {
                        self.jump(w2 as i64, (w2 as i64) <= p);
                    }
                    return true;
                }
                self.tag(o);
                self.set(L_TAG);
                self.get(L_TAG);
                self.i32c(self.lay.t_undef as i64);
                self.op(I32_NE);
                self.get(L_TAG);
                self.i32c(self.lay.t_null as i64);
                self.op(I32_NE);
                self.op(I32_AND);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w2 as i64, (w2 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_LOCAL_NULL_JF => {
                if w1 < 0 {
                    return false;
                }
                let cmp = w2 as int;
                let o = Opnd::Reg(w1 as u32);
                self.tag(o);
                self.set(L_TAG);
                // r for null, undefined, other; jump when !r
                let rn = cmp == OP_SEQ || cmp == OP_EQ;
                let ru = cmp == OP_SNE || cmp == OP_EQ;
                let ro = cmp == OP_SNE || cmp == OP_NE;
                self.i32c(if rn { 1 } else { 0 });
                self.i32c(if ru { 1 } else { 0 });
                self.i32c(if ro { 1 } else { 0 });
                self.get(L_TAG);
                self.i32c(self.lay.t_undef as i64);
                self.op(I32_EQ);
                self.op(SELECT);
                self.get(L_TAG);
                self.i32c(self.lay.t_null as i64);
                self.op(I32_EQ);
                self.op(SELECT);
                self.op(I32_EQZ);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w3 as i64, (w3 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_SWITCH => {
                let at = w2 as usize;
                if at >= self.rtab.len() {
                    return false;
                }
                let size = self.rtab[at] as usize;
                if size > SWITCH_MAX || at + 1 + size >= self.rtab.len() {
                    return false;
                }
                let tab: Vec<i64> = (0..=size).map(|i| self.rtab[at + 1 + i] as i64).collect();
                if tab.iter().any(|t| *t <= p) {
                    return false;
                }
                let o = self.opnd(w1);
                if let Opnd::K(_) = o {
                    return false;
                }
                // L_C: the index in the table, or -1 (the default)
                self.i32c(-1);
                self.set(L_C);
                self.tag(o);
                self.i32c(self.lay.t_num as i64);
                self.op(I32_EQ);
                self.op(IF);
                self.op(T_VOID);
                self.num(o);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&(w3 as f64).to_le_bytes());
                self.op(F64_SUB);
                self.set(L_A);
                self.get(L_A);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&0f64.to_le_bytes());
                self.op(F64_GE);
                self.get(L_A);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&(size as f64).to_le_bytes());
                self.op(F64_LT);
                self.op(I32_AND);
                self.get(L_A);
                self.op(F64_FLOOR);
                self.get(L_A);
                self.op(F64_EQ);
                self.op(I32_AND);
                self.op(IF);
                self.op(T_VOID);
                self.get(L_A);
                self.op(I32_TRUNC_F64_S);
                self.set(L_C);
                self.op(END);
                self.op(END);
                // the target pc by a chain of selects, then a jump to it
                self.i32c(tab[size]);
                for i in 0..size {
                    if tab[i] == tab[size] {
                        continue;
                    }
                    self.set(L_T2);
                    self.i32c(tab[i]);
                    self.get(L_T2);
                    self.get(L_C);
                    self.i32c(i as i64);
                    self.op(I32_EQ);
                    self.op(SELECT);
                }
                self.set(L_KEY);
                let d = self.exit_depth() + 1;
                self.op(BR);
                self.w.u(d as u64);
                true
            }
            R_CASE_INT_JT => {
                let o = self.opnd(w1);
                if let Opnd::K(_) = o {
                    return false;
                }
                // a number equal to w2: jump
                self.tag(o);
                self.i32c(self.lay.t_num as i64);
                self.op(I32_EQ);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.num(o);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&(w2 as f64).to_le_bytes());
                self.op(F64_EQ);
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w3 as i64, (w3 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_GET_ELEM | R_SET_ELEM | R_GET_PROP | R_SET_PROP | R_GET_THIS_PROP => {
                let next = p + 1 + ((w0 >> 16) as i64);
                let h = match kind {
                    R_GET_ELEM => H_GET_ELEM,
                    R_SET_ELEM => H_SET_ELEM,
                    R_GET_PROP | R_GET_THIS_PROP => H_GET_PROP,
                    _ => H_SET_PROP,
                };
                self.helper(h, p, next);
                true
            }
            R_CMP => {
                if w1 < 0 {
                    return false;
                }
                if !self.cmp(sub as i32, w2, w3, p) {
                    return false;
                }
                self.store_bool(w1 as u32);
                true
            }
            R_CMP_JF | R_CMP_JT => {
                if !self.cmp(sub as i32, w1, w2, p) {
                    return false;
                }
                if kind == R_CMP_JF {
                    self.op(I32_EQZ);
                }
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w3 as i64, (w3 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_LOOP => {
                let body = (w0 >> 16) as i64;
                if !self.cmp(sub as i32, w1, w2, p) {
                    return false;
                }
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(body, body <= p);
                self.op(ELSE);
                self.jump(w3 as i64, (w3 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                // both arms jump
                self.op(UNREACHABLE);
                true
            }
            R_JUMP => {
                self.jump(w1 as i64, (w1 as i64) <= p);
                true
            }
            R_JF | R_JT | R_JF_KEEP | R_JT_KEEP => {
                self.truthy(w1, p);
                if kind == R_JF || kind == R_JF_KEEP {
                    self.op(I32_EQZ);
                }
                self.op(IF);
                self.op(T_VOID);
                self.extra += 1;
                self.jump(w2 as i64, (w2 as i64) <= p);
                self.extra -= 1;
                self.op(END);
                true
            }
            R_NOT => {
                if w1 < 0 {
                    return false;
                }
                self.truthy(w2, p);
                self.op(I32_EQZ);
                self.store_bool(w1 as u32);
                true
            }
            R_INC_LOCAL => {
                if w1 < 0 {
                    return false;
                }
                let o = Opnd::Reg(w1 as u32);
                self.need_num(o, p);
                self.num(o);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&(w2 as f64).to_le_bytes());
                self.op(F64_ADD);
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            R_POSTINC => {
                if w1 < 0 || w2 < 0 {
                    return false;
                }
                let o = Opnd::Reg(w2 as u32);
                self.need_num(o, p);
                self.num(o);
                self.set(L_B);
                self.get(L_B);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&(w3 as f64).to_le_bytes());
                self.op(F64_ADD);
                self.set(L_A);
                self.store_num(w2 as u32, L_A);
                self.store_num(w1 as u32, if sub == 1 { L_A } else { L_B });
                true
            }
            R_INC => {
                if w1 < 0 {
                    return false;
                }
                let o = self.opnd(w2);
                self.need_num(o, p);
                self.num(o);
                self.op(F64_CONST);
                self.w.b.extend_from_slice(&1.0f64.to_le_bytes());
                self.op(if sub == 1 { F64_SUB } else { F64_ADD });
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            R_TONUM => {
                if w1 < 0 {
                    return false;
                }
                let o = self.opnd(w2);
                self.need_num(o, p);
                self.num(o);
                if sub == 1 {
                    self.op(F64_NEG);
                }
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            R_ADD_LOCAL_POP => {
                if w1 < 0 {
                    return false;
                }
                let a = Opnd::Reg(w1 as u32);
                let b = self.opnd(w2);
                self.need_num(a, p);
                self.need_num(b, p);
                self.num(a);
                self.num(b);
                self.op(F64_ADD);
                self.set(L_A);
                self.store_num(w1 as u32, L_A);
                true
            }
            _ => false,
        }
    }
}

/// The jump targets of an op this file writes.
fn targets(ins: Op, pc: usize, rtab: &[int], out: &mut Vec<usize>) {
    let w0 = ins.code;
    let kind = (w0 & 255) as int;
    match kind {
        R_CMP_JF | R_CMP_JT => out.push(ins.c as usize),
        R_LOOP => {
            out.push((w0 >> 16) as usize);
            out.push(ins.c as usize);
        }
        R_JUMP => out.push(ins.a as usize),
        R_JF | R_JT | R_JF_KEEP | R_JT_KEEP | R_JNN_KEEP => out.push(ins.b as usize),
        R_CMP_JFK | R_CMP_JTK => out.push((ins.a >> 16) as usize),
        R_LOCAL_NULL_JF | R_CASE_INT_JT | R_CASE_JT => out.push(ins.c as usize),
        R_SWITCH => {
            let at = ins.b as usize;
            if at < rtab.len() {
                let size = rtab[at] as usize;
                if size <= SWITCH_MAX && at + 1 + size < rtab.len() {
                    for i in 0..=size {
                        out.push(rtab[at + 1 + i] as usize);
                    }
                }
            }
        }
        R_ITER_NEXT => out.push(ins.b as usize),
        _ => {}
    }
    let _ = pc;
}

/// What compile made.
pub struct Compiled {
    /// the function's body (locals and code); module() makes the module
    pub bytes: Vec<u8>,
    /// ops written as code, and all ops
    pub done: u32,
    pub ops: u32,
    pub exits: [u32; 64],
}

/// The module for one function's register code `code` (rcode from its
/// rbase), or None when it has no back edge this file can write.
pub fn compile(_name: &str, code: &[Op], kvals: &[Val], rtab: &[int], lay: Layout, pi: u32, rb: u32, entry: usize, caches: &[u32], pics: &[i32], atoms: &[i32], fsize: u32, inl: Vec<Option<Inl>>, spec: &[bool]) -> Option<Compiled> {
    if !lay.ok || code.is_empty() {
        return None;
    }
    let n = code.len();
    if MASK.with(|m| m.get()) & (1u64 << 58) != 0 {
        // experiment: only code mostly of ops written inline without a call
        let mut light = 0usize;
        let mut all = 0usize;
        for op in code.iter() {
            let k = (op.code & 255) as int;
            if k == R_NOP {
                continue;
            }
            all += 1;
            if matches!(k, R_MOVE | R_LOAD2 | R_ADD | R_SUB | R_MUL | R_ARITH | R_CMP | R_CMP_JF | R_CMP_JT | R_JUMP | R_JF | R_JT | R_JF_KEEP | R_JT_KEEP | R_JNN_KEEP | R_LOOP | R_CMP_JFK | R_CMP_JTK | R_GET_ENV | R_SET_ENV | R_GET_ELEM | R_INC_LOCAL | R_POSTINC | R_INC | R_ADD_LOCAL_POP | R_LOCAL_NULL_JF | R_CASE_INT_JT | R_CASE_JT | R_SWITCH | R_TYPEOF | R_THIS | R_NOT | R_TONUM | R_CHECK) {
                light += 1;
            }
        }
        let pct = MASK.with(|m| m.get()) >> 59 & 1;
        if light * 100 < all * if pct == 1 { 70 } else { 50 } {
            return None;
        }
    }
    let mut starts = vec![false; n];
    starts[0] = true;
    // where a call enters it (h_call)
    if entry < n {
        starts[entry] = true;
    }
    let mut ts = Vec::new();
    for (i, op) in code.iter().enumerate() {
        targets(*op, i, rtab, &mut ts);
    }
    for t in ts {
        if t < n {
            starts[t] = true;
        }
    }
    // an op's skip field steps over the ops after it; where one of them
    // does something (a fused compare's own jump, left for the jumps that
    // still go to it) the fall-through is a jump to the op past them
    let mut skip_to = vec![0usize; n];
    for (i, op) in code.iter().enumerate() {
        let skip = (op.code >> 16) as usize;
        if skip > 0 && (op.code & 255) as int != R_LOOP && i + 1 + skip < n {
            let live = (i + 1..i + 1 + skip).any(|j| (code[j].code & 255) as int != R_NOP || starts[j]);
            if live {
                skip_to[i] = i + 1 + skip;
            }
        }
    }
    for (i, op) in code.iter().enumerate() {
        if skip_to[i] > 0 {
            starts[skip_to[i]] = true;
        }
        if (op.code & 255) as int == R_CALL || (op.code & 255) as int == R_NEW {
            let at = i + 1 + ((op.code >> 16) as usize);
            if at < n {
                starts[at] = true;
            }
        }
    }
    let blocks: Vec<usize> = (0..n).filter(|i| starts[*i]).collect();
    let nb = blocks.len() as u32;
    let (kn_fix, kn_in) = kn_plan(code, rtab, spec, &starts, &skip_to, fsize);
    let mut cg = Cg {
        w: WBuf::new(),
        lay,
        kvals,
        rtab,
        exit_d: 0,
        extra: 0,
        backs: 0,
        done: 0,
        exits: [0; 64],
        pi,
        rb,
        slow_at: None,
        caches: caches.to_vec(),
        pics: pics.to_vec(),
        atoms: atoms.to_vec(),
        fsize,
        callx: false,
        cur_kind: 0,
        inl,
        inlining: false,
        inl_bad: false,
        inl_this: None,
        spec: spec.to_vec(),
        kn: vec![-1; fsize as usize + 2],
        kn_regs: Vec::new(),
        kn_next: 0,
        kn_fix: kn_fix.iter().map(|k| if *k >= 0 { KN0 as i32 + *k } else { -1 }).collect(),
    };
    // locals: out, tag, looped (i32); a, b (f64); q0..q3 (i64); entry, c,
    // t2..t6, ssp, sbp, senv (i32); KN_N f64 from KN0
    cg.w.u(6);
    cg.w.u(3);
    cg.w.byte(T_I32);
    cg.w.u(2);
    cg.w.byte(T_F64);
    cg.w.u(4);
    cg.w.byte(T_I64);
    cg.w.u(10);
    cg.w.byte(T_I32);
    cg.w.u(KN_N as u64);
    cg.w.byte(T_F64);
    cg.w.u(1);
    cg.w.byte(T_I32);
    cg.get(L_KEY);
    cg.set(L_ENTRY);
    cg.i32c(1);
    cg.set(L_FRESH);
    // loop $L / block $exit / block b_{nb-1} .. b_0
    cg.op(LOOP);
    cg.op(T_VOID);
    cg.op(BLOCK);
    cg.op(T_VOID);
    for _ in 0..nb {
        cg.op(BLOCK);
        cg.op(T_VOID);
    }
    cg.get(L_KEY);
    cg.set(L_OUT);
    cg.get(L_KEY);
    cg.op(BR_TABLE);
    cg.w.u(n as u64);
    let mut bi: u32 = 0;
    for i in 0..n {
        if starts[i] {
            cg.w.u(bi as u64);
            bi += 1;
        } else {
            cg.w.u(nb as u64);
        }
    }
    cg.w.u(nb as u64);
    for (k, &start) in blocks.iter().enumerate() {
        cg.op(END);
        cg.exit_d = nb - 1 - k as u32;
        let stop = if k + 1 < blocks.len() { blocks[k + 1] } else { n };
        let mut pc = start;
        let mut open = true;
        // what every way into the block brings (kn_plan); an entry from
        // outside checks it
        cg.kn_block(start, kn_in[start]);
        while pc < stop {
            let ins = code[pc];
            let b0 = cg.w.b.len();
            if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
                // debug: count the op's runs (OPCOUNT)
                let k = (ins.code & 255) as i64;
                let at = std::ptr::addr_of!(OPCOUNT) as usize as i64 + k * 4;
                cg.i32c(at);
                cg.i32c(at);
                cg.mem(I32_LOAD, 2, 0);
                cg.i32c(1);
                cg.op(I32_ADD);
                cg.mem(I32_STORE, 2, 0);
            }
            cg.cur_kind = (ins.code & 255) as u32;
            let wrote = cg.one(pc, ins);
            if MASK.with(|m| m.get()) & (1u64 << 59) != 0 {
                let k = (ins.code & 255) as usize;
                let d = (cg.w.b.len() - b0) as u64;
                OPSIZE.with(|t| {
                    let mut v = t.get();
                    v[k & 255] += d;
                    t.set(v);
                });
            }
            if wrote {
                cg.done += 1;
                let kind = (ins.code & 255) as int;
                if kind == R_JUMP || kind == R_LOOP {
                    // nothing after it runs in this block
                    open = false;
                    break;
                }
                if skip_to[pc] > 0 {
                    cg.jump(skip_to[pc] as i64, false);
                }
                pc += 1;
            } else {
                let kind = (ins.code & 255) as usize;
                if kind < 64 {
                    cg.exits[kind] += 1;
                }
                cg.exit_at(pc as i64);
                // what follows is unreachable here (wasm allows it after a
                // br); it is still written so the block's back edge counts
                pc += 1;
            }
        }
        if open && k + 1 == blocks.len() {
            // past the end: return the last pc
            cg.exit_at((n - 1) as i64);
        }
    }
    cg.op(END); // $exit
    // a pc the code got to carries 2^29 past a back edge and 2^28 when it
    // is not where it started: enter() counts the entries that did not get
    // on; a negative out (budget, throw, frames) is returned as it is
    cg.get(L_OUT);
    cg.i32c(0);
    cg.i32c(1 << 28);
    cg.i32c(0);
    cg.get(L_OUT);
    cg.get(L_ENTRY);
    cg.op(I32_NE);
    cg.op(SELECT);
    cg.get(L_LOOPED);
    cg.op(I32_OR);
    cg.get(L_OUT);
    cg.i32c(0);
    cg.op(I32_LT_S);
    cg.op(SELECT);
    cg.op(I32_OR);
    cg.op(RETURN);
    cg.op(END); // $L
    cg.op(UNREACHABLE);
    cg.op(END); // function
    let done = cg.done;
    if cg.w.b.len() > MAX_BODY {
        // V8's optimizing compile grows faster than the body (a 467 KB
        // body took 1.5 s on Chart.js, 65 KB ones 70 ms): such a function
        // stays with the interpreter
        return None;
    }
    Some(Compiled { bytes: cg.w.b, done, ops: n as u32, exits: cg.exits })
}

/// A module of one function `f` (sp, kv, pc, budget, vm, bp) -> i32 over
/// the imported memory env.memory and the helpers env.<name> (vm, bp, pi,
/// pc, rcode index) -> i32; its body `body` (locals and code).
/// The types, imports (the helpers, and with `rt` look, codes, hstore and
/// callx), memory and table every module of this file starts with.
fn module_head(m: &mut WBuf, rt: bool) {
    m.b.extend_from_slice(b"\0asm");
    m.b.extend_from_slice(&[1, 0, 0, 0]);
    // types: 0 f, 1 a helper, 2 look
    let mut s = WBuf::new();
    s.u(3);
    for np in [NPARAMS, 5, 3] {
        s.byte(0x60);
        s.u(np as u64);
        for _ in 0..np {
            s.byte(T_I32);
        }
        s.u(1);
        s.byte(T_I32);
    }
    m.section(1, &s);
    let mut s = WBuf::new();
    s.u(2 + NHELPERS as u64 + if rt { RT_FNS.len() as u64 } else { 0 });
    for h in HELPERS.iter() {
        s.name("env");
        s.name(h);
        s.byte(0x00);
        s.u(1);
    }
    if rt {
        for (n, t) in RT_FNS.iter() {
            s.name("rt");
            s.name(n);
            s.byte(0x00);
            s.u(*t as u64);
        }
    }
    s.name("env");
    s.name("memory");
    s.byte(0x02);
    s.byte(0x00);
    s.u(1);
    // the function table, for a compiled callee (call_op)
    s.name("env");
    s.name("table");
    s.byte(0x01);
    s.byte(0x70);
    s.byte(0x00);
    s.u(0);
    m.section(2, &s);
}

fn name_section(m: &mut WBuf, list: &[(u32, String)]) {
    let mut fnames = WBuf::new();
    fnames.u(list.len() as u64);
    for (i, n) in list.iter() {
        fnames.u(*i as u64);
        fnames.name(n);
    }
    let mut s = WBuf::new();
    s.name("name");
    s.section(1, &fnames);
    m.section(0, &s);
}

/// The module of a batch of compiled functions: f0, f1, … (bodies in
/// order), which import look, codes, hstore and callx from the runtime
/// module (rt_module), so that V8 compiles those once and not again for
/// every function. The host puts f0… in consecutive table slots.
pub fn module(bodies: &[&[u8]], names: &[String]) -> Vec<u8> {
    let n = bodies.len() as u64;
    let mut m = WBuf::new();
    module_head(&mut m, true);
    let mut s = WBuf::new();
    s.u(n);
    for _ in 0..n {
        s.u(0);
    }
    m.section(3, &s);
    let mut s = WBuf::new();
    s.u(n);
    for k in 0..n {
        s.name(&format!("f{}", k));
        s.byte(0x00);
        s.u(F_SELF as u64 + k);
    }
    m.section(7, &s);
    let mut s = WBuf::new();
    s.u(n);
    for b in bodies.iter() {
        s.u(b.len() as u64);
        s.b.extend_from_slice(b);
    }
    m.section(10, &s);
    let list: Vec<(u32, String)> = names.iter().enumerate().map(|(k, nm)| (F_SELF + k as u32, format!("js:{}", nm))).collect();
    name_section(&mut m, &list);
    m.b
}

/// The runtime module: look, codes, hstore and callx, exported for the
/// compiled functions' modules.
pub fn rt_module(lay: Layout) -> Vec<u8> {
    let kv: [Val; 0] = [];
    let rt: [int; 0] = [];
    let mut cg = Cg {
        w: WBuf::new(),
        lay,
        kvals: &kv,
        rtab: &rt,
        exit_d: 0,
        extra: 0,
        backs: 0,
        done: 0,
        exits: [0; 64],
        pi: 0,
        rb: 0,
        slow_at: None,
        caches: Vec::new(),
        pics: Vec::new(),
        atoms: Vec::new(),
        fsize: 0,
        callx: false,
        cur_kind: 0,
        inl: Vec::new(),
        inlining: false,
        inl_bad: false,
        inl_this: None,
        spec: Vec::new(),
        kn: Vec::new(),
        kn_regs: Vec::new(),
        kn_next: 0,
        kn_fix: Vec::new(),
    };
    let hs = cg.hstore_body();
    let cx = cg.callx_body();
    let pg = cg.pget_body();
    let me = memeq_body();
    let lk = look_body(lay);
    let cd = codes_body();
    let eg = eget_body(lay);
    // gone: its pc back, no locals
    let mut gn = WBuf::new();
    gn.u(0);
    gn.byte(LOCAL_GET);
    gn.u(2);
    gn.byte(END);
    let gn = gn.b;
    let mut m = WBuf::new();
    module_head(&mut m, false);
    let mut s = WBuf::new();
    s.u(RT_FNS.len() as u64);
    for (_, t) in RT_FNS.iter() {
        s.u(*t as u64);
    }
    m.section(3, &s);
    let mut s = WBuf::new();
    s.u(RT_FNS.len() as u64);
    for (k, (n, _)) in RT_FNS.iter().enumerate() {
        s.name(n);
        s.byte(0x00);
        s.u((F_LOOK + k as u32) as u64);
    }
    m.section(7, &s);
    let mut s = WBuf::new();
    s.u(RT_FNS.len() as u64);
    for b in [&lk, &cd, &hs, &cx, &pg, &me, &eg, &gn] {
        s.u(b.len() as u64);
        s.b.extend_from_slice(b);
    }
    m.section(10, &s);
    let names: Vec<(u32, String)> = RT_FNS.iter().enumerate().map(|(k, (n, _))| (F_LOOK + k as u32, format!("jit:{}", n))).collect();
    name_section(&mut m, &names);
    m.b
}

/// Function indices: look, codes, hstore and callx are the runtime
/// module's own functions (rt_module) after the helpers, and imports of a
/// compiled function's module, whose f comes after them (F_SELF)
const F_LOOK: u32 = NHELPERS;
const F_CODES: u32 = NHELPERS + 1;
const F_HSTORE: u32 = NHELPERS + 2;
const F_CALLX: u32 = NHELPERS + 3;
const F_PGET: u32 = NHELPERS + 4;
const F_MEMEQ: u32 = NHELPERS + 5;
const F_EGET: u32 = NHELPERS + 6;
/// gone(sp, kv, pc, …) -> pc: what the host puts in the table slot of an
/// evicted function, so that a call still made to it goes back to the
/// interpreter at once
const F_GONE: u32 = NHELPERS + 7;
const F_SELF: u32 = NHELPERS + 8;
/// rt_module's functions: (name, type)
const RT_FNS: [(&str, u32); 8] = [("look", 2), ("codes", 2), ("hstore", 0), ("callx", 0), ("pget", 0), ("memeq", 2), ("eget", 2), ("gone", 0)];

/// eget(o, atom, vm): the address of the value of property `atom` (a
/// string's, known) of the plain object at o, from its shape or its plain
/// prototypes' (Vm::find), UNDEF16 when no object of the chain has it; 0
/// when it cannot answer (a getter, a long shape, another class)
fn eget_body(l: Layout) -> Vec<u8> {
    let mut w = WBuf::new();
    // locals 3 holder, 4 n, 5 i, 6 depth
    w.u(1);
    w.u(4);
    w.byte(T_I32);
    let get = |w: &mut WBuf, k: u32| {
        w.byte(LOCAL_GET);
        w.u(k as u64);
    };
    let set = |w: &mut WBuf, k: u32| {
        w.byte(LOCAL_SET);
        w.u(k as u64);
    };
    let ld = |w: &mut WBuf, off: u32| {
        w.byte(I32_LOAD);
        w.u(2);
        w.u(off as u64);
    };
    let k = |w: &mut WBuf, x: i64| {
        w.byte(I32_CONST);
        w.s(x);
    };
    let ret0 = |w: &mut WBuf| {
        w.byte(IF);
        w.byte(T_VOID);
        w.byte(I32_CONST);
        w.s(0);
        w.byte(RETURN);
        w.byte(END);
    };
    let cell = cell_addr() as i64;
    w.byte(LOOP);
    w.byte(T_VOID);
    {
        // a C_OBJECT
        get(&mut w, 0);
        ld(&mut w, l.off_class);
        k(&mut w, C_OBJECT as i64);
        w.byte(I32_NE);
        ret0(&mut w);
        // n = shapes[shape].nkeys (<= 64), holder = shapes[its kref]
        get(&mut w, 2);
        ld(&mut w, l.vm_shapes_ptr);
        get(&mut w, 0);
        ld(&mut w, l.off_shape);
        k(&mut w, l.shape_size as i64);
        w.byte(I32_MUL);
        w.byte(I32_ADD);
        w.byte(LOCAL_TEE);
        w.u(3);
        ld(&mut w, l.sh_nkeys);
        w.byte(LOCAL_TEE);
        w.u(4);
        k(&mut w, 64);
        w.byte(I32_GT_S);
        ret0(&mut w);
        get(&mut w, 2);
        ld(&mut w, l.vm_shapes_ptr);
        get(&mut w, 3);
        ld(&mut w, l.sh_kref);
        k(&mut w, l.shape_size as i64);
        w.byte(I32_MUL);
        w.byte(I32_ADD);
        set(&mut w, 3);
        k(&mut w, 0);
        set(&mut w, 5);
        // block $absent loop $scan
        w.byte(BLOCK);
        w.byte(T_VOID);
        w.byte(LOOP);
        w.byte(T_VOID);
        get(&mut w, 5);
        get(&mut w, 4);
        w.byte(I32_GE_S);
        w.byte(BR_IF);
        w.u(1);
        // keys[i] (an int, its low word) == atom
        get(&mut w, 3);
        ld(&mut w, l.sh_keys_p);
        get(&mut w, 5);
        k(&mut w, 8);
        w.byte(I32_MUL);
        w.byte(I32_ADD);
        ld(&mut w, 0);
        get(&mut w, 1);
        w.byte(I32_EQ);
        w.byte(IF);
        w.byte(T_VOID);
        {
            // not an accessor
            get(&mut w, 3);
            ld(&mut w, l.sh_attrs_p);
            get(&mut w, 5);
            k(&mut w, 8);
            w.byte(I32_MUL);
            w.byte(I32_ADD);
            ld(&mut w, 0);
            k(&mut w, P_ACCESSOR as i64);
            w.byte(I32_AND);
            ret0(&mut w);
            // slot i: i * VS < 2 * VS in the object, else in vals
            get(&mut w, 0);
            k(&mut w, l.off_inl0 as i64);
            w.byte(I32_ADD);
            get(&mut w, 5);
            k(&mut w, VS as i64);
            w.byte(I32_MUL);
            w.byte(I32_ADD);
            get(&mut w, 0);
            ld(&mut w, l.off_vals);
            get(&mut w, 5);
            k(&mut w, VS as i64);
            w.byte(I32_MUL);
            w.byte(I32_ADD);
            k(&mut w, 2 * VS as i64);
            w.byte(I32_SUB);
            get(&mut w, 5);
            k(&mut w, 2);
            w.byte(I32_LT_U);
            w.byte(SELECT);
            w.byte(RETURN);
        }
        w.byte(END);
        get(&mut w, 5);
        k(&mut w, 1);
        w.byte(I32_ADD);
        set(&mut w, 5);
        w.byte(BR);
        w.u(0);
        w.byte(END);
        w.byte(END);
        // absent here: undefined without a prototype, else up to 8 deep
        get(&mut w, 0);
        ld(&mut w, l.off_proto);
        k(&mut w, -1);
        w.byte(I32_EQ);
        w.byte(IF);
        w.byte(T_VOID);
        k(&mut w, unsafe { std::ptr::addr_of!(UNDEF16) } as usize as i64);
        w.byte(RETURN);
        w.byte(END);
        get(&mut w, 6);
        k(&mut w, 1);
        w.byte(I32_ADD);
        w.byte(LOCAL_TEE);
        w.u(6);
        k(&mut w, 8);
        w.byte(I32_GT_S);
        ret0(&mut w);
        get(&mut w, 0);
        ld(&mut w, l.off_proto);
        k(&mut w, l.obj_size as i64);
        w.byte(I32_MUL);
        k(&mut w, cell);
        ld(&mut w, 16);
        w.byte(I32_ADD);
        set(&mut w, 0);
        w.byte(BR);
        w.u(0);
    }
    w.byte(END);
    k(&mut w, 0);
    w.byte(END);
    w.b
}

/// memeq(a, b, n): 1 when the n bytes at a and b are the same
fn memeq_body() -> Vec<u8> {
    let mut w = WBuf::new();
    w.u(0);
    let get = |w: &mut WBuf, k: u32| {
        w.byte(LOCAL_GET);
        w.u(k as u64);
    };
    let set = |w: &mut WBuf, k: u32| {
        w.byte(LOCAL_SET);
        w.u(k as u64);
    };
    let k = |w: &mut WBuf, x: i64| {
        w.byte(I32_CONST);
        w.s(x);
    };
    // four bytes at a time while n >= 4, then one
    for (step, ld) in [(4i64, (I32_LOAD, 2u64)), (1, (I32_LOAD8_U, 0))] {
        w.byte(BLOCK);
        w.byte(T_VOID);
        w.byte(LOOP);
        w.byte(T_VOID);
        get(&mut w, 2);
        k(&mut w, step);
        w.byte(I32_LT_S);
        w.byte(BR_IF);
        w.u(1);
        get(&mut w, 0);
        w.byte(ld.0);
        w.u(ld.1);
        w.u(0);
        get(&mut w, 1);
        w.byte(ld.0);
        w.u(ld.1);
        w.u(0);
        w.byte(I32_NE);
        w.byte(IF);
        w.byte(T_VOID);
        k(&mut w, 0);
        w.byte(RETURN);
        w.byte(END);
        for x in [0u32, 1] {
            get(&mut w, x);
            k(&mut w, step);
            w.byte(I32_ADD);
            set(&mut w, x);
        }
        get(&mut w, 2);
        k(&mut w, step);
        w.byte(I32_SUB);
        set(&mut w, 2);
        w.byte(BR);
        w.u(0);
        w.byte(END);
        w.byte(END);
    }
    k(&mut w, 1);
    w.byte(END);
    w.b
}

/// codes(tag, pc, next) -> the pc a helper's code leaves at (helper_codes)
fn codes_body() -> Vec<u8> {
    let mut w = WBuf::new();
    w.u(0);
    let cell = cell_addr() as i64;
    // tag 1: pc
    let pick = |w: &mut WBuf, tag: i64| {
        w.byte(LOCAL_GET);
        w.u(0);
        w.byte(I32_CONST);
        w.s(tag);
        w.byte(I32_EQ);
        w.byte(IF);
        w.byte(T_VOID);
    };
    pick(&mut w, 1);
    w.byte(LOCAL_GET);
    w.u(1);
    w.byte(RETURN);
    w.byte(END);
    pick(&mut w, 2);
    w.byte(LOCAL_GET);
    w.u(2);
    w.byte(RETURN);
    w.byte(END);
    pick(&mut w, 3);
    w.byte(I32_CONST);
    w.s(cell);
    w.byte(I32_LOAD);
    w.u(2);
    w.u(8);
    w.byte(RETURN);
    w.byte(END);
    pick(&mut w, 4);
    w.byte(I32_CONST);
    w.s(OUT_THROW as i64);
    w.byte(RETURN);
    w.byte(END);
    w.byte(I32_CONST);
    w.s(OUT_FRAMES as i64);
    w.byte(END);
    w.b
}

/// look(obj: &objs[id], a: site cache, epoch) -> &the value, or 0: the
/// ways of the cache at a (prop_cache's layout) for the object.
fn look_body(l: Layout) -> Vec<u8> {
    let mut w = WBuf::new();
    // locals 3 shape, 4 class, 5 way, 6 cur, 7 depth, 8 link, 9 end
    w.u(1);
    w.u(7);
    w.byte(T_I32);
    let get = |w: &mut WBuf, k: u32| {
        w.byte(LOCAL_GET);
        w.u(k as u64);
    };
    let set = |w: &mut WBuf, k: u32| {
        w.byte(LOCAL_SET);
        w.u(k as u64);
    };
    let ld = |w: &mut WBuf, off: u32| {
        w.byte(I32_LOAD);
        w.u(2);
        w.u(off as u64);
    };
    let k = |w: &mut WBuf, x: i64| {
        w.byte(I32_CONST);
        w.s(x);
    };
    let cell = cell_addr() as i64;
    get(&mut w, 0);
    ld(&mut w, l.off_shape);
    set(&mut w, 3);
    get(&mut w, 0);
    ld(&mut w, l.off_class);
    set(&mut w, 4);
    get(&mut w, 1);
    set(&mut w, 5);
    get(&mut w, 1);
    k(&mut w, (CACHE_WAYS * CACHE_W * 4) as i64);
    w.byte(I32_ADD);
    set(&mut w, 9);
    w.byte(LOOP);
    w.byte(T_VOID);
    {
        // block $next
        w.byte(BLOCK);
        w.byte(T_VOID);
        get(&mut w, 5);
        ld(&mut w, 0);
        get(&mut w, 3);
        w.byte(I32_NE);
        get(&mut w, 5);
        ld(&mut w, 16);
        get(&mut w, 4);
        w.byte(I32_NE);
        w.byte(I32_OR);
        get(&mut w, 5);
        ld(&mut w, 12);
        get(&mut w, 2);
        w.byte(I32_NE);
        get(&mut w, 5);
        ld(&mut w, 12);
        k(&mut w, -1);
        w.byte(I32_NE);
        w.byte(I32_AND);
        w.byte(I32_OR);
        w.byte(BR_IF);
        w.u(0);
        get(&mut w, 0);
        set(&mut w, 6);
        get(&mut w, 5);
        ld(&mut w, 4);
        set(&mut w, 7);
        get(&mut w, 5);
        k(&mut w, 20);
        w.byte(I32_ADD);
        set(&mut w, 8);
        // block $ok loop $up
        w.byte(BLOCK);
        w.byte(T_VOID);
        w.byte(LOOP);
        w.byte(T_VOID);
        get(&mut w, 7);
        w.byte(I32_EQZ);
        w.byte(BR_IF);
        w.u(1);
        get(&mut w, 6);
        ld(&mut w, l.off_proto);
        get(&mut w, 8);
        ld(&mut w, 0);
        w.byte(I32_NE);
        w.byte(BR_IF);
        w.u(2);
        get(&mut w, 8);
        ld(&mut w, 0);
        k(&mut w, l.obj_size as i64);
        w.byte(I32_MUL);
        k(&mut w, cell);
        ld(&mut w, 16);
        w.byte(I32_ADD);
        w.byte(LOCAL_TEE);
        w.u(6);
        ld(&mut w, l.off_shape);
        get(&mut w, 8);
        ld(&mut w, 4);
        w.byte(I32_NE);
        w.byte(BR_IF);
        w.u(2);
        get(&mut w, 8);
        k(&mut w, 8);
        w.byte(I32_ADD);
        set(&mut w, 8);
        get(&mut w, 7);
        k(&mut w, 1);
        w.byte(I32_SUB);
        set(&mut w, 7);
        w.byte(BR);
        w.u(0);
        w.byte(END);
        w.byte(END);
        // absent (SLOT_ABSENT): undefined while the last object of the
        // chain has no prototype, else the next way
        get(&mut w, 5);
        ld(&mut w, 8);
        k(&mut w, 0);
        w.byte(I32_LT_S);
        w.byte(IF);
        w.byte(T_VOID);
        get(&mut w, 6);
        ld(&mut w, l.off_proto);
        k(&mut w, -1);
        w.byte(I32_NE);
        w.byte(BR_IF);
        w.u(1);
        k(&mut w, unsafe { std::ptr::addr_of!(UNDEF16) } as usize as i64);
        w.byte(RETURN);
        w.byte(END);
        // the slot's address: slot * VS < 2 * VS in the object, else in vals
        get(&mut w, 6);
        k(&mut w, l.off_inl0 as i64);
        w.byte(I32_ADD);
        get(&mut w, 5);
        ld(&mut w, 8);
        w.byte(I32_ADD);
        get(&mut w, 6);
        ld(&mut w, l.off_vals);
        get(&mut w, 5);
        ld(&mut w, 8);
        w.byte(I32_ADD);
        k(&mut w, 2 * VS as i64);
        w.byte(I32_SUB);
        get(&mut w, 5);
        ld(&mut w, 8);
        k(&mut w, 2 * VS as i64);
        w.byte(I32_LT_U);
        w.byte(SELECT);
        w.byte(RETURN);
        w.byte(END);
    }
    get(&mut w, 5);
    k(&mut w, (CACHE_W * 4) as i64);
    w.byte(I32_ADD);
    w.byte(LOCAL_TEE);
    w.u(5);
    get(&mut w, 9);
    w.byte(I32_LT_U);
    w.byte(BR_IF);
    w.u(0);
    w.byte(END);
    k(&mut w, 0);
    w.byte(END);
    w.b
}

// ------------------------------------------------------------------ host

/// The host's side: compile a module and give its `f` a place in this
/// module's function table.
#[cfg(all(target_arch = "wasm32", feature = "wjit"))]
mod host {
    #[link(wasm_import_module = "cerxes_jit")]
    extern "C" {
        /// compiles the module (ptr, len), puts its exports f0, f1, …
        /// into consecutive slots of the function table; the first one's
        /// index, or -1
        fn install(ptr: *const u8, len: usize) -> i32;
        /// puts the runtime module's `gone` into table slots base..base+n
        /// (an evicted module's functions), so that the module can go
        fn release(base: i32, n: i32);
    }

    pub fn put(bytes: &[u8]) -> i32 {
        // SAFETY: the host reads len bytes at ptr and returns
        unsafe { install(bytes.as_ptr(), bytes.len()) }
    }

    pub fn drop_slots(base: i32, n: i32) {
        // SAFETY: the host only writes table slots it gave out
        unsafe { release(base, n) }
    }

    pub fn call(idx: i32, sp: usize, kv: usize, pc: i32, budget: i32, vm: usize, bp: i32) -> i32 {
        // SAFETY: idx is a table index install returned for a function of
        // this type; on wasm32 a function pointer is its table index
        let f: extern "C" fn(i32, i32, i32, i32, i32, i32) -> i32 = unsafe { std::mem::transmute(idx as usize) };
        f(sp as i32, kv as i32, pc, budget, vm as i32, bp)
    }
}

#[cfg(not(all(target_arch = "wasm32", feature = "wjit")))]
mod host {
    pub fn put(_bytes: &[u8]) -> i32 {
        -1
    }
    pub fn drop_slots(_base: i32, _n: i32) {}
    pub fn call(_idx: i32, _sp: usize, _kv: usize, pc: i32, _budget: i32, _vm: usize, _bp: i32) -> i32 {
        pc
    }
}

thread_local! {
    /// 1 the runtime module is installed, -1 it could not be
    static RT: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// The runtime module (rt_module) installed with the host before the
/// first compiled function, whose module imports from it.
fn rt_ready(lay: Layout) -> bool {
    if RT.with(|r| r.get()) == 0 {
        let ok = host::put(&rt_module(lay)) >= 0;
        RT.with(|r| r.set(if ok { 1 } else { -1 }));
    }
    RT.with(|r| r.get()) == 1
}

/// Whether this build can run compiled code.
pub fn available() -> bool {
    cfg!(all(target_arch = "wasm32", feature = "wjit")) && probe().ok
}

thread_local! {
    static LAYOUT: Layout = probe();
}

/// Counters for a report.
thread_local! {
    /// debug (mask bit 59): code bytes written by register op kind
    pub static OPSIZE: std::cell::Cell<[u64; 256]> = const { std::cell::Cell::new([0; 256]) };
}

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub compiled: u32,
    pub refused: u32,
    /// loop heads no longer entered (they left at once too often)
    pub dropped: u32,
    pub bytes: u64,
    pub entries: u64,
    pub ops_done: u64,
    pub ops_all: u64,
    /// by register op kind: how many compiled functions have it as an exit
    pub exits: [u32; 64],
}

impl Default for Stats {
    fn default() -> Stats {
        Stats { compiled: 0, refused: 0, dropped: 0, bytes: 0, entries: 0, ops_done: 0, ops_all: 0, exits: [0; 64] }
    }
}

thread_local! {
    pub static STATS: std::cell::Cell<Stats> = std::cell::Cell::new(Stats::default());
    /// Stats.entries, apart: it changes on every entry
    static ENTRIES: std::cell::Cell<u64> = std::cell::Cell::new(0);
}

pub fn stats() -> Stats {
    let mut s = STATS.with(|s| s.get());
    s.entries = ENTRIES.with(|e| e.get());
    s
}

/// Compiles the function pi (when it has none yet) and runs it from pc;
/// the pc to go on at, or -1 - pc when its budget ran out.
pub fn enter(vm: &mut crate::vm::Vm, pi: usize, bp: i64, pc: i64, budget: i32, head: bool) -> i64 {
    let f = compiled_fn(vm, pi);
    if f < 0 {
        return pc;
    }
    run_at(vm, f, pi, bp, pc, budget, head)
}

/// Function pi's index in the function table, compiled now when it has
/// not been tried; -2 when it cannot be.
pub fn compiled_fn(vm: &mut crate::vm::Vm, pi: usize) -> i64 {
    if vm.jit_fn.len() <= pi {
        vm.jit_fn.resize(pi + 1, -1);
    }
    let mut f = vm.jit_fn[pi];
    if f == PENDING {
        // waits for its batch: flushed when its functions keep coming
        let hits = with_state(vm, |st| {
            st.batch.hits += 1;
            st.batch.hits
        });
        if hits >= BATCH_HITS {
            flush(vm);
        }
        return vm.jit_fn[pi];
    }
    if f == -1 {
        let rb = vm.protos[pi].rbase as usize;
        let n = vm.protos[pi].code.len();
        let caches = if rb + n <= vm.rcode.len() { site_caches(vm, pi, rb, n) } else { Vec::new() };
        // a property write's atom; R_INIT_PROP's transition cache
        let atoms = site_atoms(vm, pi, rb, n);
        let pics = if rb + n <= vm.rcode.len() { site_pics(vm, rb, n) } else { Vec::new() };
        // R_TYPEOF's strings are there before the code reads them
        vm.type_val(&Val::Undef());
        let p = &vm.protos[pi];
        let lay = LAYOUT.with(|l| *l);
        f = -2;
        // debug: mask bit 63 prints each function's register code
        if MASK.with(|m| m.get()) & (1u64 << 63) != 0 {
            eprintln!("{}", crate::regs::dump(p, &vm.rcode));
        }
        if rb + n <= vm.rcode.len() {
            let name = format!("{}#{}", p.name, pi);
            let inl = inline_sites(vm, pi, rb, n);
            // the arithmetic sites to speculate on: none in a function
            // whose speculation failed too often, else the ones that have
            // seen only numbers (debug: mask bit 42 off)
            let spec: Vec<bool> = with_state(vm, |st| {
                let fails = st.spec_fails.get(pi).cloned().unwrap_or(0);
                let on = fails < SPEC_FAILS_MAX && MASK.with(|m| m.get()) & (1u64 << 42) == 0;
                (0..n).map(|i| on && !st.nonnum.get(rb + i).cloned().unwrap_or(false)).collect()
            });
            let p = &vm.protos[pi];
            if let Some(c) = compile(&name, &vm.rcode[rb..rb + n], &vm.kvals, &vm.rtab, lay, pi as u32, rb as u32, p.rentry as usize, &caches, &pics, &atoms, p.fsize as u32, inl, &spec) {
                f = PENDING;
                let full = with_state(vm, |st| {
                    let t = &mut st.batch;
                    t.bytes += c.bytes.len();
                    t.items.push(Pending { pi, name, c });
                    // debug: mask bit 45, each function in a module of its own
                    t.items.len() >= BATCH_FNS || t.bytes >= BATCH_BYTES || MASK.with(|m| m.get()) & (1u64 << 45) != 0
                });
                vm.jit_fn[pi] = f;
                if full {
                    flush(vm);
                }
                return vm.jit_fn[pi];
            } else {
                STATS.with(|s| {
                    let mut t = s.get();
                    t.refused += 1;
                    s.set(t);
                });
            }
        }
        vm.jit_fn[pi] = f;
        calls_fill(vm, pi, f);
    }
    f
}

/// jit_fn's mark for a function compiled and waiting for its batch
const PENDING: i64 = -3;
/// a batch goes to the host at this many functions or body bytes, when
/// its functions have been asked for this many times since, or after this
/// many ends of the machine's run
const BATCH_FNS: usize = 16;
const BATCH_BYTES: usize = 256 * 1024;
const BATCH_HITS: u32 = 64;
const BATCH_ENDS: u32 = 8;

struct Pending {
    pi: usize,
    name: String,
    c: Compiled,
}

#[derive(Default)]
struct Batch {
    items: Vec<Pending>,
    bytes: usize,
    hits: u32,
    /// the machine's ends (runs to no frames) since the batch began
    ends: u32,
}

/// A machine's compiled code: the functions not yet given to the host and
/// the modules it was given. The UI extension host runs many machines on
/// one thread, each with its own.
#[derive(Default)]
struct VmJit {
    id: i64,
    batch: Batch,
    mods: Mods,
    /// by function: how often its speculation failed (spec_failed)
    spec_fails: Vec<u8>,
    /// by rcode index: an arithmetic op whose slow path has run (an
    /// operand that was not a number): not speculated (note_nonnum)
    nonnum: Vec<bool>,
}

/// a function whose speculation failed this often is compiled without
const SPEC_FAILS_MAX: u8 = 4;

/// The interpreter's slow path ran for the arithmetic op at rcode index
/// ri of function pi (Vm::note_nonnum): the site is not speculated from
/// now on, and code that speculated on it goes (spec_failed).
pub fn note_nonnum(vm: &mut crate::vm::Vm, pi: usize, ri: usize) {
    let first = with_state(vm, |st| {
        if ri < st.nonnum.len() && st.nonnum[ri] {
            return false;
        }
        if st.nonnum.len() <= ri {
            st.nonnum.resize(ri + 1, false);
        }
        st.nonnum[ri] = true;
        true
    });
    if first {
        spec_failed(vm, pi);
    }
}

/// Function pi's code speculated that an arithmetic site sees numbers and
/// it did not (Vm::note_nonnum): a function waiting for its batch leaves
/// it, a compiled one's module goes (evict, not counted against
/// EVICT_MAX); either is compiled again once hot, without that site.
pub fn spec_failed(vm: &mut crate::vm::Vm, pi: usize) {
    if pi >= vm.jit_fn.len() || vm.jit_id == 0 {
        return;
    }
    let f = vm.jit_fn[pi];
    if f == PENDING {
        with_state(vm, |st| {
            if let Some(k) = st.batch.items.iter().position(|q| q.pi == pi) {
                let q = st.batch.items.remove(k);
                st.batch.bytes -= q.c.bytes.len();
            }
            if st.spec_fails.len() <= pi {
                st.spec_fails.resize(pi + 1, 0);
            }
            st.spec_fails[pi] = st.spec_fails[pi].saturating_add(1);
        });
        vm.jit_fn[pi] = -1;
        if pi < vm.jit_hot.len() {
            vm.jit_hot[pi] = 0;
        }
        return;
    }
    if f < 0 {
        return;
    }
    let m = with_state(vm, |st| {
        if st.spec_fails.len() <= pi {
            st.spec_fails.resize(pi + 1, 0);
        }
        st.spec_fails[pi] = st.spec_fails[pi].saturating_add(1);
        let ms = &mut st.mods;
        match ms.list.iter().position(|m| f >= m.base && f < m.base + m.pis.len() as i64) {
            Some(k) => {
                let m = ms.list.remove(k);
                ms.bytes -= m.bytes;
                Some(m)
            }
            None => None,
        }
    });
    if let Some(m) = m {
        let mut none: Vec<u8> = Vec::new();
        evict_as(vm, &m, &mut none, false);
    }
}

thread_local! {
    static VMS: std::cell::RefCell<Vec<VmJit>> = const { std::cell::RefCell::new(Vec::new()) };
    static NEXT_ID: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// f on the machine's state (made on first use; the machine gets its id)
fn with_state<R>(vm: &mut crate::vm::Vm, f: impl FnOnce(&mut VmJit) -> R) -> R {
    if vm.jit_id == 0 {
        vm.jit_id = NEXT_ID.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
    }
    let id = vm.jit_id;
    VMS.with(|v| {
        let mut v = v.borrow_mut();
        let k = match v.iter().position(|s| s.id == id) {
            Some(k) => k,
            None => {
                v.push(VmJit { id, ..Default::default() });
                v.len() - 1
            }
        };
        f(&mut v[k])
    })
}

/// A machine that goes gives its modules' table slots back to the host.
impl Drop for crate::vm::Vm {
    fn drop(&mut self) {
        if self.jit_id == 0 {
            return;
        }
        let id = self.jit_id;
        let st = VMS.with(|v| {
            let mut v = v.borrow_mut();
            v.iter().position(|s| s.id == id).map(|k| v.swap_remove(k))
        });
        if let Some(st) = st {
            for m in st.mods.list.iter() {
                host::drop_slots(m.base as i32, m.pis.len() as i32);
            }
        }
    }
}

/// debug: machines with compiled code state
pub fn machines() -> usize {
    VMS.with(|v| v.borrow().len())
}

/// The waiting functions as one module to the host: each gets its table
/// slot (or -2 when the host refused the module).
fn flush(vm: &mut crate::vm::Vm) {
    let t = with_state(vm, |st| std::mem::take(&mut st.batch));
    if t.items.is_empty() {
        return;
    }
    let lay = LAYOUT.with(|l| *l);
    let bodies: Vec<&[u8]> = t.items.iter().map(|q| q.c.bytes.as_slice()).collect();
    let names: Vec<String> = t.items.iter().map(|q| q.name.clone()).collect();
    let base = if rt_ready(lay) { host::put(&module(&bodies, &names)) } else { -1 };
    for (k, q) in t.items.iter().enumerate() {
        let pi = q.pi;
        let c = &q.c;
        STATS.with(|s| {
            let mut st = s.get();
            if base >= 0 {
                st.compiled += 1;
                st.bytes += c.bytes.len() as u64;
                st.ops_done += c.done as u64;
                st.ops_all += c.ops as u64;
                for x in 0..64 {
                    st.exits[x] += c.exits[x];
                }
            } else {
                st.refused += 1;
            }
            s.set(st);
        });
        let f = if base >= 0 { base as i64 + k as i64 } else { -2 };
        if f >= 0 {
            // the pc after a call: the interpreter enters there again
            // when the callee returns
            let rb = vm.protos[pi].rbase as usize;
            let n = vm.protos[pi].code.len();
            for i in 0..n {
                let ins = vm.rcode[rb + i];
                if (ins.code & 255) as int == R_CALL || (ins.code & 255) as int == R_NEW {
                    let at = rb + i + 1 + ((ins.code >> 16) as usize);
                    while vm.jit_res.len() <= at {
                        vm.jit_res.push(false);
                    }
                    vm.jit_res[at] = true;
                }
            }
        }
        vm.jit_fn[pi] = f;
        calls_fill(vm, pi, f);
    }
    if base >= 0 {
        let cap = if MASK.with(|m| m.get()) & (1u64 << 44) != 0 { 64 * 1024 } else { CODE_MAX };
        let mut gone: Vec<Module> = Vec::new();
        let mut counts = with_state(vm, |st| {
            let ms = &mut st.mods;
            ms.bytes += t.bytes;
            ms.list.push(Module { base: base as i64, pis: t.items.iter().map(|q| q.pi).collect(), bytes: t.bytes });
            // the oldest modules go while the code is over its cap (never
            // the one just made)
            while ms.bytes > cap && ms.list.len() > 1 {
                let m = ms.list.remove(0);
                ms.bytes -= m.bytes;
                gone.push(m);
            }
            std::mem::take(&mut ms.evictions)
        });
        for m in gone.iter() {
            evict(vm, m, &mut counts);
        }
        with_state(vm, |st| st.mods.evictions = counts);
    }
}

/// The compiled code a machine keeps, in body bytes; past it the oldest
/// modules are dropped (functions still hot are compiled again)
const CODE_MAX: usize = 4 * 1024 * 1024;

struct Module {
    base: i64,
    pis: Vec<usize>,
    bytes: usize,
}

#[derive(Default)]
struct Mods {
    list: Vec<Module>,
    bytes: usize,
    /// by function: how often it was evicted
    evictions: Vec<u8>,
}

/// a function evicted this often stays with the interpreter (the hot code
/// does not fit under the cap: compiling it again and again would cost
/// more than it gives)
const EVICT_MAX: u8 = 2;

thread_local! {
    /// debug counter: functions evicted
    pub static EVICTED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Module m's functions back to the interpreter: their table slots get
/// `gone`, their records and the call sites' records that name them are
/// cleared, and each must get hot again before it is compiled again.
fn evict(vm: &mut crate::vm::Vm, m: &Module, counts: &mut Vec<u8>) {
    evict_as(vm, m, counts, true);
}

/// evict, counting each function's evictions (EVICT_MAX) when `count`
fn evict_as(vm: &mut crate::vm::Vm, m: &Module, counts: &mut Vec<u8>, count: bool) {
    let n = m.pis.len() as i64;
    host::drop_slots(m.base as i32, n as i32);
    let mine = |f: i64| f >= m.base && f < m.base + n;
    for &pi in m.pis.iter() {
        if pi < vm.jit_fn.len() && mine(vm.jit_fn[pi]) {
            if count {
                if counts.len() <= pi {
                    counts.resize(pi + 1, 0);
                }
                counts[pi] = counts[pi].saturating_add(1);
            }
            vm.jit_fn[pi] = if count && counts[pi] >= EVICT_MAX { -2 } else { -1 };
            calls_fill(vm, pi, -1);
            if pi < vm.jit_hot.len() {
                vm.jit_hot[pi] = 0;
            }
            // its loop heads start again from no misses
            let rb = vm.protos[pi].rbase as usize;
            let end = (rb + vm.protos[pi].code.len()).min(vm.jit_miss.len());
            for ri in rb.min(end)..end {
                vm.jit_miss[ri] = 0;
            }
            EVICTED.with(|e| e.set(e.get() + 1));
        }
    }
    // a call site's record of a callee that went: its proto no longer
    // matches, so the next call fills it again
    for ri in 0..vm.rcode.len().min(vm.jit_cache_at.len()) {
        if (vm.rcode[ri].code & 255) as int == R_CALL && vm.jit_cache_at[ri] >= 0 {
            let b = &mut vm.jit_caches[vm.jit_cache_at[ri] as usize];
            if b.len() > 1 && mine(b[1] as i64) {
                b[0] = -1;
                b[1] = -1;
            }
        }
    }
}

/// The machine ran to its end: flush() when a batch has waited through
/// BATCH_ENDS such ends (a host that runs the machine once per event, as
/// the UI extension host does, would otherwise send a module per event).
pub fn flush_pending(vm: &mut crate::vm::Vm) {
    if vm.jit_id == 0 {
        return;
    }
    if with_state(vm, |st| {
        let t = &mut st.batch;
        if !t.items.is_empty() {
            t.ends += 1;
        }
        t.ends >= BATCH_ENDS
    }) {
        flush(vm);
    }
}

fn run_at(vm: &mut crate::vm::Vm, f: i64, pi: usize, bp: i64, pc: i64, budget: i32, head: bool) -> i64 {
    let ri = (vm.protos[pi].rbase + pc) as usize;
    while vm.jit_miss.len() <= ri {
        vm.jit_miss.push(0);
    }
    if vm.jit_miss[ri] > crate::rvm::JIT_MISS_MAX {
        return pc;
    }
    ENTRIES.with(|e| e.set(e.get() + 1));
    fill_from(vm, bp as i32);
    let sp = vm.stack.as_ptr() as usize + (bp as usize) * VSU;
    let kv = vm.kvals.as_ptr() as usize;
    let vp = vm as *mut crate::vm::Vm as usize;
    let r = host::call(f as i32, sp, kv, pc as i32, budget, vp, bp as i32);
    if MASK.with(|m| m.get()) & (1u64 << 59) != 0 && ENTRIES.with(|e| e.get()) % 64 == 0 {
        // a sample of the entries: where from, where they left
        let to = if r == OUT_RET { cell_get(2) } else { r & !(3 << 28) };
        eprintln!("entry {} {} {} -> {} {}", vm.protos[pi].name, if head { "head" } else { "call" }, pc, to, if r == OUT_RET { "ret" } else if r == OUT_FRAMES { "frames" } else { "" });
    }
    if r == OUT_THROW {
        return -2;
    }
    if r == OUT_FRAMES {
        if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
            // debug: OPCOUNT[150 + the op kind the top frame is at]
            if let Some(t) = vm.frames.last() {
                let ri = (vm.protos[t.proto as usize].rbase + t.pc) as usize;
                if ri < vm.rcode.len() {
                    let k = (vm.rcode[ri].code & 255) as usize;
                    // SAFETY: on the VM's thread
                    unsafe {
                        (*std::ptr::addr_of_mut!(OPCOUNT))[150 + k.min(49)] += 1;
                    }
                }
            }
        }
        return -3;
    }
    // a return the compiled code reached: the interpreter does it at that
    // pc (a compiled caller does it itself, call_op)
    // a return from a loop head entry ran the function to its end: as
    // good as a back edge for jit_miss
    let r = if r == OUT_RET { cell_get(2) | if cell_get(2) as i64 != pc { 1 << 28 } else { 0 } | if head { 1 << 29 } else { 0 } } else { r };
    if r < 0 {
        // the budget ran out at -1 - r
        return -4 - (-1 - r) as i64;
    }
    // jit_miss counts, by where it was entered, the entries that went back
    // to the interpreter before a back edge (from a loop head) or where
    // they began (from a call or a return): code that leaves at once on
    // every pass only costs, and after enough of those that place is not
    // entered again (another loop of the function still is)
    if r & (1 << 29) != 0 || (!head && r & (1 << 28) != 0) {
        if vm.jit_miss[ri] > 0 {
            vm.jit_miss[ri] -= 1;
        }
    } else {
        vm.jit_miss[ri] += 4;
        if vm.jit_miss[ri] > crate::rvm::JIT_MISS_MAX {
            STATS.with(|s| {
                let mut t = s.get();
                t.dropped += 1;
                if MASK.with(|m| m.get()) & (1u64 << 62) != 0 {
                    let rbx = vm.protos[pi].rbase as usize;
                    let rr = (r & 0x0fffffff) as usize;
                    eprintln!("dropped {} {} at pc {} r {} op {} -> op {}", pi, vm.protos[pi].name, pc, r, vm.rcode[rbx + pc as usize].code & 255, if rbx + rr < vm.rcode.len() { vm.rcode[rbx + rr].code & 255 } else { -1 });
                }
                s.set(t);
            });
        }
    }
    (r & !(3 << 28)) as i64
}

// ---------------------------------------------------------------- helpers
//
// The ops the compiled code does not write itself but asks the VM for:
// each does what the interpreter's fast path does (rvm.rs) and nothing
// that can run JavaScript, push a frame or move Vm.stack. 0 done, 1 not
// done (the interpreter runs the op), 2 done but go back to the
// interpreter after it (a collection is due).

/// The VM a helper was called with.
///
/// SAFETY: vm is the address enter() passed, of the Vm whose rrun called
/// the compiled code; rrun does not touch it until the call returns.
#[inline(always)]
unsafe fn vm_of<'a>(vm: i32) -> &'a mut crate::vm::Vm {
    &mut *(vm as usize as *mut crate::vm::Vm)
}

/// Native id's answer for the call at fpos with argc arguments when it is
/// one of a number's or string's (builtins.rs math, String, isFinite,
/// Number.prototype.toString) and its operands are numbers or strings;
/// None for the full call.
fn pure_native(vm: &mut crate::vm::Vm, id: int, fpos: int, argc: int) -> Option<Val> {
    use crate::builtins::*;
    let arg = |vm: &crate::vm::Vm, k: int| if k < argc { vm.stack[(fpos + 2 + k) as usize] } else { Val::Undef() };
    let a0 = arg(vm, 0);
    if id >= NF_MATH && id < NF_MATH + 28 {
        if let Some(x) = a0.as_num() {
            return Some(Val::Num(math1(id - NF_MATH, x)));
        }
        return None;
    }
    if id >= NF_MATH + 28 && id <= NF_MATH + 30 {
        if a0.is_num() && (arg(vm, 1)).is_num() { 
            let args = vec![a0, arg(vm, 1)];
            return Some(vm.math(id - NF_MATH, &args));
        }
        return None;
    }
    if id == NF_STRING && argc >= 1 {
        return if a0.is_str() { Some(a0) } else if let Some(n) = a0.as_num() { Some(vm.string_val(crate::num::number_to_string(n))) } else { None };
    }
    if id == NF_ISFINITE {
        if let Some(n) = a0.as_num() {
            return Some(Val::Bool(crate::num::is_finite(n)));
        }
        return None;
    }
    if id == NF_SP_CHARCODEAT && argc >= 1 {
        let t = vm.stack[(fpos + 1) as usize];
        let r = vm.char_code_fast(&t, &a0);
        if !r.is_undef() {
            return Some(r);
        }
        return None;
    }
    if id == NF_NP_TOSTRING && argc <= 1 {
        let rx = if argc == 0 { Val::Num(10.0) } else { a0 };
        if (vm.stack[(fpos + 1) as usize]).is_num() && rx.is_num() { let n = (vm.stack[(fpos + 1) as usize]).num(); let r = rx.num();
            if r == 2.0 || r == 8.0 || r == 10.0 || r == 16.0 || r == 36.0 {
                return Some(vm.string_val(crate::num::radix_string(n, r as int)));
            }
        }
    }
    None
}

fn after(vm: &crate::vm::Vm) -> i32 {
    if vm.gc_due() {
        2
    } else {
        0
    }
}

/// R_GET_ELEM: an array element by a whole number in range.
fn h_get_elem(vm: i32, bp: i32, _pi: i32, _pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let mut r = Val::Undef();
    let mut done = false;
    if (vm.rget(bp, ins.b as int)).is_obj() && (vm.rget(bp, ins.c as int)).is_num() { let o = (vm.rget(bp, ins.b as int)).obj(); let n = (vm.rget(bp, ins.c as int)).num();
        let ob = &vm.objs[o as usize];
        if ob.class == C_ARRAY {
            let i = n as int;
            let ev = &vm.evecs[ob.elems as usize].v;
            if i >= 0 && i < (ev.len() as int) && (i as double) == n {
                r = ev[i as usize];
                done = true;
            }
        } else if ob.class == C_TYPED {
            // a typed array's element (Vm::ta_slot)
            let i = n as int;
            if (i as double) == n {
                let sl = vm.ta_slot(o, i);
                if sl >= 0 {
                    let bc = vm.objs[ob.env as usize].cold as usize;
                    r = Val::Num(vm.colds[bc].nums[sl as usize]);
                    done = true;
                }
            }
        }
    }
    if !done {
        let v = *vm.rget(bp, ins.b as int);
        let k = *vm.rget(bp, ins.c as int);
        if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
            // debug: HCOUNT[1200 + object class (0..30, 31 not an object)
            // * 3 + key kind (0 number, 1 string with an atom, 2 other)]
            let c = if let Some(o) = (&v).as_obj() { (vm.objs[o as usize].class as usize).min(30) } else { 31 };
            let kk = if (&k).is_num() { 0 } else if let Some(h) = (&k).as_str() { if vm.strs[h as usize].atom >= 0 { 1 } else { 2 } } else { 2 };
            HCOUNT.with(|h| {
                let mut t = h.get();
                t[1200 + c * 3 + kk] += 1;
                h.set(t)
            });
        }
        let fi = vm.frames.len() - 1;
        vm.frames[fi].pc = _pc as int + 1;
        if v.is_obj() && k.is_str() { let po = v.obj(); let kh = k.strh();
            if vm.objs[po as usize].class == C_PROXY && vm.strs[kh as usize].atom >= 0 {
                let a = vm.strs[kh as usize].atom as int;
                let fpos = bp + vm.protos[_pi as usize].fsize;
                if vm.r_proxy_get(po, a, v, fpos, bp + ins.a as int) {
                    return 5;
                }
            }
        }
        let r = vm.get_elem(&v, &k);
        if !vm.throwing {
            vm.rset((bp + ins.a as int) as usize, r);
        }
        return settle(vm, fi, _pc as int);
    }
    vm.rset((bp + ins.a as int) as usize, r);
    0
}

/// After a slow path: 4 it threw, 5 frames changed, else as after().
fn settle(vm: &mut crate::vm::Vm, fi: usize, pc: int) -> i32 {
    if vm.throwing {
        if vm.frames.len() == fi + 1 {
            vm.frames[fi].pc = pc + 1;
        }
        return 4;
    }
    if vm.frames.len() != fi + 1 {
        return 5;
    }
    after(vm)
}

/// R_SET_ELEM: an array element by a whole number in range, or one past
/// the end of an extensible array.
fn h_set_elem(vm: i32, bp: i32, _pi: i32, pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let val = *vm.rget(bp, ins.c as int);
    // sub 1: the assignment's value also into w1, where the object was
    let keep = ((ins.code >> 8) & 255) == 1;
    if set_elem_fast(vm, bp, ins, val) {
        if keep {
            vm.rset((bp + ins.a as int) as usize, val);
        }
        return after(vm);
    }
    let fi = vm.frames.len() - 1;
    vm.r_set_elem_slow(bp, fi, pc as int, ins.a as int, ins.b as int, val);
    if keep && !vm.throwing {
        vm.rset((bp + ins.a as int) as usize, val);
    }
    settle(vm, fi, pc as int)
}

fn set_elem_fast(vm: &mut crate::vm::Vm, bp: int, ins: Op, val: Val) -> bool {
    let o = crate::vm::obj_of(vm.rget(bp, ins.a as int));
    let n = if let Some(x) = (vm.rget(bp, ins.b as int)).as_num() { x } else { return false };
    if o >= 0 && vm.objs[o as usize].class == C_TYPED {
        let i = n as int;
        return if let Some(x) = val.as_num() { (i as double) == n && vm.ta_put_num(o, i, x) } else { false };
    }
    if o < 0 || vm.objs[o as usize].class != C_ARRAY || vm.objs[o as usize].pos >= 2 {
        return false;
    }
    let i = n as int;
    if i < 0 || (i as double) != n {
        return false;
    }
    let e = vm.objs[o as usize].elems as usize;
    let len = vm.evecs[e].v.len();
    if i < (len as int) {
        vm.inc(&val);
        let old = crate::vm::obj_of(&vm.evecs[e].v[i as usize]);
        let old_s = crate::vm::str_of(&vm.evecs[e].v[i as usize]);
        elem_put(&mut vm.evecs[e], i as usize, val);
        let oc = vm.objs[o as usize].cold as usize;
        if oc > 0 && i < (vm.colds[oc].elems2.len() as int) {
            vm.colds[oc].elems2[i as usize] = Val::Bool(false);
        }
        vm.dec_o(old);
        vm.sdec(old_s);
        return true;
    }
    if i == (len as int) && e != 0 && vm.objs[o as usize].extensible && vm.objs[o as usize].pos != 3 {
        vm.inc(&val);
        vm.evecs[e].v.push(val);
        return true;
    }
    false
}

/// R_GET_PROP: an own data property its inline cache knows, or an array's
/// length.
fn h_get_prop(vm: i32, bp: i32, pi: i32, pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let o = if (ins.code & 255) as int == R_GET_THIS_PROP {
        match vm.frames.last() {
            Some(f) => crate::vm::obj_of(&f.this_val),
            None => -1,
        }
    } else {
        crate::vm::obj_of(vm.rget(bp, ins.b as int))
    };
    if o < 0 {
        return get_prop_slow(vm, bp, pi, pc, ri, o);
    }
    let c = ins.c;
    let ob = &vm.objs[o as usize];
    if c >= 65536 && (c >> 16) == ob.shape {
        let r = ob.get_slot((c & 65535) as usize);
        vm.rset((bp + ins.a as int) as usize, r);
        let atom = vm.ratom(pi as usize, pc as int);
        cache_fill(vm, ri as usize, o, atom);
        return 0;
    }
    if ob.class == C_ARRAY && vm.ratom(pi as usize, pc as int) == crate::vm::A_LENGTH {
        let n = vm.evecs[ob.elems as usize].v.len() as double;
        vm.rset((bp + ins.a as int) as usize, Val::Num(n));
        if c < 65536 && vm.find(o, crate::vm::A_LENGTH) < 0 {
            // the site reads arrays' lengths (rrun and compiled code)
            vm.rcode[ri as usize].c = -4;
        }
        return 0;
    }
    if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
        // debug: misses by class * 4 + (dictionary 1 | getter 2)
        let ob = &vm.objs[o as usize];
        let atom = vm.ratom(pi as usize, pc as int);
        let f = vm.find(o, atom);
        // 1: on the prototype chain (else absent)
        let mut onp = 0;
        let mut q = ob.proto;
        let mut d = 0;
        while q >= 0 && d < 20 {
            if vm.find(q as int, atom) >= 0 {
                onp = 1 + (d >= 3) as usize;
                break;
            }
            q = vm.objs[q as usize].proto;
            d += 1;
        }
        let k = 900 + (ob.class as usize).min(19) * 4 + if f < 0 { 2 + onp.min(1) } else { 0 } + if onp == 2 { 80 } else { 0 };
        HCOUNT.with(|h| { let mut v = h.get(); v[k.min(2047)] += 1; h.set(v) });
    }
    let r = get_prop_slow(vm, bp, pi, pc, ri, o);
    if r == 0 || r == 2 {
        let atom = vm.ratom(pi as usize, pc as int);
        cache_fill(vm, ri as usize, o, atom);
    }
    r
}

fn get_prop_slow(vm: &mut crate::vm::Vm, bp: int, pi: i32, pc: i32, ri: i32, o: int) -> i32 {
    let ins = vm.rcode[ri as usize];
    let fi = vm.frames.len() - 1;
    let rb = vm.protos[pi as usize].rbase;
    if vm.r_get_prop_slow(o, pi as usize, rb, bp, fi, pc as int, ins.code as int, ins.a as int, ins.b as int) {
        return 5;
    }
    settle(vm, fi, pc as int)
}

/// R_SET_PROP: an own data property its inline cache knows.
fn h_set_prop(vm: i32, bp: i32, _pi: i32, _pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let bp = bp as int;
    let o = crate::vm::obj_of(vm.rget(bp, ins.a as int));
    let w3 = ins.c;
    let fi = vm.frames.len() - 1;
    let pi = vm.frames[fi].proto as usize;
    let sh = if o >= 0 { vm.objs[o as usize].shape } else { -1 };
    if o >= 0 && w3 >= 65536 && (w3 >> 16) == sh && ((ins.code >> 8) & 255) == 0 {
        let val = *vm.rget(bp, ins.b as int);
        vm.put_slot(o, (w3 & 65535) as int, val);
    } else {
        let rb = vm.protos[pi].rbase;
        vm.r_set_prop_slow(o, pi, rb, bp, fi, _pc as int, ins.code as int, ins.a as int, ins.b as int, w3 as int);
    }
    if o >= 0 && !vm.throwing && vm.frames.len() == fi + 1 && vm.objs[o as usize].shape == sh {
        // a write to a property it had: the site's own way
        let atom = vm.ratom(pi, _pc as int);
        set_cache_fill(vm, ri as usize, o, atom);
    }
    settle(vm, fi, _pc as int)
}

// ------------------------------------------------------------ the cell
//
// After a helper the compiled code reads the stack's and the constants'
// addresses again from a cell the helper filled in (cell[0] the frame's
// registers, cell[1] kvals, cell[2] a pc for code 3).

static mut CELL: [i32; 16] = [0; 16];
thread_local! {
    /// debug (mask bit 54): native calls through H_CALL by native id
    pub static NATS: std::cell::Cell<Vec<u32>> = const { std::cell::Cell::new(Vec::new()) };
}
/// debug (mask bit 54): runs of compiled ops by register op kind
pub static mut OPCOUNT: [u32; 256] = [0; 256];
/// A plain call's operands for the module's callx (call_op writes them):
/// f, dst, argc, next, pc, rcode index, call cache address
static mut CARGS: [i32; 16] = [0; 16];

fn cargs_addr() -> usize {
    // SAFETY: only the address is taken
    unsafe { std::ptr::addr_of!(CARGS) as usize }
}

fn cell_addr() -> usize {
    // SAFETY: only the address is taken
    unsafe { std::ptr::addr_of!(CELL) as usize }
}

/// an op whose code goes on after its helper whatever it returned 0 for
/// (a comparison that jumps): it reloads the stack's and constants'
/// addresses (helper_raw)
fn goes_on(kind: int) -> bool {
    matches!(kind, R_CMP_JF | R_CMP_JT | R_LOOP | R_CASE_JT | R_CMP_JFK | R_CMP_JTK)
}

thread_local! {
    /// debug (mask 54): helper calls by helper * 64 + op kind
    pub static HCOUNT: std::cell::Cell<[u32; 2048]> = const { std::cell::Cell::new([0; 2048]) };
}

/// where the stack and the constants are
fn moved_key(vm: i32) -> (usize, usize) {
    // SAFETY: as vm_of
    let v = unsafe { vm_of(vm) };
    (v.stack.as_ptr() as usize, v.kvals.as_ptr() as usize)
}

fn fill_cell(vm: i32, bp: i32) {
    // SAFETY: as vm_of
    fill_from(unsafe { vm_of(vm) }, bp);
}

fn fill_from(v: &crate::vm::Vm, bp: i32) {
    // SAFETY: CELL is only touched on this (the VM's) thread
    unsafe {
        let c = &mut *std::ptr::addr_of_mut!(CELL);
        c[0] = (v.stack.as_ptr() as usize + (bp as usize) * VSU) as i32;
        c[1] = v.kvals.as_ptr() as usize as i32;
        c[4] = v.objs.as_ptr() as usize as i32;
        c[5] = top_this(v);
    }
}

/// the address of the top frame's this_val (cell[5])
fn top_this(v: &crate::vm::Vm) -> i32 {
    match v.frames.last() {
        Some(f) => (&f.this_val as *const Val) as usize as i32,
        None => 0,
    }
}

fn cell_get(k: usize) -> i32 {
    // SAFETY: as fill_cell
    unsafe { (*std::ptr::addr_of!(CELL))[k] }
}

/// cell[9]: how deep compiled code calls compiled code (call_op)
fn depth_add(d: i32) {
    // SAFETY: as fill_cell
    unsafe {
        let c = &mut *std::ptr::addr_of_mut!(CELL);
        c[9] += d;
    }
}

fn set_cell_pc(pc: int) {
    // SAFETY: as fill_cell
    unsafe {
        let c = &mut *std::ptr::addr_of_mut!(CELL);
        c[2] = pc as i32;
    }
}

macro_rules! export_helper {
    ($name:ident, $inner:ident, $leave:expr, $hk:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(vm: i32, bp: i32, pi: i32, pc: i32, ri: i32) -> i32 {
            if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
                let k = unsafe { vm_of(vm) }.rcode[ri as usize].code as usize & 255;
                HCOUNT.with(|h| { let mut v = h.get(); v[k.min(63) + 64 * ($hk as usize)] += 1; h.set(v) });
            }
            let was = moved_key(vm);
            let r = $inner(vm, bp, pi, pc, ri);
            fill_cell(vm, bp);
            // the stack or the constants moved: the code that called (it
            // keeps their addresses, helper_raw) goes on in the
            // interpreter from the next op
            if $leave && r == 0 && moved_key(vm) != was && !goes_on(unsafe { vm_of(vm) }.rcode[ri as usize].code as int & 255) {
                return 2;
            }
            r
        }
    };
}

/// Helper k (HELPERS order) as its index in the function table. A host
/// imports the helpers from the table rather than from the exports: a
/// wasm32-wasip1 export is wrapped to run the constructors and destructors
/// around each call.
#[no_mangle]
pub extern "C" fn cerxes_jit_helper(k: i32) -> i32 {
    let f: extern "C" fn(i32, i32, i32, i32, i32) -> i32 = match k {
        0 => cerxes_jit_get_elem,
        1 => cerxes_jit_set_elem,
        2 => cerxes_jit_get_prop,
        3 => cerxes_jit_set_prop,
        4 => cerxes_jit_op,
        5 => cerxes_jit_call,
        6 => cerxes_jit_slow,
        _ => cerxes_jit_ret,
    };
    f as usize as i32
}

export_helper!(cerxes_jit_get_elem, h_get_elem, true, 0);
export_helper!(cerxes_jit_set_elem, h_set_elem, true, 1);
export_helper!(cerxes_jit_get_prop, h_get_prop, true, 2);
export_helper!(cerxes_jit_set_prop, h_set_prop, true, 3);
export_helper!(cerxes_jit_op, h_op, true, 4);
export_helper!(cerxes_jit_call, h_call, false, 5);
export_helper!(cerxes_jit_slow, h_slow, true, 6);
export_helper!(cerxes_jit_ret, h_ret, false, 7);

fn set_cell_c(r: bool) {
    // SAFETY: as fill_cell
    unsafe {
        let c = &mut *std::ptr::addr_of_mut!(CELL);
        c[3] = if r { 1 } else { 0 };
    }
}

/// An op whose fast path in the compiled code did not take its operands
/// (not numbers, not int32s, a constant): what the interpreter does on
/// its slow path. A comparison leaves its answer in cell[3].
fn h_slow(vm: i32, bp: i32, pi: i32, pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let (w0, w1, w2, w3) = (ins.code as int, ins.a as int, ins.b as int, ins.c as int);
    let bp = bp as int;
    let pi = pi as usize;
    let pc = pc as int;
    let nf = vm.frames.len();
    let fi = nf - 1;
    let sub = (w0 >> 8) & 255;
    let kind = w0 & 255;
    let mut gc_ok = true;
    if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
        let (x, y) = if kind == R_CMP_JFK || kind == R_CMP_JTK || kind == R_CMP { (w2, w3) } else { (w1, w2) };
        if matches!(kind, R_CMP_JFK | R_CMP_JTK | R_CMP | R_CMP_JF | R_CMP_JT | R_CASE_JT) {
            let t = |v: &Val| match v.kind() { VK::Undef => 0, VK::Null => 1, VK::Bool(_) => 2, VK::Num(_) => 3, VK::Str(_) => 4, _ => 5 };
            let mut k = 512 + ((sub - 42).clamp(0, 13) as usize) * 36 + t(vm.rget(bp, x)) * 6 + t(vm.rget(bp, y));
            if (vm.rget(bp, x)).is_str() && (vm.rget(bp, y)).is_str() { let p = (vm.rget(bp, x)).strh(); let q = (vm.rget(bp, y)).strh();
                let (e, f) = (&vm.strs[p as usize], &vm.strs[q as usize]);
                // 1000 + same len * 8 + up both <0 *4 + eq content*2 + same handle
                k = 1000 + (e.len == f.len) as usize * 8 + (e.up < 0 && f.up < 0) as usize * 4 + (vm.sref(p) == vm.sref(q)) as usize * 2 + (p == q) as usize;
            }
            HCOUNT.with(|h| { let mut v = h.get(); v[k.min(2047)] += 1; h.set(v) });
        }
    }
    match kind {
        R_ADD => vm.r_add_slow(bp, fi, pc, w1, w2, w3),
        R_SUB | R_MUL | R_ARITH => {
            let code = if kind == R_SUB {
                OP_SUB
            } else if kind == R_MUL {
                OP_MUL
            } else {
                sub
            };
            // the interpreter's whole arm: its own fast paths first
            let mut done = false;
            if (*vm.rget(bp, w2)).is_num() && (*vm.rget(bp, w3)).is_num() { let a = (*vm.rget(bp, w2)).num(); let b = (*vm.rget(bp, w3)).num();
                let r = if code == OP_SUB {
                    a - b
                } else if code == OP_MUL {
                    a * b
                } else if code == OP_MOD && a >= 1.0 && b > 0.0 && a <= 2147483647.0 && b <= 2147483647.0 && a == ((a as int) as double) && b == ((b as int) as double) {
                    ((a as int) % (b as int)) as double
                } else if code >= OP_BAND && code <= OP_SHR && a >= -2147483648.0 && a <= 2147483647.0 && b >= -2147483648.0 && b <= 2147483647.0 {
                    let (p, q) = (a as int, b as int);
                    match code {
                        OP_BAND => (p & q) as double,
                        OP_BOR => (p | q) as double,
                        OP_BXOR => (p ^ q) as double,
                        OP_SHL => crate::vm::wrap32(p << (q & 31)) as double,
                        _ => (p >> (q & 31)) as double,
                    }
                } else if code >= OP_BAND && code <= OP_USHR {
                    crate::rvm::bit_num(code, a, b)
                } else {
                    crate::vm::arith_num(code, a, b)
                };
                vm.rset((bp + w1) as usize, Val::Num(r));
                done = true;
            }
            if !done {
                vm.r_arith_slow(code, bp, fi, pc, w1, w2, w3);
            }
        }
        R_CMP => {
            let r = vm.rcmp(sub, pi, bp, w2, w3, fi, pc);
            if !vm.throwing {
                vm.rset((bp + w1) as usize, Val::Bool(r));
            }
        }
        R_CMP_JF | R_CMP_JT | R_LOOP => {
            gc_ok = false;
            let r = vm.rcmp(sub, pi, bp, w1, w2, fi, pc);
            set_cell_c(r);
        }
        R_CASE_JT => {
            gc_ok = false;
            let r = vm.rcmp(OP_SEQ, pi, bp, w1, w2, fi, pc);
            set_cell_c(r);
        }
        R_CMP_JFK | R_CMP_JTK => {
            gc_ok = false;
            let r = vm.rcmp(sub, pi, bp, w2, w3, fi, pc);
            if !vm.throwing {
                vm.stack[(bp + (w1 & 65535)) as usize] = Val::Bool(r);
            }
            set_cell_c(r);
        }
        R_INC_LOCAL => {
            let i = (bp + w1) as usize;
            let v = vm.stack[i];
            vm.frames[fi].pc = pc + 1;
            let (_, nv) = vm.numeric_step(&v, w2);
            if !vm.throwing {
                vm.stack[i] = nv;
            }
        }
        R_POSTINC => {
            let i = (bp + w2) as usize;
            let pre = sub == 1;
            let v = vm.stack[i];
            vm.frames[fi].pc = pc + 1;
            let (old, nv) = vm.numeric_step(&v, w3);
            if !vm.throwing {
                vm.stack[i] = nv;
                vm.rset((bp + w1) as usize, if pre { nv } else { old });
            }
        }
        R_INC => {
            let dec = sub == 1;
            if let Some(x) = (*vm.rget(bp, w2)).as_num() {
                vm.rset((bp + w1) as usize, Val::Num(if dec { x - 1.0 } else { x + 1.0 }));
            } else {
                vm.r_unary_slow(bp, fi, pc, if dec { 3 } else { 2 }, w1, w2);
            }
        }
        R_TONUM => {
            let neg = sub == 1;
            if let Some(x) = (*vm.rget(bp, w2)).as_num() {
                vm.rset((bp + w1) as usize, Val::Num(if neg { -x } else { x }));
            } else {
                vm.r_unary_slow(bp, fi, pc, if neg { 1 } else if sub == 2 { 4 } else { 0 }, w1, w2);
            }
        }
        R_ADD_LOCAL_POP => {
            let b = *vm.rget(bp, w2);
            let i = (bp + w1) as usize;
            let h = vm.append_val(vm.stack[i], b);
            if h >= 0 {
                vm.stack[i] = Val::Str(h);
            } else {
                let a = vm.stack[i];
                let r = if a.is_num() && b.is_num() { let x = a.num(); let y = b.num();
                    Val::Num(x + y)
                } else {
                    vm.frames[fi].pc = pc + 1;
                    vm.add_vals(&a, &b)
                };
                if !vm.throwing {
                    vm.stack[i] = r;
                }
            }
        }
        _ => return 1,
    }
    if vm.throwing {
        if vm.frames.len() == nf {
            vm.frames[fi].pc = pc + 1;
        }
        return 4;
    }
    if vm.frames.len() != nf {
        return 5;
    }
    if gc_ok {
        after(vm)
    } else {
        0
    }
}

/// The ops the interpreter does with a method or a few lines that cannot
/// push a frame of the compiled function's own (rvm.rs, the same arms).
fn h_op(vm: i32, bp: i32, pi: i32, pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let (w0, w1, w2, w3) = (ins.code as int, ins.a as int, ins.b as int, ins.c as int);
    let bp = bp as int;
    let pi = pi as usize;
    let pc = pc as int;
    if MASK.with(|m| m.get()) & (1u64 << 54) != 0 && (w0 & 255) == R_ITER_NEXT {
        // debug: iterators by class * 8 + kind, cold home set * 4
        let o = crate::vm::obj_of(vm.rget(bp, w1));
        let k = if o >= 0 {
            let ob = &vm.objs[o as usize];
            let home = (vm.colds[ob.cold as usize].home >= 0) as usize;
            1100 + (ob.class as usize).min(15) + if ob.class == C_ITER { (ob.pos as usize & 3) * 16 + home * 4 * 16 } else { 0 }
        } else { 1099 };
        HCOUNT.with(|h| { let mut v = h.get(); v[k.min(2047)] += 1; h.set(v) });
    }
    if MASK.with(|m| m.get()) & (1u64 << 59) != 0 {
        OPN.with(|n| n.set(n.get() + 1));
        if OPN.with(|n| n.get()) % 256 == 0 {
            eprintln!("hop {} {}", w0 & 255, (w0 >> 8) & 255);
            if (w0 & 255) == R_GET_METHOD || (w0 & 255) == R_SET_ENV {
                let v = if (w0 & 255) == R_GET_METHOD { *vm.rget(bp, w2) } else { *vm.rget(bp, w1) };
                let cl = if let Some(o) = v.as_obj() { vm.objs[o as usize].class } else { -1 };
                let tg = vm.type_index(&v);
                let atom = vm.protos[pi].code[pc as usize].a as int;
                let nm = if (w0 & 255) == R_GET_METHOD && atom >= 0 && (atom as usize) < vm.atom_names.len() { vm.atom_str(atom) } else { String::new() };
                let mut extra = String::new();
                if (w0 & 255) == R_SET_ENV {
                    let e = vm.get_env(w2);
                    let ov = vm.env_slot(e, w3);
                    extra = if let Some(o) = ov.as_obj() { format!("old obj rc{} zct{}/{}", vm.objs[o as usize].rc, vm.zct.len(), vm.zct.capacity()) } else if let Some(h) = ov.as_str() { format!("old str rc{}", if h > 0 { vm.strs[h as usize].rc } else { -9 }) } else { format!("old t{}", vm.type_index(&ov)) };
                }
                eprintln!("hopx {} t{} c{} {} {}", w0 & 255, tg, cl, nm, extra);
            }
        }
    }
    let nf = vm.frames.len();
    let fi = nf - 1;
    let rb = vm.protos[pi].rbase;
    let next = pc + 1 + (w0 >> 16);
    let sub = (w0 >> 8) & 255;
    let mut out = 0;
    match w0 & 255 {
        R_GET_GLOBAL => {
            let g = vm.global;
            let c = vm.protos[pi].code[pc as usize].c;
            let slot = vm.ic_slot(g, w3, c as int, false);
            if slot >= 0 {
                let v = vm.objs[g as usize].get_slot(slot as usize);
                vm.rset((bp + w1) as usize, v);
                cache_fill(vm, ri as usize, g, w3);
            } else {
                vm.r_step_push(pi, bp, fi, pc, w1);
            }
        }
        R_GET_ENV => {
            let e = vm.get_env(w2);
            let v = vm.env_slot(e, w3);
            vm.rset((bp + w1) as usize, v);
            if sub == 1 {
                vm.rset((bp + w1 + 1) as usize, Val::Undef());
            }
        }
        R_SET_ENV => {
            let e = vm.get_env(w2);
            let v = *vm.rget(bp, w1);
            vm.put_elem(e, w3, v);
        }
        R_THIS => {
            let t = vm.frames[fi].this_val;
            vm.rset((bp + w1) as usize, t);
        }
        R_TYPEOF => {
            let v = *vm.rget(bp, w2);
            let t = vm.type_val(&v);
            vm.rset((bp + w1) as usize, t);
        }
        R_CHECK => {
            let v = *vm.rget(bp, w1);
            let bad = if sub == 0 {
                if let Some(o) = v.as_obj() {
                    o == vm.tdz_obj
                } else {
                    false
                }
            } else {
                v.is_undef() || v.is_null()
            };
            if bad {
                vm.frames[fi].pc = pc + 1;
                vm.r_check_fail(pi, sub, w2);
            }
        }
        R_STEP_PUSH => vm.r_step_push(pi, bp, fi, pc, w1),
        R_STEP_K => {
            if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
                // debug: HCOUNT[1300 + the stack op]
                let k = (vm.protos[pi].code[pc as usize].code as usize).min(700);
                HCOUNT.with(|h| {
                    let mut t = h.get();
                    t[1300 + k] += 1;
                    h.set(t)
                });
            }
            vm.r_step_k(pi, bp, fi, pc, w0, w1, w2, w3)
        }
        R_STEP0 => {
            let op = vm.protos[pi].code[pc as usize];
            let mut p2 = pc + 1;
            vm.frames[fi].pc = p2;
            vm.step(op, pi, bp, fi, &mut p2);
        }
        R_GET_METHOD => {
            // a string's or number's method: its prototype's (prop_cache)
            let o = match (*vm.rget(bp, w2)).kind() {
                VK::Obj(o) => o,
                VK::Str(_) => vm.string_proto,
                VK::Num(_) => vm.number_proto,
                _ => -1,
            };
            vm.r_get_method(bp, w1, w2, w3, rb, fi, pc, w0);
            if !vm.throwing && vm.frames.len() == nf {
                let atom = vm.protos[pi].code[pc as usize].a as int;
                cache_fill(vm, ri as usize, o, atom);
            }
        }
        R_GET_METHOD_ELEM => vm.r_get_method_elem(bp, fi, pc, w1, w2, w3),
        R_ADD_ENV => vm.r_add_env(bp, fi, pc, w0, w1, w2, w3),
        R_NEW_OBJECT => {
            let o = vm.new_object();
            vm.rset((bp + w1) as usize, Val::Obj(o));
        }
        R_NEW_ARRAY => vm.r_new_array(bp, w1, w2),
        R_SET_GLOBAL => {
            // as rrun's R_SET_GLOBAL
            let g = vm.global;
            let c = vm.protos[pi].code[pc as usize].c;
            let v = *vm.rget(bp, w1);
            let slot = vm.ic_slot(g, w3, c as int, true);
            if slot >= 0 {
                vm.put_slot(g, slot, v);
            } else {
                vm.frames[fi].pc = pc + 1;
                vm.set_global(w3, v, sub, pi, pc + 1);
            }
        }
        R_CLOSURE => {
            let f = vm.closure(w2);
            vm.rset((bp + w1) as usize, Val::Obj(f));
        }
        R_INIT_PROP => {
            let v = *vm.rget(bp, w2);
            let o = crate::vm::obj_of(vm.rget(bp, w1));
            let sh = vm.objs[o as usize].shape as int;
            let c = vm.protos[pi].code[pc as usize].c;
            if c >= 0 && ((c >> 16) as int) == sh {
                vm.objs[o as usize].shape = c & 65535;
                let most = vm.shapes[vm.holder((c & 65535) as int)].keys.len();
                crate::vm::slot_room(&mut vm.objs[o as usize], most);
                vm.push_val(o, v);
            } else {
                vm.frames[fi].pc = pc + 1;
                vm.define(o, w3, v, 0);
                let sh2 = vm.objs[o as usize].shape as int;
                if sh2 != sh && sh < 32768 && sh2 < 65536 && !vm.shapes[sh as usize].owned && !vm.shapes[sh2 as usize].owned {
                    vm.protos[pi].code[pc as usize].c = (sh * 65536 + sh2) as i32;
                }
            }
        }
        R_GET_THIS_PROP => {
            let c = w3;
            let o = if let Some(x) = vm.frames[fi].this_val.as_obj() { x } else { return 1 };
            if c >= 65536 && ((c >> 16) as int) == vm.objs[o as usize].shape as int {
                let r = vm.objs[o as usize].get_slot((c & 65535) as usize);
                vm.rset((bp + w1) as usize, r);
            } else {
                return 1;
            }
        }
        R_WIN => {
            let want = (bp + vm.protos[pi].fsize) as usize;
            let p2 = vm.r_win(pi, bp, fi, pc, w1, want);
            if vm.throwing {
                return 4;
            }
            if p2 < 0 {
                return 5;
            }
            if p2 != next {
                set_cell_pc(p2);
                out = 3;
            }
        }
        R_ITER_NEXT => {
            // the next value into w3, or 7: the loop is done (go to w2)
            if !vm.r_iter_next(bp, fi, pc, w1, w3) && !vm.throwing && vm.frames.len() == nf {
                out = 7;
            }
        }
        _ => return 1,
    }
    if vm.throwing {
        if vm.frames.len() == nf {
            vm.frames[fi].pc = pc + 1;
        }
        return 4;
    }
    if vm.frames.len() != nf {
        return 5;
    }
    if out != 0 {
        return out;
    }
    after(vm)
}

/// R_CALL of anything but a JavaScript function (a native, a bound
/// function, a proxy): what r_call_slow does. A JavaScript function goes
/// back to the interpreter, which pushes its frame; the compiled code is
/// entered again when it returns (rvm.rs, R_RET).
fn h_call(vm: i32, bp: i32, _pi: i32, pc: i32, ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let ins = vm.rcode[ri as usize];
    let (w0, w1, w2, w3) = (ins.code as int, ins.a as int, ins.b as int, ins.c as int);
    let bp = bp as int;
    let pc = pc as int;
    if vm.gc_due() {
        return 1;
    }
    let fpos = bp + w1;
    let fo = crate::vm::obj_of(&vm.stack[fpos as usize]);
    let nf = vm.frames.len();
    let fi = nf - 1;
    let pi = vm.frames[fi].proto as usize;
    let want = (bp + vm.protos[pi].fsize) as usize;
    if (w0 & 255) as int == R_NEW {
        return h_new(vm, bp, fo, fpos, nf, pc, w0, w2, w3, want);
    }
    let tp = if fo >= 0 && vm.objs[fo as usize].class == C_FUNCTION { vm.objs[fo as usize].func as usize } else { usize::MAX };
    let plain = tp != usize::MAX && {
        let p = &vm.protos[tp];
        p.rest < 0 && !p.class_ctor && !p.generator && !p.is_async
    };
    if MASK.with(|m| m.get()) & (1u64 << 54) != 0 {
        // debug: OPCOUNT 220 native/other, 221 plain compiled, 222 plain
        // not compiled, 223 not plain
        let k = if tp == usize::MAX {
            if fo >= 0 && vm.objs[fo as usize].class == C_NATIVE {
                let nf = vm.objs[fo as usize].func as usize;
                if nf < 40 {
                    // SAFETY: on the VM's thread
                    unsafe {
                        (*std::ptr::addr_of_mut!(OPCOUNT))[230 + nf % 26] += 0;
                    }
                }
                NATS.with(|t| {
                    let mut v = t.take();
                    if v.len() <= nf {
                        v.resize(nf + 1, 0);
                    }
                    v[nf] += 1;
                    t.set(v);
                });
            }
            220
        } else if !plain {
            223
        } else if tp < vm.jit_fn.len() && vm.jit_fn[tp] >= 0 {
            let p = &vm.protos[tp];
            if p.arrow {
                224
            } else if p.env_size > 0 {
                225
            } else if tp * 8 + 1 < vm.jit_calls.len() && vm.jit_calls[tp * 8 + 1] < 0 {
                227
            } else if (vm.stack.capacity() as int) < bp + w1 + 2 + p.fsize {
                228
            } else if (vm.stack.len() as int) < bp + w1 + 2 + p.fsize {
                226
            } else {
                221
            }
        } else {
            222
        };
        // SAFETY: on the VM's thread
        unsafe {
            (*std::ptr::addr_of_mut!(OPCOUNT))[k] += 1;
        }
    }
    if tp == usize::MAX && fo >= 0 && vm.objs[fo as usize].class == C_NATIVE && MASK.with(|m| m.get()) & (1u64 << 47) == 0 {
        // natives of numbers and strings that cannot call back or throw:
        // answered off the stack as call_native does, without its frame
        // and argument work
        if let Some(v) = pure_native(vm, vm.objs[fo as usize].func as int, fpos, w2) {
            let next = pc + 1 + (w0 >> 16);
            vm.frames[fi].pc = next;
            let dst = if w3 > 0 { bp + w3 - 1 } else { fpos };
            vm.rset(dst as usize, v);
            return after(vm);
        }
    }
    if plain {
        // a plain function runs here, nested, and the compiled code goes
        // on after it; a generator, an async function, one that uses
        // arguments... gets its frame from r_call_slow below, and the
        // interpreter runs it
        // with slices on, or deep in nesting, the callee's frame is pushed
        // here and the interpreter runs it (5); the caller's code is
        // entered again at its return (rvm.rs, R_RET)
        let deep = no_nesting(vm);
        let next = pc + 1 + (w0 >> 16);
        vm.frames[fi].pc = next;
        if ((w0 >> 8) & 255) == 1 {
            vm.stack[(fpos + 1) as usize] = Val::Undef();
        }
        let dst = if w3 > 0 { bp + w3 - 1 } else { fpos };
        // compiled: the caller's code calls it itself (6), and h_ret ends
        // it; the stack past the arguments stays (renter clears the
        // callee's locals). Run here, its return leaves the value at the
        // stack's end, so the stack ends after the arguments.
        let f = if deep || MASK.with(|m| m.get()) & (1u64 << 60) != 0 || cell_get(9) >= DEPTH_MAX || vm.protos[tp].rstate == 0 { -1 } else { hot_fn(vm, tp) };
        if f < 0 && !deep {
            vm.stack.truncate((fpos + w2 + 2) as usize);
        }
        if !vm.renter(fo, fpos, w2, dst) {
            return settle(vm, fi, pc);
        }
        if f >= 0 {
            call_fill(vm, ri as usize, tp, f);
            return direct(vm, nf, f);
        }
        if deep {
            return 5;
        }
        NEST.with(|n| n.set(n.get() + 1));
        let done = call_compiled(vm, tp, nf, dst);
        if !done {
            vm.rrun(nf as int);
        }
        NEST.with(|n| n.set(n.get() - 1));
        if done {
            if vm.throwing {
                return 4;
            }
            vm.grow_stack(want);
            return after(vm);
        }
        if vm.throwing {
            return 4;
        }
        if vm.suspended || vm.frames.len() != nf {
            return 5;
        }
        // the callee's return left its value at fpos, the stack's end
        let v = vm.stack.pop().unwrap_or(Val::Undef());
        vm.grow_stack(want);
        vm.stack[dst as usize] = v;
        return after(vm);
    }
    vm.frames[fi].pc = pc + 1 + (w0 >> 16);
    if ((w0 >> 8) & 255) == 1 {
        vm.stack[(fpos + 1) as usize] = Val::Undef();
    }
    if vm.r_call_slow(fo, fpos, w2, bp, w3, want) {
        // one frame more, not a coroutine's (f.call(...), a function that
        // uses arguments...): it runs here, nested, as a plain call does
        let deep = no_nesting(vm);
        if deep || vm.throwing || vm.frames.len() != nf + 1 || MASK.with(|m| m.get()) & (1u64 << 56) != 0 {
            return 5;
        }
        let cp = &vm.protos[vm.frames[nf].proto as usize];
        if vm.frames[nf].gen >= 0 || cp.generator || cp.is_async || vm.frames[nf].construct {
            return 5;
        }
        let dst = if w3 > 0 { bp + w3 - 1 } else { fpos };
        NEST.with(|n| n.set(n.get() + 1));
        vm.rrun(nf as int);
        NEST.with(|n| n.set(n.get() - 1));
        if vm.throwing {
            return 4;
        }
        if vm.suspended || vm.frames.len() != nf {
            return 5;
        }
        let v = vm.stack.pop().unwrap_or(Val::Undef());
        vm.grow_stack(want);
        vm.stack[dst as usize] = v;
        return after(vm);
    }
    if vm.throwing {
        return 4;
    }
    if vm.frames.len() != nf {
        return 5;
    }
    after(vm)
}

// ---------------------------------------------------------- site caches
//
// A property or method site of compiled code has a cache of its own, which
// the code reads (prop_cache) and the helpers fill on a miss: CACHE_WAYS
// ways of CACHE_W words [shape, depth (0 own), slot * 16, epoch, class,
// then up to CACHE_DEPTH (prototype, its shape)], then the next way to
// replace. In Vm.jit_caches, whose buffers do not move, by rcode index.

const CACHE_WAYS: u32 = 4;
/// typeof's index (vm.rs type_index) of tag 0xFFF8 + t in nibble t:
/// number 3, undefined 0, null 1, bool 2, string 4
const TYPE_NIB: i64 = 0x42103;
/// a way's slot * 16 for a property absent from the whole chain
const SLOT_ABSENT: i32 = -(VS as i32);

/// undefined as a value's 16 bytes, for an absent property (look);
/// layout() writes it
static mut UNDEF16: [i64; 2] = [0, 0];
const CACHE_DEPTH: u32 = 4;
const CACHE_W: u32 = 5 + 2 * CACHE_DEPTH;
/// the ways, then the next way to replace
const CACHE_N: usize = (CACHE_WAYS * CACHE_W) as usize + 1;
/// words after a cache: a global site's value (4), a call's operands (9)
const CALL_REC: usize = 9;

/// The address of site ri's cache, made empty when it has none. The
/// caches are the Vm's (another machine's shapes are not these).
fn site_cache(vm: &mut crate::vm::Vm, ri: usize) -> u32 {
    while vm.jit_cache_at.len() <= ri {
        vm.jit_cache_at.push(-1);
    }
    if vm.jit_cache_at[ri] < 0 {
        // room for a global site's value (site_caches): the buffer stays
        let mut x: Vec<i32> = Vec::with_capacity(CACHE_N + CALL_REC);
        x.resize(CACHE_N, -1);
        x[CACHE_N - 1] = 0;
        vm.jit_cache_at[ri] = vm.jit_caches.len() as int;
        vm.jit_caches.push(x);
    }
    vm.jit_caches[vm.jit_cache_at[ri] as usize].as_ptr() as usize as u32
}

/// After a helper read property `atom` of o at site ri: the way that
/// answers it, an own data property or one on o's prototype, into the
/// site's cache (when it has one).
fn cache_fill(vm: &mut crate::vm::Vm, ri: usize, o: int, atom: int) {
    if o < 0 || atom < 0 || ri >= vm.jit_cache_at.len() || vm.jit_cache_at[ri] < 0 {
        return;
    }
    let ob = &vm.objs[o as usize];
    let cls = ob.class;
    // String.prototype (a primitive method's holder) answers but for its
    // length as a plain object: its string is ""
    let sp = o == vm.string_proto && atom != A_LENGTH;
    if cls == C_PROXY || (cls == C_STRING && !sp) {
        return;
    }
    let w = CACHE_W as usize;
    let mut way = vec![-1i32; w];
    way[0] = ob.shape;
    way[1] = 0;
    way[3] = vm.shape_epoch as i32;
    way[4] = cls;
    // an object answers only from its shape (rvm.rs pic_plain): any
    // plain object; an array or function but for the properties it may
    // answer without them (length, name, prototype)
    let plain = |c: i32| c == C_OBJECT || (c != C_PROXY && c != C_STRING && atom != A_LENGTH && atom != A_NAME && atom != A_PROTOTYPE);
    let own = vm.find(o, atom);
    if own >= 0 {
        if (vm.attr_at(o, own) & P_ACCESSOR) != 0 {
            return;
        }
        way[2] = (own * VS as i64) as i32;
    } else {
        // a dictionary receiver gains keys without an epoch (add_own)
        if (!plain(cls) && !sp) || vm.shapes[ob.shape as usize].owned {
            return;
        }
        let mut cur = o;
        let mut d: usize = 0;
        loop {
            let p = vm.objs[cur as usize].proto as int;
            if p < 0 && plain(cls) && cls != C_NATIVE && plain(vm.objs[cur as usize].class) && (vm.rcode[ri].code & 255) as int != R_GET_GLOBAL {
                // absent from a plain object and its plain prototypes:
                // undefined while the chain keeps its shapes (look
                // checks that the last one still has no prototype)
                way[1] = d as i32;
                way[2] = SLOT_ABSENT;
                break;
            }
            if p < 0 || d >= CACHE_DEPTH as usize {
                return;
            }
            let pc = vm.objs[p as usize].class;
            if !plain(pc) && pc != C_NATIVE {
                return;
            }
            way[5 + 2 * d] = p as i32;
            way[6 + 2 * d] = vm.objs[p as usize].shape;
            d += 1;
            let k = vm.find(p, atom);
            if k >= 0 {
                if (vm.attr_at(p, k) & P_ACCESSOR) != 0 {
                    return;
                }
                way[1] = d as i32;
                way[2] = (k * VS as i64) as i32;
                break;
            }
            if !plain(pc) {
                return;
            }
            cur = p;
        }
    }
    // shared shapes never change or go (an owned one's id can come back
    // as another shape, which moves the epoch): no epoch to check
    let mut owned = vm.shapes[way[0] as usize].owned;
    for d in 0..way[1].max(0) as usize {
        owned = owned || vm.shapes[way[6 + 2 * d] as usize].owned;
    }
    if !owned {
        way[3] = -1;
    }
    if MASK.with(|m| m.get()) & (1u64 << 59) != 0 {
        OPN.with(|n| n.set(n.get() + 1));
        if OPN.with(|n| n.get()) % 64 == 0 {
            eprintln!("fill {} op {} way {:?}", ri, vm.rcode[ri].code & 255, way);
        }
    }
    let b = &mut vm.jit_caches[vm.jit_cache_at[ri] as usize];
    let nw = CACHE_WAYS as usize;
    let mut at = nw;
    for i in 0..nw {
        if b[i * w] == way[0] && b[i * w + 4] == way[4] && b[i * w + 5] == way[5] {
            at = i;
        }
    }
    if at == nw {
        at = (b[nw * w] as usize) % nw;
        b[nw * w] = ((at + 1) % nw) as i32;
    }
    for k in 0..w {
        b[at * w + k] = way[k];
    }
}

/// By pc of a function's code (from rb, n ops): the cache of each property
/// and method site prop_cache reads, 0 for none.
/// By pc of the function at rb (n ops): the callee a plain call's site
/// has always called (call_fill's cache) when inline_ok can write its body
/// there.
fn inline_sites(vm: &mut crate::vm::Vm, pi: usize, rb: usize, n: usize) -> Vec<Option<Inl>> {
    let mut out: Vec<Option<Inl>> = vec![None; n];
    for i in 0..n {
        if rb + i >= vm.rcode.len() || (vm.rcode[rb + i].code & 255) as int != R_CALL || rb + i >= vm.jit_cache_at.len() || vm.jit_cache_at[rb + i] < 0 {
            continue;
        }
        let mut tp = vm.jit_caches[vm.jit_cache_at[rb + i] as usize][0];
        if tp < 0 {
            // not called from compiled code yet: the function object the
            // last call left in the slot of f of the function's running
            // frame (a guess; inline_call checks it every time)
            tp = guess_callee(vm, pi, rb, i);
        }
        if tp < 0 || tp as usize >= vm.protos.len() {
            continue;
        }
        let q = &vm.protos[tp as usize];
        if q.rstate != 1 || q.uses_args || q.rest >= 0 || q.class_ctor || q.generator || q.is_async || q.arrow || q.env_size > 0 {
            continue;
        }
        let qb = q.rbase as usize;
        let qn = q.code.len();
        if qb + qn > vm.rcode.len() {
            continue;
        }
        let fwd = match inline_ok(&vm.rcode[qb..qb + qn], q.rentry as usize, q.nparams as u32) {
            Some(f) => f,
            None => continue,
        };
        let (entry, nparams, nslots, fsize, strict) = (q.rentry as usize, q.nparams as u32, q.nslots as u32, q.fsize as u32, q.strict);
        let caches = site_caches(vm, tp as usize, qb, qn);
        let atoms = site_atoms(vm, tp as usize, qb, qn);
        out[i] = Some(Inl {
            tp: tp as u32,
            code: vm.rcode[qb..qb + qn].to_vec(),
            entry,
            nparams,
            nslots,
            fsize,
            rb: qb as u32,
            caches,
            atoms,
            strict,
            fwd,
        });
    }
    out
}

/// By pc of function pi (at rb, n ops): a property write's atom; R_INIT_PROP's
/// transition cache; -1 for anything else.
fn site_atoms(vm: &mut crate::vm::Vm, pi: usize, rb: usize, n: usize) -> Vec<i32> {
    (0..n)
        .map(|i| {
            let k = if rb + i < vm.rcode.len() { (vm.rcode[rb + i].code & 255) as int } else { -1 };
            if k == R_SET_PROP {
                vm.ratom(pi, i as int) as i32
            } else if k == R_INIT_PROP {
                vm.protos[pi].code[i].c
            } else {
                -1
            }
        })
        .collect()
}

/// The proto of the script function a call (pc of the function at rb) is
/// likely to call (guess_reg of its register f), -1 when none is known.
/// Only a guess: inline_call checks the function object every time.
fn guess_callee(vm: &crate::vm::Vm, pi: usize, rb: usize, pc: usize) -> i32 {
    if let Some(o) = (guess_reg(vm, pi, rb, pc, vm.rcode[rb + pc].a, 2)).as_obj() {
        if o >= 0 && (o as usize) < vm.objs.len() && vm.objs[o as usize].class == C_FUNCTION {
            return vm.objs[o as usize].func;
        }
    }
    -1
}

/// The value register r likely holds at pc of the function at rb, from
/// the op before it that loaded r: a global's or a scope slot's value
/// now, a local of the function's newest frame, or a method (by the
/// receiver's guess and the site's cache, or its first prototype way).
fn guess_reg(vm: &crate::vm::Vm, pi: usize, rb: usize, pc: usize, r: i32, depth: u32) -> Val {
    if r < 0 {
        return Val::Undef();
    }
    let frame = (0..vm.frames.len()).rev().find(|&k| vm.frames[k].proto as usize == pi);
    let local = |reg: i32| -> Val {
        match frame {
            Some(fi) if reg >= 0 && (reg as int) < vm.protos[pi].nslots => {
                let at = (vm.frames[fi].bp + reg as int) as usize;
                if at < vm.stack.len() { vm.stack[at] } else { Val::Undef() }
            }
            _ => Val::Undef(),
        }
    };
    if (r as int) < vm.protos[pi].nslots {
        // a local: its value in the newest frame
        return local(r);
    }
    let mut k = pc;
    let mut steps = 0;
    while k > 0 && steps < 32 {
        k -= 1;
        steps += 1;
        let op = vm.rcode[rb + k];
        let kind = (op.code & 255) as int;
        if kind == R_NOP || op.a != r {
            continue;
        }
        return match kind {
            R_GET_GLOBAL => {
                let g = vm.global;
                let slot = vm.find(g, op.c as int);
                if slot >= 0 { vm.objs[g as usize].get_slot(slot as usize) } else { Val::Undef() }
            }
            R_GET_ENV => match frame {
                Some(fi) => {
                    let mut e = vm.frames[fi].env;
                    let mut d = op.b;
                    while d > 0 && e >= 0 {
                        e = vm.objs[e as usize].proto as int;
                        d -= 1;
                    }
                    let ev = if e >= 0 { vm.objs[e as usize].elems as usize } else { 0 };
                    if e >= 0 && ev < vm.evecs.len() && (op.c as usize) < vm.evecs[ev].v.len() { vm.evecs[ev].v[op.c as usize] } else { Val::Undef() }
                }
                None => Val::Undef(),
            },
            R_MOVE if op.b >= 0 => local(op.b),
            R_GET_METHOD if depth > 0 => {
                // an own method: the receiver's guess and the site's cache
                if op.c >= 65536 {
                    if let Some(o) = (guess_reg(vm, pi, rb, k, op.b, depth - 1)).as_obj() {
                        if o >= 0 && (o as usize) < vm.objs.len() && vm.objs[o as usize].shape == (op.c >> 16) {
                            return vm.objs[o as usize].get_slot((op.c & 65535) as usize);
                        }
                    }
                }
                // the site's first way: a data method some prototypes up
                // (rvm.rs PIC_W: depth, slot, accessor, then each
                // prototype and its shape)
                let site = rb + k;
                if site < vm.rpic.len() && vm.rpic[site] >= 0 {
                    let b = (vm.rpic[site] * (crate::rvm::PIC_W as i32)) as usize;
                    let d = if b + 4 < vm.pic.len() { vm.pic[b + 2] } else { 0 };
                    if d >= 1 && d <= crate::rvm::PIC_DEPTH as i32 && b + 6 + 2 * (d as usize) < vm.pic.len() && vm.pic[b + 3] >= 0 && vm.pic[b + 4] == 0 {
                        let h = vm.pic[b + 5 + 2 * (d as usize - 1)];
                        if h >= 0 && (h as usize) < vm.objs.len() {
                            return vm.objs[h as usize].get_slot(vm.pic[b + 3] as usize);
                        }
                    }
                }
                Val::Undef()
            }
            _ => Val::Undef(),
        };
    }
    Val::Undef()
}

/// Cg.pics: by pc * 4, the first way of an R_GET_METHOD's or R_GET_PROP's
/// interpreter cache (rvm.rs PIC_W) that finds a data property on the
/// receiver's prototype, when neither shape is a dictionary's (shapes that
/// are not owned never change, so the code needs no epoch check)
fn site_pics(vm: &crate::vm::Vm, rb: usize, n: usize) -> Vec<i32> {
    let mut out = vec![-1i32; n * 4];
    let w = crate::rvm::PIC_W as usize;
    for i in 0..n {
        let k = (vm.rcode[rb + i].code & 255) as int;
        if k != R_GET_METHOD && k != R_GET_PROP {
            continue;
        }
        let site = rb + i;
        if site >= vm.rpic.len() || vm.rpic[site] < 0 {
            continue;
        }
        let mut way = 0;
        while way < crate::rvm::PIC_WAYS as usize {
            let b = (vm.rpic[site] as usize + way) * w;
            way += 1;
            if b + 7 > vm.pic.len() || vm.pic[b + 1] != vm.chain_epoch as i32 || vm.pic[b + 2] != 1 || vm.pic[b + 3] < 0 || vm.pic[b + 4] != 0 {
                continue;
            }
            let (sh, p, ps) = (vm.pic[b], vm.pic[b + 5], vm.pic[b + 6]);
            if sh < 0 || p < 0 || ps < 0 || sh as usize >= vm.shapes.len() || ps as usize >= vm.shapes.len() || vm.shapes[sh as usize].owned || vm.shapes[ps as usize].owned {
                continue;
            }
            out[i * 4] = sh;
            out[i * 4 + 1] = p;
            out[i * 4 + 2] = ps;
            out[i * 4 + 3] = vm.pic[b + 3];
            break;
        }
    }
    out
}

fn site_caches(vm: &mut crate::vm::Vm, pi: usize, rb: usize, n: usize) -> Vec<u32> {
    let mut out = vec![0u32; n];
    for i in 0..n {
        let k = (vm.rcode[rb + i].code & 255) as int;
        let _ = pi;
        if k == R_CALL {
            // the cache, then the call's record (call_op writes it)
            site_cache(vm, rb + i);
            let at = vm.jit_cache_at[rb + i] as usize;
            if vm.jit_caches[at].len() == CACHE_N {
                vm.jit_caches[at].resize(CACHE_N + CALL_REC, 0);
            }
            out[i] = vm.jit_caches[at].as_ptr() as usize as u32;
        } else if k == R_GET_PROP || k == R_GET_THIS_PROP || k == R_GET_METHOD || k == R_SET_PROP {
            out[i] = site_cache(vm, rb + i);
        } else if k == R_GET_GLOBAL {
            // after the cache, the value Obj(global) for prop_cache to read
            let a = site_cache(vm, rb + i);
            let at = vm.jit_cache_at[rb + i] as usize;
            if vm.jit_caches[at].len() == CACHE_N {
                let g = bytes_of(Val::Obj(vm.global));
                for k in 0..4 {
                    vm.jit_caches[at].push(i32::from_le_bytes([g[4 * k], g[4 * k + 1], g[4 * k + 2], g[4 * k + 3]]));
                }
            }
            // the push may have moved the buffer
            let _ = a;
            out[i] = vm.jit_caches[at].as_ptr() as usize as u32;
        }
    }
    out
}

/// The callee's frame (pushed by renter, frames[nf]) runs its compiled
/// code when it has some or is called often enough. true when it returned
/// (its value in dst, its frame gone) or threw past it; false when the
/// interpreter is to run the frame on from its pc.
/// R_NEW of a plain JS constructor (rvm.rs R_NEW): this after f, the args
/// one slot up, the frame a construct; then as h_call. Anything else is
/// the interpreter's.
fn h_new(vm: &mut crate::vm::Vm, bp: int, fo: int, fpos: int, nf: usize, pc: int, w0: int, argc: int, w3: int, want: usize) -> i32 {
    let fi = nf - 1;
    if fo < 0 || vm.objs[fo as usize].class != C_FUNCTION || no_nesting(vm) {
        return new_win(vm, bp, fi, pc, w0, w3, want);
    }
    // the constructor to enter: a class with the default derived
    // constructor gives its parent (Vm::ctor_target)
    let f0 = fo;
    let fo = vm.ctor_target(fo);
    let tp = vm.objs[fo as usize].func as usize;
    // a base class's constructor too: its fields are set as enter_function
    // sets them
    let (plain, cls) = {
        let p = &vm.protos[tp];
        (p.rest < 0 && (!p.class_ctor || !p.derived) && (p.class_ctor || p.constructible()), p.class_ctor)
    };
    if !plain {
        return new_win(vm, bp, fi, pc, w0, w3, want);
    }
    let _ = bp;
    vm.frames[fi].pc = pc + 1 + (w0 >> 16);
    let top = (fpos + argc + 2) as usize;
    if vm.stack.len() < top {
        vm.grow_stack(top);
    }
    let mut k = argc;
    while k > 0 {
        vm.stack[(fpos + 1 + k) as usize] = vm.stack[(fpos + k) as usize];
        k -= 1;
    }
    let f = vm.stack[fpos as usize];
    let this = vm.make_this(&f);
    vm.stack[(fpos + 1) as usize] = this;
    // a forwarder's m in its place (Vm::fwd_target)
    let m = if !cls && vm.protos[tp].fwd_atom >= 0 { vm.fwd_target(tp, &this) } else { -1 };
    let tp = if m >= 0 {
        vm.stack[fpos as usize] = Val::Obj(m);
        vm.objs[m as usize].func as usize
    } else {
        if fo != f0 {
            vm.stack[fpos as usize] = Val::Obj(fo);
        }
        tp
    };
    if !vm.renter(if m >= 0 { m } else { fo }, fpos, argc, fpos) {
        return settle(vm, fi, pc);
    }
    vm.frames[nf].construct = true;
    vm.frames[nf].new_target = if m >= 0 { Val::Undef() } else { f };
    if cls {
        vm.run_fields(fo, &this);
        if vm.throwing {
            return 4;
        }
        if vm.frames.len() != nf + 1 {
            return 5;
        }
    }
    let fc = if cell_get(9) >= DEPTH_MAX { -1 } else { hot_fn(vm, tp) };
    if fc >= 0 {
        return direct(vm, nf, fc);
    }
    NEST.with(|n| n.set(n.get() + 1));
    vm.rrun(nf as int);
    NEST.with(|n| n.set(n.get() - 1));
    if vm.throwing {
        return 4;
    }
    if vm.suspended || vm.frames.len() != nf {
        return 5;
    }
    // the constructor's return left the object at fpos, the stack's end
    let v = vm.stack.pop().unwrap_or(Val::Undef());
    vm.grow_stack(want);
    vm.stack[fpos as usize] = v;
    after(vm)
}

/// After a write to own property `atom` of o at site ri: an own way for
/// set_prop_cached when it is a writable data property of a plain object.
fn set_cache_fill(vm: &mut crate::vm::Vm, ri: usize, o: int, atom: int) {
    if atom < 0 || ri >= vm.jit_cache_at.len() || vm.jit_cache_at[ri] < 0 {
        return;
    }
    if vm.objs[o as usize].class != C_OBJECT {
        return;
    }
    let k = vm.find(o, atom);
    if k < 0 || (vm.attr_at(o, k) & (P_ACCESSOR | P_READONLY)) != 0 {
        return;
    }
    cache_fill(vm, ri, o, atom);
}

/// A plain call at site ri went to compiled function f of proto tp: its
/// call cache (call_push) names it, unless renter would do more than
/// call_push does (an arrow's this, a scope to make).
/// Vm.jit_calls for proto tp, compiled as table index f (callx)
fn calls_fill(vm: &mut crate::vm::Vm, tp: usize, f: i64) {
    let n = vm.protos.len().max(tp + 1) * 8;
    if vm.jit_calls.len() < n {
        vm.jit_calls.resize(n, -1);
    }
    let p = &vm.protos[tp];
    let arrow = p.arrow && MASK.with(|m| m.get()) & (1u64 << 48) == 0;
    let ok = f >= 0 && (!p.arrow || arrow) && p.env_size == 0 && !p.uses_args && p.rest < 0 && !p.class_ctor && !p.generator && !p.is_async;
    // word 6: the callee takes this as it comes (strict, or an arrow,
    // whose this is its own: word 7)
    let w = [tp as i32, if ok { f as i32 } else { -1 }, p.nparams as i32, p.nslots as i32, p.fsize as i32, p.rentry as i32, if p.strict || p.arrow { 1 } else { 0 }, if p.arrow { 1 } else { 0 }];
    for k in 0..8 {
        vm.jit_calls[tp * 8 + k] = w[k];
    }
}

fn call_fill(vm: &mut crate::vm::Vm, ri: usize, tp: usize, f: i64) {
    if ri >= vm.jit_cache_at.len() || vm.jit_cache_at[ri] < 0 {
        return;
    }
    let p = &vm.protos[tp];
    if p.arrow || p.env_size > 0 || p.uses_args {
        return;
    }
    let w = [tp as i32, f as i32, p.nparams as i32, p.nslots as i32, p.fsize as i32, p.rentry as i32, if p.strict { 1 } else { 0 }];
    let b = &mut vm.jit_caches[vm.jit_cache_at[ri] as usize];
    for k in 0..7 {
        b[k] = w[k];
    }
}

/// R_NEW as the interpreter does it for anything but a plain constructor
/// (its window op, which pushes the frame of a script function: 5).
fn new_win(vm: &mut crate::vm::Vm, bp: int, fi: usize, pc: int, w0: int, w3: int, want: usize) -> i32 {
    let pi = vm.frames[fi].proto as usize;
    let next = pc + 1 + (w0 >> 16);
    let p2 = vm.r_win(pi, bp, fi, pc, w3, want);
    if vm.throwing {
        if vm.frames.len() == fi + 1 {
            vm.frames[fi].pc = pc + 1;
        }
        return 4;
    }
    if p2 < 0 || vm.frames.len() != fi + 1 {
        return 5;
    }
    if p2 != next {
        set_cell_pc(p2);
        return 3;
    }
    after(vm)
}

/// The compiled callee f of the frame renter pushed (frames[nf]): the
/// caller's code calls it (6), with its entry, table index, bp and sp in
/// the cell.
fn direct(vm: &mut crate::vm::Vm, nf: usize, f: i64) -> i32 {
    let cbp = vm.frames[nf].bp;
    let entry = vm.frames[nf].pc;
    depth_add(1);
    // SAFETY: as fill_cell
    unsafe {
        let c = &mut *std::ptr::addr_of_mut!(CELL);
        c[2] = entry as i32;
        c[7] = f as i32;
        c[8] = cbp as i32;
        c[10] = (vm.stack.as_ptr() as usize + (cbp as usize) * VSU) as i32;
    }
    6
}

/// Function tp's compiled code when it has some or has been called often
/// enough to get it, else -1.
pub fn hot_fn(vm: &mut crate::vm::Vm, tp: usize) -> i64 {
    let f = if tp < vm.jit_fn.len() { vm.jit_fn[tp] } else { -1 };
    if f == PENDING {
        let f = compiled_fn(vm, tp);
        return if f >= 0 { f } else { -1 };
    }
    if f >= 0 || f == -2 {
        if f >= 0 {
            let ri = (vm.protos[tp].rbase + vm.protos[tp].rentry) as usize;
            if ri < vm.jit_miss.len() && vm.jit_miss[ri] > crate::rvm::JIT_MISS_MAX {
                return -1;
            }
        }
        return if f >= 0 { f } else { -1 };
    }
    while vm.jit_hot.len() <= tp {
        vm.jit_hot.push(0);
    }
    if vm.jit_hot[tp] < vm.jit_min {
        vm.jit_hot[tp] += 1;
        return -1;
    }
    let f = compiled_fn(vm, tp);
    if f >= 0 {
        f
    } else {
        -1
    }
}

/// After the compiled code called a compiled callee itself (h_call's 6):
/// r is what the callee returned. Its return op is done here (the value
/// into the frame's ret slot, the frame gone); anything else leaves the
/// callee's frame to the interpreter.
fn h_ret(vm: i32, bp: i32, r: i32, _pc: i32, _ri: i32) -> i32 {
    let vm = unsafe { vm_of(vm) };
    let _ = bp;
    depth_add(-1);
    let r = if r == OUT_RET { cell_get(2) } else { r };
    if r == OUT_THROW {
        return 4;
    }
    if r == OUT_FRAMES {
        return 5;
    }
    let at = if r >= 0 { (r & !(3 << 28)) as int } else { (-1 - r) as int };
    let top = vm.frames.len() - 1;
    let tp = vm.frames[top].proto as usize;
    let rb = vm.protos[tp].rbase;
    let ins = vm.rcode[(rb + at) as usize];
    let k = (ins.code & 255) as int;
    let cbp = vm.frames[top].bp;
    let mut v = if k == R_RET { *vm.rget(cbp, ins.a as int) } else { Val::Undef() };
    let mut ok = r >= 0 && (k == R_RET || k == R_RET_UNDEF);
    if ok && vm.frames[top].construct && !crate::vm::is_obj(&v) {
        // a constructor's this, unless a derived one returned a value
        if !v.is_undef() && vm.protos[tp].derived {
            ok = false;
        } else {
            v = vm.frames[top].this_val;
        }
    }
    if ok {
        let fr = vm.frames.pop().unwrap();
        while !vm.handlers.is_empty() && vm.handlers[vm.handlers.len() - 1].frame >= top as int {
            vm.handlers.pop();
        }
        let to = if fr.ret >= 0 { fr.ret as usize } else { (fr.bp - 2) as usize };
        vm.stack[to] = v;
        // the stack as long as the caller's frame again (h_call cut it at
        // the call's arguments)
        if let Some(c) = vm.frames.last() {
            let want = (c.bp + vm.protos[c.proto as usize].fsize) as usize;
            vm.grow_stack(want);
        }
        return after(vm);
    }
    vm.frames[top].pc = at;
    // a callee that does not get to its return here is called by the
    // interpreter after enough of those (hot_fn)
    let ri = (rb + vm.protos[tp].rentry) as usize;
    while vm.jit_miss.len() <= ri {
        vm.jit_miss.push(0);
    }
    // left just after a call of its own (a callee the interpreter runs):
    // its code goes on there when that returns (jit_res), not a miss
    let resumes = ((rb + at) as usize) < vm.jit_res.len() && vm.jit_res[(rb + at) as usize];
    vm.jit_miss[ri] += if resumes { 0 } else { 4 };
    if vm.jit_miss[ri] > crate::rvm::JIT_MISS_MAX && tp * 8 + 1 < vm.jit_calls.len() {
        // callx calls it no more either
        vm.jit_calls[tp * 8 + 1] = -1;
    }
    if MASK.with(|m| m.get()) & (1u64 << 54) != 0 && k < 100 {
        // debug: OPCOUNT[150 + the op kind a callee left at]
        // SAFETY: on the VM's thread
        unsafe {
            (*std::ptr::addr_of_mut!(OPCOUNT))[(150 + k.min(105)) as usize] += 1;
        }
    }
    if MASK.with(|m| m.get()) & (1u64 << 62) != 0 && vm.jit_miss[ri] == crate::rvm::JIT_MISS_MAX + 4 {
        eprintln!("callee {} {} left at pc {} r {} op {} sub {} stack op {}", tp, vm.protos[tp].name, at, r, k, (ins.code >> 8) & 255, vm.protos[tp].code[at as usize].code);
    }
    5
}

fn call_compiled(vm: &mut crate::vm::Vm, tp: usize, nf: usize, dst: int) -> bool {
    while vm.jit_hot.len() <= tp {
        vm.jit_hot.push(0);
    }
    let f = if tp < vm.jit_fn.len() { vm.jit_fn[tp] } else { -1 };
    if f == -2 {
        return false;
    }
    if f == -1 {
        if vm.jit_hot[tp] < vm.jit_min {
            vm.jit_hot[tp] += 1;
            return false;
        }
    }
    let bp = vm.frames[nf].bp;
    let pc = vm.frames[nf].pc;
    let r = enter(vm, tp, bp, pc, if vm.checking { crate::vm::SLICE_TICKS as i32 } else { 1 << 30 }, false);
    if r == -2 {
        // threw: a handler in the callee goes on there
        if vm.unwind(nf as int) {
            return false;
        }
        return true;
    }
    if r == -3 || vm.frames.len() != nf + 1 {
        return false;
    }
    let at = if r >= 0 { r } else { -4 - r };
    let rb = vm.protos[tp].rbase;
    let ins = vm.rcode[(rb + at) as usize];
    let k = (ins.code & 255) as int;
    if (k == R_RET || k == R_RET_UNDEF) && !vm.frames[nf].construct {
        let v = if k == R_RET { *vm.rget(bp, ins.a as int) } else { Val::Undef() };
        vm.frames.pop();
        while !vm.handlers.is_empty() && vm.handlers[vm.handlers.len() - 1].frame >= nf as int {
            vm.handlers.pop();
        }
        vm.stack[dst as usize] = v;
        return true;
    }
    vm.frames[nf].pc = at;
    false
}

/// how deep plain calls from compiled code run nested (h_call): each one
/// is a native rrun on the wasm stack, so past NEST_MAX the interpreter
/// makes the call
const NEST_MAX: u32 = 48;
thread_local! {
    static OPN: std::cell::Cell<u64> = std::cell::Cell::new(0);
}
/// compiled code calling compiled code nests wasm frames too (call_op)
const DEPTH_MAX: i32 = 400;

/// Whether a call from compiled code must not run nested (a native rrun
/// on the stack): slices on, NEST_MAX nested runs, or half of the native
/// stack's budget used. Each nested run takes several KB of the shadow
/// stack (rrun's frame, h_call, call_compiled, jit_enter), and in
/// WebAssembly the budget is 128 KB: 16 levels with compiled code between
/// them used it up in Octane's EarleyBoyer, and the next rrun threw
/// RangeError at 185 script frames. Past it the interpreter makes the
/// call, which grows no native stack.
#[inline(always)]
fn no_nesting(vm: &crate::vm::Vm) -> bool {
    vm.slicing || NEST.with(|n| n.get()) >= NEST_MAX || (vm.stack_base != 0 && vm.stack_base - crate::vm::native_sp() > vm.stack_budget / 2)
}

thread_local! {
    static NEST: std::cell::Cell<u32> = std::cell::Cell::new(0);
    /// op kinds written as exits whatever they are (cerxes_jit_mask), to
    /// find which op's code goes wrong
    static MASK: std::cell::Cell<u64> = std::cell::Cell::new(0);
}

#[no_mangle]
pub extern "C" fn cerxes_jit_mask(lo: i32, hi: i32) {
    MASK.with(|m| m.set((lo as u32 as u64) | ((hi as u32 as u64) << 32)));
}

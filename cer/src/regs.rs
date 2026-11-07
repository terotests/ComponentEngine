// SPDX-License-Identifier: AGPL-3.0-or-later
//! Register code for a function, made from its stack bytecode when the
//! function first runs (rvm.rs runs it).
//!
//! A frame is a fixed window of the stack: slots 0..nslots are the locals,
//! slot nslots + d holds what the stack bytecode keeps at depth d. Every op
//! keeps its index, so a frame's pc, a handler's catch_pc, a generator's
//! saved pc, the inline caches (`code[i].c`) and the jump targets mean the
//! same in both codes.
//!
//! An instruction is an Op in `rcode` (one bounds check and one load per
//! dispatch): `code` is `op + 256 * sub + 65536 * skip`, a b c the three
//! operands. `skip` is how many instructions after this one do
//! nothing (R_NOP), so falling through steps over them. An operand register
//! x >= 0 is slot x of the frame, x < 0 is `kvals[-1 - x]`.
//!
//! The translation follows the stack bytecode with an abstract stack. A
//! value pushed by a load (a local, a constant, a DUP) is not copied at
//! once: the op that uses it reads its source directly, and the load does
//! nothing. Only when the value must be in its own slot (at a jump, a join,
//! before an op this file does not translate, the operands of a call, or
//! before its source local is written) is the load rewritten into R_MOVE.
//! A result stored into a local right away is written there by the op that
//! makes it. Ops without a register form run their stack handler on the
//! frame's window (R_WIN).

use ranger::prelude::*;
use crate::ops::*;
use crate::value::*;

pub const R_NOP: int = 0;
/// w1 the window's top: the stack is cut there, the stack op runs, the
/// frame is padded back
pub const R_WIN: int = 1;
/// an op that neither reads nor writes the stack
pub const R_STEP0: int = 2;
/// w1 dst: an op that only pushes, its value moved to dst
pub const R_STEP_PUSH: int = 3;
/// w1 dst, w2 src
pub const R_MOVE: int = 4;
/// w1 dst, w2 src, w3 src of dst + 1
pub const R_LOAD2: int = 5;
/// w1 dst, w2 obj, w3 atom
pub const R_GET_PROP: int = 6;
/// w1 dst, w3 atom
pub const R_GET_THIS_PROP: int = 7;
/// w1 obj, w2 value, w3 atom
pub const R_SET_PROP: int = 8;
/// w1 dst, w2 obj, w3 key
pub const R_GET_ELEM: int = 9;
/// w1 obj, w2 key, w3 value; sub 1: the value also into w1 (the
/// assignment's result)
pub const R_SET_ELEM: int = 10;
/// w1 dst, w2 x, w3 y
pub const R_ADD: int = 11;
pub const R_SUB: int = 12;
pub const R_MUL: int = 13;
/// sub the OP_ code
pub const R_ARITH: int = 14;
pub const R_CMP: int = 15;
/// w1 x, w2 y, w3 target, sub the comparison
pub const R_CMP_JF: int = 16;
/// w1 target
pub const R_JUMP: int = 17;
/// w1 x, w2 target
pub const R_JF: int = 18;
pub const R_JT: int = 19;
pub const R_JF_KEEP: int = 20;
pub const R_JT_KEEP: int = 21;
pub const R_JNN_KEEP: int = 22;
/// w1 x
pub const R_RET: int = 23;
pub const R_RET_UNDEF: int = 24;
/// w1 the slot of f (this and the arguments after it), w2 argc
pub const R_CALL: int = 25;
/// w1 dst (f; the receiver in dst + 1), w2 obj, w3 atom, sub OP_GET_METHOD's b
pub const R_GET_METHOD: int = 26;
/// w1 dst
pub const R_THIS: int = 27;
/// w1 dst, w2 depth, w3 slot; sub 1: undefined in dst + 1 too
pub const R_GET_ENV: int = 28;
/// w1 value, w2 depth, w3 slot
pub const R_SET_ENV: int = 29;
/// w1 dst, w3 atom
pub const R_GET_GLOBAL: int = 30;
/// w1 dst, w2 x
pub const R_NOT: int = 31;
/// w1 dst
pub const R_NEW_OBJECT: int = 32;
/// w1 obj, w2 value, w3 atom
pub const R_INIT_PROP: int = 33;
/// w1 dst, w2 proto
pub const R_CLOSURE: int = 34;
/// w1 local, w2 delta
pub const R_INC_LOCAL: int = 35;
/// w1 dst, w2 local, w3 delta; sub 1: the new value (++x)
pub const R_POSTINC: int = 36;
/// w1 local, w2 value
pub const R_ADD_LOCAL_POP: int = 37;
/// w1 local, w2 comparison, w3 target
pub const R_LOCAL_NULL_JF: int = 38;
/// w1 x, w2 k, w3 target
pub const R_CASE_INT_JT: int = 39;
/// w1 x, w2 y, w3 target
pub const R_CASE_JT: int = 40;
/// a loop's back edge: a jump to an R_CMP_JF done here; w1 x, w2 y, w3 the
/// exit, sub the comparison, and the skip field holds the body's start
pub const R_LOOP: int = 41;
/// a comparison and a jump when it holds; w1 x, w2 y, w3 target, sub the
/// comparison (R_CMP_JF jumps when it does not)
pub const R_CMP_JT: int = 42;
/// a comparison and a jump that keeps its value: w1 dst + 65536 * target,
/// w2 x, w3 y, sub the comparison; JFK jumps when false, JTK when true, and
/// the next op is the one after the R_JF_KEEP / R_JT_KEEP it stands for
pub const R_CMP_JFK: int = 43;
pub const R_CMP_JTK: int = 44;
/// a run of R_CASE_INT_JT on one value as a table: w1 x, w2 where the
/// table is in Vm.rtab (its size n, n targets, the default), w3 the key of
/// the table's first target
pub const R_SWITCH: int = 45;
/// w1 dst, w2 x
pub const R_TYPEOF: int = 46;
/// w1 the iterator, w2 where to go when it is done, w3 the value's slot
pub const R_ITER_NEXT: int = 47;
/// w1 dst, w2 x: ToNumeric (sub 0) or -x (sub 1)
pub const R_TONUM: int = 48;
/// w1 dst, w2 x: x + 1 (sub 0) or x - 1 (sub 1)
pub const R_INC: int = 49;
/// w1 the value, w3 the atom: `name = value` for a global (the value
/// stays where it is); the inline cache is the stack op's c
pub const R_SET_GLOBAL: int = 50;
/// sub env depth, w1 the old value, w2 what is added, w3 the env index:
/// `x += v` for a closure variable (the stack op's a + b goes to the env)
pub const R_ADD_ENV: int = 51;
/// w1 dst (f; the receiver in dst + 1), w2 obj, w3 key: `o[k](...)`
pub const R_GET_METHOD_ELEM: int = 52;
/// w1 the value (it stays), sub 0: a let/const read before its
/// declaration (w2 the name's constant), sub 1: destructuring null/undefined
pub const R_CHECK: int = 53;
/// w1 x, w2 where to go when x is not undefined (a default's test)
pub const R_JNU: int = 54;
/// a stack op with sub (0..2) operands, w2 and w3, and one result in w1:
/// the operands go above the frame, the op runs as the stack machine
/// does it, its result comes back (instanceof, in, delete o[k], a
/// function's name, for-in keys, a short array literal)
pub const R_STEP_K: int = 55;
/// `new f(args)`: w1 f (the args after it), w2 argc, w3 the window's top.
/// A plain JS constructor runs in rrun with `this` put after f (the
/// args move up one slot); anything else runs the stack op on the window.
pub const R_NEW: int = 56;
/// `[a, b, c, …]`: w1 the first item's slot (the array goes there), w2
/// the count
pub const R_NEW_ARRAY: int = 57;

const BAD: int = 1000000;

/// The net stack effect of an op that falls through to the next one; BAD
/// for ops handled by `Tr::flow` or unknown.
pub fn net_effect(op: &Op) -> int {
    let c = op.code;
    if (c as int) >= OP_ADD && (c as int) <= OP_GE {
        return -1;
    }
    match (c as int) {
        OP_UNDEF | OP_NULL | OP_TRUE | OP_FALSE | OP_INT | OP_CONST | OP_GET_LOCAL | OP_GET_ENV | OP_GET_GLOBAL | OP_TYPEOF_GLOBAL | OP_CLOSURE | OP_THIS | OP_NEW_TARGET | OP_ARGUMENTS | OP_NEW_OBJECT | OP_CALLEE | OP_GET_THIS_PROP | OP_TDZ | OP_REGEX | OP_GET_SUPER | OP_POSTINC_LOCAL | OP_PREINC_LOCAL | OP_GET_LOCAL_PROP | OP_DUP | OP_OVER | OP_ARRAY_REST | OP_GET_METHOD => 1,
        OP_GET_LOCAL2 | OP_GET_ENV_UNDEF | OP_DUP2 => 2,
        OP_SET_LOCAL | OP_SET_ENV | OP_SET_GLOBAL | OP_DECL_GLOBAL | OP_INC_LOCAL | OP_CHECK_TDZ | OP_REQUIRE_OBJ | OP_SET_NAME | OP_CONST_ERROR | OP_NOP | OP_END_TRY | OP_PUSH_ENV | OP_POP_ENV | OP_COPY_ENV | OP_GEN_START | OP_ITER_CLOSE_AT | OP_ARRAY_HOLE | OP_SWAP | OP_ROT3 | OP_ROT4 | OP_GET_METHOD_ELEM => 0,
        OP_GET_PROP | OP_NEG | OP_TONUM | OP_NOT | OP_BNOT | OP_TYPEOF | OP_INC | OP_DEC | OP_TOSTR | OP_BIGINT | OP_DEL_PROP | OP_GET_PRIVATE | OP_ITER_KEYS | OP_ITER_VALUES | OP_ASYNC_ITER | OP_TO_ARRAY | OP_GET_SUPER_ELEM | OP_SUPER_CALL_SPREAD | OP_TO_KEY => 0,
        OP_INSTANCEOF | OP_IN | OP_GET_ELEM | OP_SET_PROP | OP_DEL_ELEM | OP_INIT_PROP | OP_DEFINE_FIELD | OP_INIT_GETTER | OP_INIT_SETTER | OP_INIT_SPREAD | OP_DEFINE_METHOD | OP_ARRAY_PUSH | OP_ARRAY_SPREAD | OP_SET_FIELDS | OP_NEW_SPREAD | OP_POP | OP_ITER_CLOSE | OP_ADD_LOCAL_POP => -1,
        OP_SET_ELEM | OP_INIT_ELEM | OP_DEFINE_FIELD_ELEM | OP_INIT_GETTER_ELEM | OP_INIT_SETTER_ELEM | OP_DEFINE_METHOD_ELEM | OP_CALL_SPREAD | OP_ADD_ENV_POP => -2,
        OP_ADD_PROP_POP => -3,
        OP_YIELD => 1,
        OP_CALL => ((-op.a - 1) as int),
        OP_NEW => (-op.a as int),
        OP_SUPER_CALL | OP_NEW_ARRAY | OP_OBJ_REST => (1 - op.a as int),
        OP_CLASS => {
            if op.a == 1 {
                0
            } else {
                1
            }
        }
        _ => BAD,
    }
}

/// The translation of one function.
pub struct Tr {
    pub code: Vec<Op>,
    pub consts: Vec<Val>,
    pub n: int,
    pub nslots: int,
    /// the stack depth before each op, -1 where no path reaches
    pub depth: Vec<int>,
    /// reached by a jump, a handler or a join: the abstract stack starts
    /// over there with every value in its own slot
    pub label: Vec<bool>,
    pub maxd: int,
    pub rc: Vec<Op>,
    pub kv: Vec<Val>,
    /// where kv starts in Vm.kvals
    pub kbase: int,
    /// switch tables, and where they start in Vm.rtab
    pub tab: Vec<int>,
    pub tbase: int,
    /// kv index of consts[i] + 1, 0 when not yet in kv
    pub kc: Vec<int>,
    /// the abstract stack: where each value is read from (a register),
    /// the load that copies it into its own slot when it must be there (-1
    /// when it is), and the op whose result it is (-1 if none or unknown)
    pub es: Vec<int>,
    pub el: Vec<int>,
    pub ep: Vec<int>,
    pub err: String,
    /// the first instruction that does something
    pub entry: int,
}

impl Tr {
    pub fn new(p: &Proto, kbase: int, tbase: int) -> Tr {
        let n = p.code.len() as int;
        let mut t = Tr {
            code: p.code.clone(),
            consts: p.consts.clone(),
            n: n,
            nslots: p.nslots,
            depth: Vec::new(),
            label: Vec::new(),
            maxd: 0,
            rc: Vec::new(),
            kv: Vec::new(),
            kbase: kbase,
            tab: Vec::new(),
            tbase: tbase,
            kc: Vec::new(),
            es: Vec::new(),
            el: Vec::new(),
            ep: Vec::new(),
            err: String::new(),
            entry: 0,
        };
        let mut i: int = 0;
        while i < n {
            t.depth.push(-1);
            t.label.push(false);
            t.rc.push(Op { code: (R_NOP as i32), a: 0, b: 0, c: 0 });
            i += 1;
        }
        let mut k: usize = 0;
        while k < t.consts.len() {
            t.kc.push(0);
            k += 1;
        }
        t
    }

    fn fail(&mut self, what: &str, at: int) {
        if self.err.is_empty() {
            self.err = format!("register code: {} at {}", what, at);
        }
    }

    fn edge(&mut self, to: int, d: int, jump: bool, work: &mut Vec<int>, from: int) {
        if to < 0 || to >= self.n || d < 0 {
            self.fail("bad edge", from);
            return;
        }
        if jump {
            self.label[to as usize] = true;
        }
        if d > self.maxd {
            self.maxd = d;
        }
        let cur = self.depth[to as usize];
        if cur < 0 {
            self.depth[to as usize] = d;
            work.push(to);
        } else if cur != d {
            self.fail("stack depths differ", to);
        }
    }

    /// The stack depth at every op (a worklist over the edges).
    pub fn flow(&mut self) {
        if self.n == 0 {
            return;
        }
        let mut work: Vec<int> = Vec::new();
        self.depth[0] = 0;
        work.push(0);
        while !work.is_empty() && self.err.is_empty() {
            let i = work.pop().unwrap();
            let d = self.depth[i as usize];
            let op = self.code[i as usize];
            let c = op.code;
            if (c as int) == OP_JUMP {
                self.edge((op.a as int), d, true, &mut work, i);
            } else if (c as int) == OP_JF || (c as int) == OP_JT || (c as int) == OP_JNOT_UNDEF {
                self.edge((op.a as int), d - 1, true, &mut work, i);
                self.edge(i + 1, d - 1, false, &mut work, i);
            } else if (c as int) == OP_JF_KEEP || (c as int) == OP_JT_KEEP || (c as int) == OP_JNN_KEEP {
                self.edge((op.a as int), d, true, &mut work, i);
                self.edge(i + 1, d - 1, false, &mut work, i);
            } else if (c as int) == OP_JNULLISH {
                if op.b == 1 {
                    self.edge((op.a as int), d, true, &mut work, i);
                    self.edge(i + 1, d, false, &mut work, i);
                } else {
                    let m = if op.b > 1 { op.b } else { 1 };
                    self.edge((op.a as int), d - (m as int) + 1, true, &mut work, i);
                    self.edge(i + 1, d - 1, false, &mut work, i);
                }
            } else if (c as int) == OP_CMP_JF {
                self.edge((op.a as int), d - 2, true, &mut work, i);
                self.edge(i + 1, d - 2, false, &mut work, i);
            } else if (c as int) == OP_LOCAL_NULL_JF || (c as int) == OP_CASE_INT_JT {
                self.edge((op.a as int), d, true, &mut work, i);
                self.edge(i + 1, d, false, &mut work, i);
            } else if (c as int) == OP_CASE_JT {
                self.edge((op.a as int), d - 1, true, &mut work, i);
                self.edge(i + 1, d - 1, false, &mut work, i);
            } else if (c as int) == OP_ITER_NEXT {
                self.edge((op.a as int), d, true, &mut work, i);
                self.edge(i + 1, d + 1, false, &mut work, i);
            } else if (c as int) == OP_TEMPLATE_OBJ {
                if op.b >= 0 {
                    self.edge((op.a as int), d + 1, true, &mut work, i);
                    self.edge(i + 1, d, false, &mut work, i);
                } else {
                    self.edge(i + 1, d + 1 - (2 * op.a as int), false, &mut work, i);
                }
            } else if (c as int) == OP_GEN_RESUME {
                self.edge((op.a as int), d - 1, true, &mut work, i);
                self.edge(i + 1, d - 1, false, &mut work, i);
            } else if (c as int) == OP_ASTAR_CALL {
                self.edge((op.a as int), d - 2, true, &mut work, i);
                self.edge((op.b as int), d - 1, true, &mut work, i);
                self.edge(i + 1, d, false, &mut work, i);
            } else if (c as int) == OP_YIELD_STAR || (c as int) == OP_ASTAR_DONE {
                self.edge((op.a as int), d - 2, true, &mut work, i);
                self.edge((op.b as int), d - 1, true, &mut work, i);
                self.edge(i + 1, d - 1, false, &mut work, i);
            } else if (c as int) == OP_TRY {
                self.edge((op.a as int), d + 1, true, &mut work, i);
                self.edge(i + 1, d, false, &mut work, i);
            } else if (c as int) == OP_RETURN || (c as int) == OP_RETURN_UNDEF || (c as int) == OP_THROW {
                // no successor
            } else if (c as int) == OP_SET_LOCAL_POP {
                self.edge(i + 2, d - 1, false, &mut work, i);
            } else if (c as int) == OP_SET_PROP_POP {
                self.edge(i + 2, d - 2, false, &mut work, i);
            } else if (c as int) == OP_SET_ELEM_POP {
                self.edge(i + 2, d - 3, false, &mut work, i);
            } else {
                let e = net_effect(&op);
                if e == BAD {
                    self.fail("unknown op", i);
                    return;
                }
                if d + e + 1 > self.maxd {
                    self.maxd = d + e + 1;
                }
                self.edge(i + 1, d + e, false, &mut work, i);
            }
        }
        // a store that steps over its POP joins the POP's path after it
        let mut i: int = 0;
        while i + 2 < self.n {
            let c = self.code[i as usize].code;
            if ((c as int) == OP_SET_LOCAL_POP || (c as int) == OP_SET_PROP_POP || (c as int) == OP_SET_ELEM_POP) && self.depth[i as usize] >= 0 && self.depth[(i + 1) as usize] >= 0 {
                self.label[(i + 2) as usize] = true;
            }
            i += 1;
        }
    }

    fn set(&mut self, i: int, rop: int, sub: int, a: int, b: int, c: int) {
        self.rc[i as usize] = Op { code: (rop + 256 * sub) as i32, a: (a as i32), b: (b as i32), c: (c as i32) };
    }

    fn rop_at(&self, i: int) -> int {
        ((self.rc[i as usize].code % 256) as int)
    }

    fn kval(&mut self, v: Val) -> int {
        let mut i: usize = 0;
        while i < self.kv.len() && i < 32 {
            let same = match (&self.kv[i], &v) {
                (Val::Undef, Val::Undef) => true,
                (Val::Null, Val::Null) => true,
                (Val::Bool(x), Val::Bool(y)) => *x == *y,
                (Val::Num(x), Val::Num(y)) => *x == *y && (1.0 / *x) == (1.0 / *y),
                _ => false,
            };
            if same {
                return -1 - (self.kbase + i as int);
            }
            i += 1;
        }
        self.kv.push(v);
        -(self.kbase + self.kv.len() as int)
    }

    fn kconst(&mut self, ci: int) -> int {
        let have = self.kc[ci as usize];
        if have > 0 {
            return -have;
        }
        let v = self.consts[ci as usize].clone();
        self.kv.push(v);
        let k = self.kbase + self.kv.len() as int;
        self.kc[ci as usize] = k;
        -k
    }

    fn top(&self) -> int {
        self.es.len() as int
    }

    fn push_pend(&mut self, src: int, load: int) {
        self.es.push(src);
        self.el.push(load);
        self.ep.push(-1);
    }

    /// a value in its own slot, made by op `prod` (-1: not one that can
    /// write elsewhere)
    fn push_own(&mut self, prod: int) {
        let k = self.es.len() as int;
        self.es.push(self.nslots + k);
        self.el.push(-1);
        self.ep.push(prod);
    }

    fn popn(&mut self, m: int) {
        let mut i: int = 0;
        while i < m {
            self.es.pop();
            self.el.pop();
            self.ep.pop();
            i += 1;
        }
    }

    fn opnd(&self, k: int) -> int {
        self.es[k as usize]
    }

    fn reset(&mut self, d: int) {
        self.es = Vec::new();
        self.el = Vec::new();
        self.ep = Vec::new();
        let mut k: int = 0;
        while k < d {
            self.push_own(-1);
            k += 1;
        }
    }

    /// Value k into its own slot: its load becomes a move.
    fn mat(&mut self, k: int) {
        let l = self.el[k as usize];
        if l < 0 {
            return;
        }
        let dst = self.nslots + k;
        let src = self.es[k as usize];
        let cur = self.rop_at(l);
        if cur == R_NOP {
            self.set(l, R_MOVE, 0, dst, src, 0);
        } else if cur == R_MOVE {
            // the other half of a GET_LOCAL2
            let d0 = self.rc[l as usize].a;
            let s0 = self.rc[l as usize].b;
            if ((d0 + 1) as int) == dst {
                self.set(l, R_LOAD2, 0, (d0 as int), (s0 as int), src);
            } else if dst + 1 == (d0 as int) {
                self.set(l, R_LOAD2, 0, dst, src, (s0 as int));
            } else {
                self.fail("load shared", l);
            }
        } else {
            self.fail("load is used", l);
        }
        self.es[k as usize] = dst;
        self.el[k as usize] = -1;
    }

    fn mat_all(&mut self) {
        let mut k: int = 0;
        while k < self.top() {
            self.mat(k);
            k += 1;
        }
    }

    fn mat_from(&mut self, from: int) {
        let mut k = from;
        while k < self.top() {
            self.mat(k);
            k += 1;
        }
    }

    /// Before local x is written: the values still read from it get their
    /// own slots.
    fn mat_local(&mut self, x: int) {
        let mut k: int = 0;
        while k < self.top() {
            if self.el[k as usize] >= 0 && self.es[k as usize] == x {
                self.mat(k);
            }
            k += 1;
        }
    }

    /// The top value is the result of the op just before `i` with nothing
    /// run since: that op may write it to local x instead. Answers the op.
    fn fusable_top(&self, i: int) -> int {
        let t = self.top();
        if t == 0 {
            return -1;
        }
        let p = self.ep[(t - 1) as usize];
        if p < 0 || self.el[(t - 1) as usize] >= 0 {
            return -1;
        }
        let mut j = p + 1;
        while j < i {
            if self.rop_at(j) != R_NOP {
                return -1;
            }
            j += 1;
        }
        p
    }

    fn result(&mut self, i: int, rop: int, sub: int, pops: int, b: int, c: int) {
        self.popn(pops);
        let dst = self.nslots + self.top();
        self.set(i, rop, sub, dst, b, c);
        self.push_own(i);
    }

    /// The register code, or err set.
    pub fn translate(&mut self) {
        self.flow();
        if !self.err.is_empty() {
            return;
        }
        let ns = self.nslots;
        let mut expect: int = -1;
        let mut i: int = 0;
        while i < self.n && self.err.is_empty() {
            let d = self.depth[i as usize];
            if d < 0 {
                i += 1;
                continue;
            }
            if expect != i || self.label[i as usize] {
                if expect > i && !self.label[expect as usize] {
                    self.fail("a path crosses", i);
                    return;
                }
                self.reset(d);
            }
            if self.top() != d {
                self.fail("depth", i);
                return;
            }
            let op = self.code[i as usize];
            let c = op.code;
            let mut next = i + 1;
            if (c as int) == OP_UNDEF || (c as int) == OP_NULL || (c as int) == OP_TRUE || (c as int) == OP_FALSE || (c as int) == OP_INT {
                let v = match (c as int) {
                    OP_UNDEF => Val::Undef,
                    OP_NULL => Val::Null,
                    OP_TRUE => Val::Bool(true),
                    OP_FALSE => Val::Bool(false),
                    _ => Val::Num(op.a as double),
                };
                let k = self.kval(v);
                self.push_pend(k, i);
            } else if (c as int) == OP_CONST {
                let k = self.kconst((op.a as int));
                self.push_pend(k, i);
            } else if (c as int) == OP_GET_LOCAL {
                self.push_pend((op.a as int), i);
            } else if (c as int) == OP_GET_LOCAL2 {
                self.push_pend((op.a as int), i);
                self.push_pend((op.b as int), i);
            } else if (c as int) == OP_DUP2 {
                // both copies load in one op (R_LOAD2 when both are used)
                let s0 = self.opnd(d - 2);
                let s1 = self.opnd(d - 1);
                self.push_pend(s0, i);
                self.push_pend(s1, i);
            } else if (c as int) == OP_DUP || (c as int) == OP_OVER {
                let from = if (c as int) == OP_DUP { d - 1 } else { d - 2 };
                let s = self.opnd(from);
                self.push_pend(s, i);
            } else if (c as int) == OP_POP {
                self.popn(1);
            } else if (c as int) == OP_SET_LOCAL || (c as int) == OP_SET_LOCAL_POP {
                let x = op.a;
                self.mat_local((x as int));
                let p = self.fusable_top(i);
                if p >= 0 {
                    if self.rop_at(p) == R_CALL {
                        // the call writes its result to x (c holds x + 1)
                        self.rc[p as usize].c = x + 1;
                    } else {
                        self.rc[p as usize].a = x;
                    }
                    self.popn(1);
                    if (c as int) == OP_SET_LOCAL {
                        self.push_pend((x as int), i);
                    }
                } else {
                    let s = self.opnd(d - 1);
                    self.set(i, R_MOVE, 0, (x as int), s, 0);
                    if (c as int) == OP_SET_LOCAL_POP {
                        self.popn(1);
                    }
                }
                if (c as int) == OP_SET_LOCAL_POP {
                    next = i + 2;
                }
            } else if (c as int) == OP_GET_PROP {
                let o = self.opnd(d - 1);
                self.result(i, R_GET_PROP, 0, 1, o, -1);
            } else if (c as int) == OP_GET_LOCAL_PROP {
                self.result(i, R_GET_PROP, 0, 0, (op.a as int), -1);
            } else if (c as int) == OP_GET_THIS_PROP {
                self.result(i, R_GET_THIS_PROP, 0, 0, 0, -1);
            } else if (c as int) == OP_SET_PROP_POP {
                let o = self.opnd(d - 2);
                let v = self.opnd(d - 1);
                self.popn(2);
                self.set(i, R_SET_PROP, 0, o, v, -1);
                next = i + 2;
            } else if (c as int) == OP_GET_ELEM {
                let o = self.opnd(d - 2);
                let k = self.opnd(d - 1);
                self.result(i, R_GET_ELEM, 0, 2, o, k);
            } else if (c as int) == OP_SET_ELEM_POP {
                let o = self.opnd(d - 3);
                let k = self.opnd(d - 2);
                let v = self.opnd(d - 1);
                self.popn(3);
                self.set(i, R_SET_ELEM, 0, o, k, v);
                next = i + 2;
            } else if (c as int) == OP_SET_ELEM {
                // the value stays where the object was (as OP_SET_PROP)
                self.mat(d - 3);
                let k = self.opnd(d - 2);
                let v = self.opnd(d - 1);
                self.popn(3);
                self.set(i, R_SET_ELEM, 1, ns + d - 3, k, v);
                self.push_own(-1);
            } else if (c as int) >= OP_ADD && (c as int) <= OP_GE {
                let x = self.opnd(d - 2);
                let y = self.opnd(d - 1);
                let (rop, sub) = if (c as int) == OP_ADD {
                    (R_ADD, 0)
                } else if (c as int) == OP_SUB {
                    (R_SUB, 0)
                } else if (c as int) == OP_MUL {
                    (R_MUL, 0)
                } else if (c as int) >= OP_EQ {
                    (R_CMP, c)
                } else {
                    (R_ARITH, c)
                };
                self.result(i, rop, (sub as int), 2, x, y);
            } else if (c as int) == OP_ITER_NEXT {
                self.mat_all();
                self.set(i, R_ITER_NEXT, 0, ns + d - 1, (op.a as int), ns + d);
                self.push_own(-1);
            } else if (c as int) == OP_SET_PROP {
                // the value stays where the object was
                self.mat(d - 2);
                let v = self.opnd(d - 1);
                self.popn(2);
                self.set(i, R_SET_PROP, 1, ns + d - 2, v, -1);
                self.push_own(-1);
            } else if (c as int) == OP_TYPEOF {
                let x = self.opnd(d - 1);
                self.result(i, R_TYPEOF, 0, 1, x, 0);
            } else if (c as int) == OP_NOT {
                let x = self.opnd(d - 1);
                self.result(i, R_NOT, 0, 1, x, 0);
            } else if (c as int) == OP_TONUM || (c as int) == OP_NEG {
                let x = self.opnd(d - 1);
                self.result(i, R_TONUM, if (c as int) == OP_NEG { 1 } else { 0 }, 1, x, 0);
            } else if (c as int) == OP_INC || (c as int) == OP_DEC {
                let x = self.opnd(d - 1);
                self.result(i, R_INC, if (c as int) == OP_INC { 0 } else { 1 }, 1, x, 0);
            } else if (c as int) == OP_CMP_JF {
                let x = self.opnd(d - 2);
                let y = self.opnd(d - 1);
                self.popn(2);
                self.mat_all();
                self.set(i, R_CMP_JF, (op.b as int), x, y, (op.a as int));
            } else if (c as int) == OP_JUMP {
                self.mat_all();
                self.set(i, R_JUMP, 0, (op.a as int), 0, 0);
                next = -1;
            } else if (c as int) == OP_JF || (c as int) == OP_JT {
                let x = self.opnd(d - 1);
                self.popn(1);
                self.mat_all();
                self.set(i, if (c as int) == OP_JF { R_JF } else { R_JT }, 0, x, (op.a as int), 0);
            } else if (c as int) == OP_JNOT_UNDEF {
                let x = self.opnd(d - 1);
                self.popn(1);
                self.mat_all();
                self.set(i, R_JNU, 0, x, (op.a as int), 0);
            } else if (c as int) == OP_JF_KEEP || (c as int) == OP_JT_KEEP || (c as int) == OP_JNN_KEEP {
                self.mat_all();
                let rop = if (c as int) == OP_JF_KEEP {
                    R_JF_KEEP
                } else if (c as int) == OP_JT_KEEP {
                    R_JT_KEEP
                } else {
                    R_JNN_KEEP
                };
                self.set(i, rop, 0, ns + d - 1, (op.a as int), 0);
                self.popn(1);
            } else if (c as int) == OP_LOCAL_NULL_JF {
                self.mat_all();
                self.set(i, R_LOCAL_NULL_JF, 0, ((op.b / 256) as int), ((op.b % 256) as int), (op.a as int));
            } else if (c as int) == OP_CASE_INT_JT {
                self.mat_all();
                self.set(i, R_CASE_INT_JT, 0, ns + d - 1, (op.b as int), (op.a as int));
            } else if (c as int) == OP_CASE_JT {
                let y = self.opnd(d - 1);
                self.popn(1);
                self.mat_all();
                self.set(i, R_CASE_JT, 0, ns + d - 2, y, (op.a as int));
            } else if (c as int) == OP_RETURN {
                let x = self.opnd(d - 1);
                self.set(i, R_RET, 0, x, 0, 0);
                next = -1;
            } else if (c as int) == OP_RETURN_UNDEF {
                self.set(i, R_RET_UNDEF, 0, 0, 0, 0);
                next = -1;
            } else if (c as int) == OP_CALL {
                let w = d - (op.a as int) - 2;
                // an undefined `this` is written by the call itself
                let mut sub = 0;
                let tk = (w + 1) as usize;
                if self.el[tk] >= 0 && self.es[tk] < 0 {
                    if let Val::Undef = self.kv[(-1 - self.es[tk] - self.kbase) as usize] {
                        sub = 1;
                        self.es[tk] = ns + w + 1;
                        self.el[tk] = -1;
                    }
                }
                self.mat_from(w);
                self.popn(((op.a + 2) as int));
                self.set(i, R_CALL, sub, ns + w, (op.a as int), 0);
                self.push_own(i);
            } else if (c as int) == OP_NEW_ARRAY && op.a > 2 {
                let w = d - (op.a as int);
                self.mat_from(w);
                self.popn((op.a as int));
                self.set(i, R_NEW_ARRAY, 0, ns + w, (op.a as int), 0);
                self.push_own(-1);
            } else if (c as int) == OP_NEW {
                let w = d - (op.a as int) - 1;
                // the stack op may run on the window: every operand in place
                self.mat_all();
                self.popn(((op.a + 1) as int));
                self.set(i, R_NEW, 0, ns + w, (op.a as int), ns + d);
                self.push_own(-1);
            } else if (c as int) == OP_GET_METHOD {
                let o = self.opnd(d - 1);
                self.popn(1);
                // c is the own-property cache; the atom is the stack op's
                self.set(i, R_GET_METHOD, (op.b as int), ns + d - 1, o, 0);
                self.push_own(-1);
                self.push_own(-1);
            } else if (c as int) == OP_THIS {
                self.result(i, R_THIS, 0, 0, 0, 0);
            } else if (c as int) == OP_GET_ENV {
                self.result(i, R_GET_ENV, 0, 0, (op.a as int), (op.b as int));
            } else if (c as int) == OP_GET_ENV_UNDEF {
                self.set(i, R_GET_ENV, 1, ns + d, (op.a as int), (op.b as int));
                self.push_own(-1);
                self.push_own(-1);
            } else if (c as int) == OP_SET_ENV {
                let v = self.opnd(d - 1);
                self.set(i, R_SET_ENV, 0, v, (op.a as int), (op.b as int));
            } else if (c as int) == OP_GET_GLOBAL {
                self.result(i, R_GET_GLOBAL, 0, 0, 0, (op.a as int));
            } else if (c as int) == OP_ADD_ENV_POP && op.a < 256 {
                let x = self.opnd(d - 2);
                let y = self.opnd(d - 1);
                self.popn(2);
                self.set(i, R_ADD_ENV, (op.a as int), x, y, (op.b as int));
            } else if (c as int) == OP_CHECK_TDZ || (c as int) == OP_REQUIRE_OBJ {
                let x = self.opnd(d - 1);
                self.set(i, R_CHECK, if (c as int) == OP_CHECK_TDZ { 0 } else { 1 }, x, (op.a as int), 0);
            } else if (c as int) == OP_GET_METHOD_ELEM {
                let o = self.opnd(d - 2);
                let k = self.opnd(d - 1);
                self.popn(2);
                self.set(i, R_GET_METHOD_ELEM, 0, ns + d - 2, o, k);
                self.push_own(-1);
                self.push_own(-1);
            } else if (c as int) == OP_SET_GLOBAL {
                let v = self.opnd(d - 1);
                self.set(i, R_SET_GLOBAL, (op.b as int), v, 0, (op.a as int));
            } else if (c as int) == OP_NEW_OBJECT {
                self.result(i, R_NEW_OBJECT, 0, 0, 0, 0);
            } else if (c as int) == OP_INIT_PROP && op.b != 1 {
                let o = self.opnd(d - 2);
                let v = self.opnd(d - 1);
                self.popn(1);
                self.set(i, R_INIT_PROP, 0, o, v, (op.a as int));
                // the object stays where it was; nothing writes it elsewhere
                let t = self.top();
                self.ep[(t - 1) as usize] = -1;
            } else if (c as int) == OP_CLOSURE {
                self.result(i, R_CLOSURE, 0, 0, (op.a as int), 0);
            } else if (c as int) == OP_INC_LOCAL {
                self.mat_local((op.a as int));
                self.set(i, R_INC_LOCAL, 0, (op.a as int), (op.b as int), 0);
            } else if (c as int) == OP_POSTINC_LOCAL || (c as int) == OP_PREINC_LOCAL {
                self.mat_local((op.a as int));
                let sub = if (c as int) == OP_PREINC_LOCAL { 1 } else { 0 };
                self.result(i, R_POSTINC, sub, 0, (op.a as int), (op.b as int));
            } else if (c as int) == OP_ADD_LOCAL_POP {
                let v = self.opnd(d - 1);
                self.popn(1);
                self.mat_local((op.a as int));
                self.set(i, R_ADD_LOCAL_POP, 0, (op.a as int), v, 0);
            } else if (c as int) == OP_NOP || (c as int) == OP_END_TRY || (c as int) == OP_PUSH_ENV || (c as int) == OP_POP_ENV || (c as int) == OP_COPY_ENV || (c as int) == OP_DECL_GLOBAL || (c as int) == OP_CONST_ERROR {
                self.set(i, R_STEP0, 0, 0, 0, 0);
                if (c as int) == OP_CONST_ERROR {
                    // it throws; nothing after it on this path
                    self.mat_all();
                }
            } else if (c as int) == OP_INSTANCEOF || (c as int) == OP_IN || (c as int) == OP_DEL_ELEM || (c as int) == OP_SET_NAME || (c as int) == OP_ITER_KEYS || ((c as int) == OP_NEW_ARRAY && op.a <= 2) {
                let k: int = if (c as int) == OP_NEW_ARRAY { op.a as int } else if (c as int) == OP_SET_NAME || (c as int) == OP_ITER_KEYS { 1 } else { 2 };
                let x = if k >= 1 { self.opnd(d - k) } else { 0 };
                let y = if k >= 2 { self.opnd(d - 1) } else { 0 };
                self.result(i, R_STEP_K, k, k, x, y);
            } else if (c as int) == OP_TDZ || (c as int) == OP_NEW_TARGET || (c as int) == OP_ARGUMENTS || (c as int) == OP_CALLEE || (c as int) == OP_REGEX || (c as int) == OP_TYPEOF_GLOBAL || (c as int) == OP_GET_SUPER {
                self.result(i, R_STEP_PUSH, 0, 0, 0, 0);
            } else {
                // the stack op on the frame's window
                self.mat_all();
                self.set(i, R_WIN, 0, ns + d, 0, 0);
                if (c as int) == OP_RETURN || (c as int) == OP_THROW {
                    next = -1;
                } else if (c as int) == OP_SET_PROP || (c as int) == OP_SET_ELEM || (c as int) == OP_INIT_PROP || (c as int) == OP_GET_METHOD_ELEM || (c as int) == OP_ADD_ENV_POP || (c as int) == OP_ADD_PROP_POP {
                    let e = net_effect(&op);
                    self.reset(d + e);
                } else if (c as int) == OP_THROW {
                    next = -1;
                } else {
                    // the fall-through depth
                    let nd = if i + 1 < self.n { self.depth[(i + 1) as usize] } else { -1 };
                    if (c as int) == OP_JNULLISH || (c as int) == OP_JNOT_UNDEF || (c as int) == OP_ITER_NEXT || (c as int) == OP_TEMPLATE_OBJ || (c as int) == OP_GEN_RESUME || (c as int) == OP_YIELD_STAR || (c as int) == OP_ASTAR_CALL || (c as int) == OP_ASTAR_DONE || (c as int) == OP_TRY || net_effect(&op) != BAD {
                        if nd < 0 {
                            next = -1;
                        } else {
                            self.reset(nd);
                        }
                    } else {
                        self.fail("op", i);
                        return;
                    }
                }
            }
            if next >= 0 && next < self.n && self.label[next as usize] {
                self.mat_all();
            }
            expect = next;
            i += 1;
        }
        self.finish();
    }

    /// The instruction after the one at i.
    fn after(&self, i: int) -> int {
        i + 1 + ((self.rc[i as usize].code / 65536) as int)
    }

    /// Where a keeping jump from register x ends up when it lands on t with
    /// a value whose truth is v: past the conditional jumps on x there.
    fn thread(&self, t0: int, x: int, v: bool) -> int {
        let mut t = t0;
        let mut k = 0;
        while k < 8 && t < self.n {
            let o = self.rc[t as usize];
            let rop = o.code % 256;
            if !((rop as int) == R_JF || (rop as int) == R_JT || (rop as int) == R_JF_KEEP || (rop as int) == R_JT_KEEP) || (o.a as int) != x {
                break;
            }
            let jumps = if (rop as int) == R_JF || (rop as int) == R_JF_KEEP { !v } else { v };
            t = if jumps { (o.b as int) } else { self.after(t) };
            k += 1;
        }
        t
    }

    /// A comparison followed by a conditional jump on its result becomes
    /// one instruction, and a keeping jump onto a jump on the same value
    /// goes where that one would.
    fn fuse_branches(&mut self) {
        let n = self.n;
        let mut i: int = 0;
        while i < n {
            let k = i as usize;
            let rop = self.rc[k].code % 256;
            if (rop as int) == R_CMP {
                let j = self.after(i);
                if j < n {
                    let o = self.rc[j as usize];
                    let jr = o.code % 256;
                    let sub = (self.rc[k].code / 256) % 256;
                    let (dst, x, y) = (self.rc[k].a, self.rc[k].b, self.rc[k].c);
                    let skip = self.after(j) - i - 1;
                    // a temp dies at its jump; a local (var b = x === y)
                    // keeps the value, so it fuses only as a keeping jump
                    let temp = (dst as int) >= self.nslots;
                    if o.a == dst && skip < 32000 {
                        if ((jr as int) == R_JF || (jr as int) == R_JT) && !temp {
                            let jt = (jr as int) == R_JT;
                            let t = self.thread((o.b as int), (dst as int), jt);
                            if t < 32000 && dst < 65536 {
                                let r2 = if jt { R_CMP_JTK } else { R_CMP_JFK };
                                self.rc[k] = Op { code: (r2 + 256 * (sub as int) + 65536 * skip) as i32, a: dst + (65536 * t) as i32, b: x, c: y };
                            }
                        } else if (jr as int) == R_JF || (jr as int) == R_JT {
                            let r2 = if (jr as int) == R_JF { R_CMP_JF } else { R_CMP_JT };
                            self.rc[k] = Op { code: (r2 + 256 * (sub as int) + 65536 * skip) as i32, a: x, b: y, c: o.b };
                        } else if ((jr as int) == R_JF_KEEP || (jr as int) == R_JT_KEEP) && dst < 65536 {
                            let jt = (jr as int) == R_JT_KEEP;
                            let t = self.thread((o.b as int), (dst as int), jt);
                            if t < 32000 {
                                let r2 = if jt { R_CMP_JTK } else { R_CMP_JFK };
                                self.rc[k] = Op { code: (r2 + 256 * (sub as int) + 65536 * skip) as i32, a: dst + (65536 * t) as i32, b: x, c: y };
                            }
                        }
                    }
                }
            } else if (rop as int) == R_JUMP {
                // through jumps to jumps; a jump to a return returns
                let mut t = self.rc[k].a;
                let mut m = 0;
                while m < 8 && (t as int) < n && ((self.rc[t as usize].code % 256) as int) == R_JUMP && (t as int) != i {
                    t = self.rc[t as usize].a;
                    m += 1;
                }
                let tr = if (t as int) < n { self.rc[t as usize].code % 256 } else { -1 };
                if (tr as int) == R_RET || (tr as int) == R_RET_UNDEF {
                    self.rc[k] = self.rc[t as usize];
                } else {
                    self.rc[k].a = t;
                }
            } else if (rop as int) == R_JF_KEEP || (rop as int) == R_JT_KEEP {
                let x = self.rc[k].a;
                let t = self.thread((self.rc[k].b as int), (x as int), (rop as int) == R_JT_KEEP);
                self.rc[k].b = (t as i32);
            }
            i += 1;
        }
    }

    /// A run of at least four R_CASE_INT_JT on one value with keys close
    /// together starts with an R_SWITCH that jumps where the run would.
    fn switch_tables(&mut self) {
        let n = self.n;
        let mut i: int = 0;
        while i < n {
            let o = self.rc[i as usize];
            if ((o.code % 256) as int) == R_CASE_INT_JT {
                let x = o.a;
                let mut keys: Vec<int> = Vec::new();
                let mut tgts: Vec<int> = Vec::new();
                let mut j = i;
                while j < n && ((self.rc[j as usize].code % 256) as int) == R_CASE_INT_JT && self.rc[j as usize].a == x && keys.len() < 1024 {
                    keys.push((self.rc[j as usize].b as int));
                    tgts.push((self.rc[j as usize].c as int));
                    j = self.after(j);
                }
                let mut lo = keys[0];
                let mut hi = keys[0];
                let mut k: usize = 0;
                while k < keys.len() {
                    if keys[k] < lo {
                        lo = keys[k];
                    }
                    if keys[k] > hi {
                        hi = keys[k];
                    }
                    k += 1;
                }
                let size = hi - lo + 1;
                if keys.len() >= 4 && size <= 4 * (keys.len() as int) + 8 {
                    let at = self.tbase + self.tab.len() as int;
                    self.tab.push(size);
                    let base = self.tab.len();
                    let mut m: int = 0;
                    while m < size {
                        self.tab.push(j);
                        m += 1;
                    }
                    self.tab.push(j);
                    // the first case with a key wins, as the run does
                    k = keys.len();
                    while k > 0 {
                        k -= 1;
                        self.tab[base + (keys[k] - lo) as usize] = tgts[k];
                    }
                    let skip = self.rc[i as usize].code / 65536;
                    self.rc[i as usize] = Op { code: R_SWITCH as i32 + 65536 * skip, a: x, b: (at as i32), c: (lo as i32) };
                }
                i = j;
                continue;
            }
            i += 1;
        }
    }

    /// Skips over the instructions that do nothing, and jumps to the
    /// first that does.
    fn finish(&mut self) {
        let n = self.n;
        let mut run: Vec<int> = Vec::new();
        let mut i: int = 0;
        while i <= n {
            run.push(0);
            i += 1;
        }
        i = n - 1;
        while i >= 0 {
            if self.rop_at(i) == R_NOP {
                let r = run[(i + 1) as usize] + 1;
                run[i as usize] = if r > 32000 { 32000 } else { r };
            }
            i -= 1;
        }
        i = 0;
        while i < n {
            let k = i as usize;
            let rop = self.rc[k].code % 256;
            self.rc[k].code += (65536 * run[(i + 1) as usize] as i32);
            let w = if (rop as int) == R_JUMP {
                1
            } else if (rop as int) == R_JF || (rop as int) == R_JT || (rop as int) == R_JF_KEEP || (rop as int) == R_JT_KEEP || (rop as int) == R_JNN_KEEP || (rop as int) == R_ITER_NEXT || (rop as int) == R_JNU {
                2
            } else if (rop as int) == R_CMP_JF || (rop as int) == R_LOCAL_NULL_JF || (rop as int) == R_CASE_INT_JT || (rop as int) == R_CASE_JT {
                3
            } else {
                0
            };
            if w == 1 {
                let t = self.rc[k].a;
                self.rc[k].a = t + (run[t as usize] as i32);
            } else if w == 2 {
                let t = self.rc[k].b;
                self.rc[k].b = t + (run[t as usize] as i32);
            } else if w == 3 {
                let t = self.rc[k].c;
                self.rc[k].c = t + (run[t as usize] as i32);
            }
            i += 1;
        }
        self.fuse_branches();
        self.switch_tables();
        // a jump to a compare-and-branch does the compare itself
        i = 0;
        while i < n {
            let k = i as usize;
            if ((self.rc[k].code % 256) as int) == R_JUMP {
                let t = self.rc[k].a;
                let tk = t as usize;
                if ((self.rc[tk].code % 256) as int) == R_CMP_JF {
                    let body = t + 1 + self.rc[tk].code / 65536;
                    if body < 32000 {
                        let sub = (self.rc[tk].code / 256) % 256;
                        let (x, y, exit) = (self.rc[tk].a, self.rc[tk].b, self.rc[tk].c);
                        self.rc[k] = Op { code: R_LOOP as i32 + 256 * sub + 65536 * body, a: x, b: y, c: exit };
                    }
                }
            }
            i += 1;
        }
        self.entry = run[0];
    }
}

/// Appends the register code of p to rcode and its constants to kvals,
/// and sets `p.rbase`, `p.fsize` and `p.rentry`; an error when the code is
/// not one this translator knows.
pub fn translate(p: &mut Proto, rcode: &mut Vec<Op>, kvals: &mut Vec<Val>, rtab: &mut Vec<int>) -> String {
    let mut t = Tr::new(p, kvals.len() as int, rtab.len() as int);
    t.translate();
    if !t.err.is_empty() {
        return t.err.clone();
    }
    let rb = rcode.len() as int;
    let mut i: usize = 0;
    while i < t.rc.len() {
        let o = t.rc[i];
        rcode.push(o);
        i += 1;
    }
    i = 0;
    while i < t.kv.len() {
        kvals.push(t.kv[i].clone());
        i += 1;
    }
    i = 0;
    while i < t.tab.len() {
        rtab.push(t.tab[i]);
        i += 1;
    }
    p.rbase = rb;
    p.fsize = p.nslots + t.maxd + 1;
    p.rentry = t.entry;
    p.rstate = 1;
    String::new()
}

/// The register code of a function as text, one instruction a line (for
/// looking at what the translation made).
pub fn dump(p: &Proto, rcode: &Vec<Op>) -> String {
    let mut out = format!("{} nslots {} fsize {} rbase {}\n", p.name, p.nslots, p.fsize, p.rbase);
    let n = p.code.len() as int;
    let mut i: int = 0;
    while i < n {
        let r = if p.rstate == 1 { rcode[(p.rbase + i) as usize] } else { Op { code: 0, a: 0, b: 0, c: 0 } };
        let w0 = r.code;
        out.push_str(format!("{:4} op{:3} {:>3} sub {:3} skip {:3} | {} {} {}  ({} {} {})\n", i, p.code[i as usize].code, w0 % 256, (w0 / 256) % 256, w0 / 65536, r.a, r.b, r.c, p.code[i as usize].a, p.code[i as usize].b, p.code[i as usize].c).as_str());
        i += 1;
    }
    out
}

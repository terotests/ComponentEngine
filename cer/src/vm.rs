// SPDX-License-Identifier: AGPL-3.0-or-later
//! The machine: the object heap, conversions, property access, calls, the
//! interpreter loop and a mark-and-sweep collector.

use ranger::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;

use crate::jsstr;
use crate::num::*;
use crate::ops::*;
use crate::proxy::*;
use crate::value::*;

// atoms interned first, in this order
pub const A_LENGTH: int = 0;
pub const A_PROTOTYPE: int = 1;
pub const A_CONSTRUCTOR: int = 2;
pub const A_NAME: int = 3;
pub const A_MESSAGE: int = 4;
pub const A_TOSTRING: int = 5;
pub const A_VALUEOF: int = 6;
pub const A_CALLEE: int = 7;
pub const A_ITERATOR: int = 8;
pub const A_NEXT: int = 9;
pub const A_DONE: int = 10;
pub const A_VALUE: int = 11;
pub const A_GET: int = 12;
pub const A_SET: int = 13;
pub const A_LASTINDEX: int = 14;
pub const A_INDEX: int = 15;
pub const A_INPUT: int = 16;
pub const A_PROTO: int = 17;
pub const A_STACK: int = 18;
pub const A_WRITABLE: int = 19;
pub const A_ENUMERABLE: int = 20;
pub const A_CONFIGURABLE: int = 21;
pub const A_RAW: int = 22;
pub const A_TOPRIMITIVE: int = 23;
pub const A_CAUSE: int = 24;
pub const A_GROUPS: int = 25;
pub const A_TOJSON: int = 26;
pub const A_HASINSTANCE: int = 27;
pub const A_ERRORS: int = 28;
pub const A_THEN: int = 29;
pub const A_SOURCE: int = 33;
pub const A_GLOBAL: int = 35;
pub const A_IGNORECASE: int = 36;
pub const A_MULTILINE: int = 37;
pub const A_DOTALL: int = 38;
pub const A_UNICODE: int = 39;
pub const A_STICKY: int = 40;

pub fn first_atoms() -> Vec<String> {
    let names = vec![
        "length",
        "prototype",
        "constructor",
        "name",
        "message",
        "toString",
        "valueOf",
        "callee",
        "@@iterator",
        "next",
        "done",
        "value",
        "get",
        "set",
        "lastIndex",
        "index",
        "input",
        "__proto__",
        "stack",
        "writable",
        "enumerable",
        "configurable",
        "raw",
        "@@toPrimitive",
        "cause",
        "groups",
        "toJSON",
        "@@hasInstance",
        "errors",
        "then",
        "globalThis",
        "",
        "join",
        "source",
        "flags",
        "global",
        "ignoreCase",
        "multiline",
        "dotAll",
        "unicode",
        "sticky",
    ];
    let mut out: Vec<String> = Vec::new();
    for n in names {
        out.push(String::from(n));
    }
    out
}

/// A host's synchronous answer to a script (__hostSync(text)): the embedder
/// sets one with Vm::set_host_sync, and the script waits for its text. The
/// engine gives the text through unchanged; what it means is the host's.
pub trait HostSync {
    fn call(&mut self, request: &str) -> String;
}

/// The host when the embedder set none: __hostSync throws.
pub struct NoHostSync {}

impl HostSync for NoHostSync {
    fn call(&mut self, _request: &str) -> String {
        String::new()
    }
}

pub struct Vm {
    pub objs: Vec<JsObj>,
    pub free_list: Vec<int>,
    /// the objects' Colds (value.rs); 0 is the empty one, never written
    pub colds: Vec<Cold>,
    pub free_colds: Vec<int>,
    /// the objects' elements (JsObj.elems): 0 is the empty one, never
    /// written
    pub evecs: Vec<Elems>,
    /// `evecs` no object holds
    pub efree: Vec<i32>,
    /// the objects whose count is 0 (JsObj.rc): garbage unless a root
    /// holds one (Vm::reclaim)
    pub zct: Vec<int>,
    /// the buffer the next reclaim's zct takes (Rust: no new vector)
    pub zct_spare: Vec<int>,
    /// reclaim runs when zct grows past this, at least zct_min (ZCT_MIN;
    /// tests make it 1: a reclaim at every safe point)
    pub zct_limit: int,
    pub zct_min: int,
    /// slots reclaim freed, reused after the next reclaim or collection
    pub freed: Vec<int>,
    /// objects in use: allocated and not yet freed
    pub live: int,
    /// reference counting frees objects (false: only the collector)
    pub rc_on: bool,
    /// tests: every reclaim checks against a full mark that it frees only
    /// unreachable objects, and every collection that no count was low;
    /// rc_errors counts what was wrong
    pub rc_check: bool,
    pub rc_errors: int,
    pub rc_log: Vec<String>,
    pub rc_freed: int,
    pub reclaims: int,
    /// object shapes (value.rs); 0 is the empty one
    pub shapes: Vec<Shape>,
    /// owned shapes of freed objects, for reuse
    pub free_shapes: Vec<int>,
    pub protos: Vec<Proto>,
    pub atoms: HashMap<String, int>,
    pub atom_names: Vec<String>,
    /// intern's cache: atom by a hash of the text, -1 for none
    pub atom_cache: Vec<int>,
    /// array index -> its atom, for the atoms made so far (a cache in front
    /// of format + intern): open addressing, keys stored as index + 1 (0 is
    /// empty), the atom beside each
    pub idx_keys: Vec<int>,
    /// index atoms made by index_atom that are not in `atoms` yet (intern
    /// finds them through the index table); atoms_sync puts them there
    pub idx_pending: Vec<int>,
    /// intern's own table over atom_names[0..fa_seen]: open addressing
    /// (atom ids, -1 empty) by str_hash, atom_hash[a] each name's hash (-1
    /// for an index atom left to the index table); new atoms go into
    /// `atoms` (the compilers' map) through idx_pending and atoms_sync
    pub fa_tab: Vec<int>,
    pub atom_hash: Vec<int>,
    pub fa_seen: int,
    pub fa_count: int,
    pub idx_vals: Vec<int>,
    pub idx_count: int,
    /// own properties removed so far (remove_own): a for-in over a plain
    /// object skips its per-key check while this stays the same
    pub del_count: int,
    /// atom_names up to here are in the idx table when they are indexes
    pub idx_seen: int,
    /// atom names as string values, made on first use (atom_val)
    pub atom_vals: Vec<Val>,
    /// the one-unit strings of ASCII, shared (unit_val), pinned like
    /// atom_vals
    pub char_vals: Vec<Val>,
    /// "undefined", "object", … as string values for typeof (type_val)
    pub type_vals: Vec<Val>,
    /// atoms that are symbols (their description is the name)
    pub symbol_atoms: HashMap<int, int>,
    /// what kind of key each atom is, found on first use (atom_kind): 0
    /// not yet, 1 a name, 2 an array index, 3 a symbol, 4 a private name
    pub atom_kinds: Vec<i32>,
    /// Object.prototype.toString's answers by class ("[object Array]"…),
    /// made on first use, and the @@toStringTag atom (-1 before)
    pub tostr_vals: Vec<Val>,
    pub a_tostringtag: int,
    pub stack: Vec<Val>,
    pub frames: Vec<Frame>,
    pub handlers: Vec<Handler>,
    pub global: int,
    pub object_proto: int,
    pub function_proto: int,
    pub array_proto: int,
    pub string_proto: int,
    pub number_proto: int,
    pub boolean_proto: int,
    pub symbol_proto: int,
    pub error_proto: int,
    pub type_error_proto: int,
    pub range_error_proto: int,
    pub reference_error_proto: int,
    pub syntax_error_proto: int,
    pub regexp_proto: int,
    pub date_proto: int,
    pub map_proto: int,
    pub set_proto: int,
    pub iter_proto: int,
    pub promise_proto: int,
    /// %IteratorPrototype%, %GeneratorPrototype%, the prototypes of
    /// generator and async functions
    pub iterator_proto: int,
    /// %MapIteratorPrototype%, %SetIteratorPrototype%, %StringIteratorPrototype%
    pub map_iter_proto: int,
    pub set_iter_proto: int,
    pub string_iter_proto: int,
    pub generator_proto: int,
    pub generator_function_proto: int,
    pub async_function_proto: int,
    /// set by OP_YIELD for the resumer: the frame yielded (not returned),
    /// the value is an iterator result already; the coroutine finished
    pub gen_yielded: bool,
    pub gen_raw: bool,
    pub gen_done: bool,
    /// the last OP_YIELD was an `await`
    pub gen_awaiting: bool,
    pub async_generator_proto: int,
    pub async_generator_function_proto: int,
    /// the prelude's CreateAsyncFromSyncIterator, for `for await`
    pub async_from_sync: int,
    /// the prelude's generic Array.prototype methods (an array by native
    /// id - NF_AP_PUSH), for receivers that are not plain arrays
    pub array_generic: int,
    /// per function (Proto index): 0 not looked at, 1 an ordinary
    /// comparator, 2 `(a, b) => a - b`, 3 `(a, b) => b - a` (sort_kind)
    pub cmp_kinds: Vec<i32>,
    /// the prelude's proxy hooks (an array), -1 before
    pub proxy_hooks: int,
    /// %ThrowTypeError%
    pub thrower: int,
    /// the getter/setter pair (both %ThrowTypeError%) every strict
    /// arguments object's `callee` shares, made on first use; -1 before
    pub callee_pair: int,
    /// the shape an arguments object gets (callee, then Symbol.iterator),
    /// sloppy [0] and strict [1]; -1 until the first one is made
    pub args_shapes: Vec<int>,
    /// the shape a regexp object gets (lastIndex, source and the flags);
    /// -1 until the first one is made
    pub regex_shape: int,
    /// the marker of a binding in its temporal dead zone
    pub tdz_obj: int,
    /// large BigInt values (objects) by their decimal text (weak), and
    /// BigInt.prototype
    pub bigints: HashMap<String, int>,
    pub bigint_proto: int,
    pub array_values_fn: int,
    pub roots: Vec<int>,
    pub throwing: bool,
    pub exc: Val,
    pub out: Vec<String>,
    /// __hostSync's host (Vm::set_host_sync); NoHostSync by default
    pub(crate) host_sync: Box<dyn HostSync>,
    pub(crate) host_sync_set: bool,
    pub echo: bool,
    /// run frames on the register machine (rvm.rs)
    pub regs: bool,
    /// compile with pcomp.rs (tokens to bytecode in one pass, no tree)
    pub pcomp: bool,
    /// the register code of every translated function (Proto.rbase) and
    /// the constants it reads (a register x < 0 is kvals[-1 - x])
    pub rcode: Vec<Op>,
    pub kvals: Vec<Val>,
    /// the register code's switch tables
    pub rtab: Vec<int>,
    /// the string table: Val::Str(h) is strs[h] (0 is ""); sup[h] >= 0
    /// when an append in place moved h's buffer on to that newer handle
    /// (h is then its first slen[h] bytes); sflag 1 pinned (constants,
    /// atoms), 2 free. collect() sweeps it (mark_strs).
    pub strs: Vec<SEnt>,
    pub sfree: Vec<int>,
    /// freed by the last collection, reused after the next one
    pub sfreed: Vec<int>,
    /// handles whose count went to 0 (or that are new): reclaim() frees
    /// those no root holds
    pub szct: Vec<int>,
    pub szct_spare: Vec<int>,
    /// szct's length that makes a reclaim due
    pub szct_limit: int,
    /// strings reclaim() freed (statistics)
    pub sfree_count: int,
    /// strings made since the last collection, and how many make one due
    pub snew: int,
    pub slimit: int,
    pub slimit_min: int,
    /// bytes of strings made since the last collection, and the count
    /// that makes one due: big strings die only when the table is swept
    pub sbytes: int,
    /// argument vectors for native calls, reused (Vm::call_op)
    pub arg_pool: Vec<Vec<Val>>,
    pub sbytes_limit: int,
    pub sempty: Rc<String>,
    /// prototype-chain caches of the register code's reads (rvm.rs
    /// pic_get): rpic[i] is rcode[i]'s entry in pic (PIC_W ints), -1 none;
    /// an entry holds while shape_epoch, which every change of an owned
    /// shape moves, stays
    pub rpic: Vec<i32>,
    /// per register site (rbase + pc): a second own-slot inline cache
    /// (shape * 65536 + slot) for a property read or write that sees two
    /// shapes; the site's c and this swap on a hit
    pub ic2: Vec<i32>,
    /// R_GET_METHOD's method cache (rcode c <= -100000, entry -100000 - c):
    /// receiver shape, shape epoch, prototype, its shape, slot; 5 ints each
    pub mcache: Vec<i32>,
    /// regexp_exec_in: a new array's shape and the one index, input and
    /// groups give it
    pub rx_shapes: Vec<i32>,
    pub gn_memo: Vec<i32>,
    /// array_iter_plain's memo: an array's shape, the shape epoch, its
    /// prototype, the prototype's shape and its Symbol.iterator slot
    pub ai_memo: Vec<i32>,
    /// JSON.stringify's plans for ordinary objects (json_plan): (shape,
    /// start, len) triples, start -1 when the shape has no plan; an entry's
    /// own keys in output order are js_atoms/js_slots[start..start+len] and
    /// js_kq holds each one quoted. Kept while shape_epoch is js_epoch.
    pub js_plan: Vec<i32>,
    pub js_slots: Vec<i32>,
    pub js_atoms: Vec<i32>,
    pub js_kq: Vec<String>,
    pub js_epoch: int,
    pub js_depth: int,
    pub js_pshape: int,
    pub pic: Vec<i32>,
    pub shape_epoch: int,
    /// instanceof: Function.prototype's and Object.prototype's shapes and
    /// shape_epoch when neither had @@hasInstance (-1 none yet)
    pub hi_fp: int,
    pub hi_op: int,
    pub hi_epoch: int,
    /// instanceof_fast's memo: a function shape (not owned, so it never
    /// changes) without @@hasInstance, its data `prototype` slot (-1 none)
    pub io_shape: int,
    pub io_slot: int,
    /// the atom of "apply" (fwd_target), -1 before its first use
    pub a_apply: int,
    /// a collection or reclaim is due at the next safe point (gc_due)
    pub gc_flag: bool,
    /// where in pic the last pic_get hit or filled
    pub pic_way: int,
    pub alloc_count: int,
    pub gc_threshold: int,
    /// objects made since the last collection, freed or not, and how many
    /// make one due: reference counting frees no cycle, so they wait for a
    /// collection that the allocation volume, not the net growth, brings
    pub made: int,
    pub made_limit: int,
    pub native_depth: int,
    /// time slices (Engine::set_slice): the slice's length in ms, 0 for
    /// none; when it began; back jumps left before the clock is read again
    pub slice_ms: double,
    pub slice_t0: double,
    pub ticks: int,
    /// the script stopped at a branch when its slice ran out, its frames
    /// kept (Engine::resume goes on)
    pub suspended: bool,
    /// slices are on for this run: set by Engine::run / resume only, so a
    /// host call (call_top, jobs) never stops half way
    pub slicing: bool,
    /// the sampling profiler (cerxes CERXES_PROF=1): with slicing on, every
    /// tick charges the time since the last one to the running function
    /// (prof_self) and once to each function on the stack (prof_incl), in
    /// ms by proto index; no tick ever ends a slice then
    pub prof_on: bool,
    pub prof_t: double,
    pub prof_self: Vec<double>,
    pub prof_incl: Vec<double>,
    pub prof_mark: Vec<int>,
    pub prof_n: int,
    /// the baseline JIT (wjit.rs) runs hot loops: a host that can run its
    /// code sets it
    pub jit_on: bool,
    /// back edges a function takes before it is compiled
    pub jit_min: int,
    /// by proto: the compiled function (a table index), -1 not yet, -2 none
    pub jit_fn: Vec<int>,
    /// the machine's key to its compiled code state (wjit.rs), 0 none yet
    pub jit_id: int,
    /// by proto: back edges taken while not compiled
    pub jit_hot: Vec<int>,
    /// by rcode index of a loop head: entries that left at once (wjit.rs)
    pub jit_miss: Vec<int>,
    /// by rcode index: the pc after a call in compiled code (wjit.rs)
    pub jit_res: Vec<bool>,
    /// by rcode index: the property site's cache in jit_caches, -1 none;
    /// each cache keeps its buffer (compiled code has its address)
    pub jit_cache_at: Vec<int>,
    pub jit_caches: Vec<Vec<i32>>,
    /// by proto, 8 words: what a compiled caller needs to call it directly
    /// (wjit.rs callx: proto, table index, nparams, nslots, fsize, entry,
    /// strict), table index -1 when it cannot be
    pub jit_calls: Vec<i32>,
    /// functions compiled from the prelude (protos below this) are built-ins
    pub prelude_protos: int,
    pub temp_roots: Vec<Val>,
    /// some object has a setter or a read-only property: writes that miss
    /// look along the prototype chain
    pub any_setter: bool,
    /// an element or index property was put on Array.prototype or
    /// Object.prototype: holes and reads past an array's end look there
    pub idx_protos: bool,
    /// a native's frame depth whose failed writes throw as in strict code
    /// (Object.assign's Set(.., true)); -1 for none
    pub strict_at: int,
    /// the last class id unit::link gave private keys
    pub priv_seq: int,
    /// the atoms some object has as a setter or a read-only property (by
    /// atom), and whether a proxy was made: a write that adds a property
    /// looks along the chain only for these
    pub setter_atoms: Vec<bool>,
    pub proxy_made: bool,
    pub rng: int,
    pub gc_runs: int,
    pub symbols: Vec<int>,
    pub symbol_registry: HashMap<String, int>,
    /// promise jobs, four values each
    pub jobs: Vec<Val>,
    /// the next job's index in `jobs` (run_jobs takes them from the front
    /// without moving the rest down; the ones taken are undefined)
    pub job_head: int,
    pub regexes: Vec<crate::regex::Regex>,
    pub regex_cache: HashMap<String, int>,
    /// a regex's `source` as shown (make_regexp), by its index
    pub regex_shown: Vec<Val>,
    /// the last long string indexed (str_at, str_len): a loop over a
    /// string's units does not scan it again for every unit
    pub sc_s: Rc<String>,
    pub sc_ascii: bool,
    pub sc_len: int,
    pub sc_units: Vec<int>,
    /// the string RegExp.prototype.exec matched last, as UTF-16 units: a
    /// loop of exec calls (matchAll, a global regexp's exec loop) over one
    /// string converts it once
    pub rx_s: Rc<String>,
    pub rx_units: Vec<int>,
    /// String.prototype.normalize's tables, decoded on first use
    pub norm: crate::normalize::Norm,
    /// Intl's collation tables, decoded on first use
    pub intl: crate::intl::Intl,
}

/// The same string: the same one first (Rust's Rc, Go's string compare
/// sees the same bytes), then by content.
fn same_str(a: &Rc<String>, b: &Rc<String>) -> bool {
    let r: bool = ranger::native!(
        rust: { Rc::ptr_eq(a, b) || a.as_str() == b.as_str() },
        es6: "({a} === {b})",
        go: "({a} == {b})",
        cpp: "({a} == {b})",
    );
    r
}

pub fn is_obj(v: &Val) -> bool {
    match v {
        Val::Obj(_) => true,
        _ => false,
    }
}

/// A value that reference counts track (an object or a string).
#[inline]
pub fn is_heap(v: &Val) -> bool {
    match v {
        Val::Obj(_) => true,
        Val::Str(_) => true,
        _ => false,
    }
}

pub fn obj_of(v: &Val) -> int {
    match v {
        Val::Obj(o) => *o,
        _ => -1,
    }
}

/// The string handle of `v`, 0 (the empty string, never counted) when it
/// is no string.
pub fn str_of(v: &Val) -> int {
    match v {
        Val::Str(h) => *h,
        _ => 0,
    }
}

/// `===` (strict) or `==` of two values that are objects, null or
/// undefined: 1 or 0; -1 when either is anything else.
pub fn quick_eq(a: &Val, b: &Val, strict: bool) -> int {
    match (a, b) {
        (Val::Obj(x), Val::Obj(y)) => {
            if x == y {
                1
            } else if strict {
                0
            } else {
                // a Symbol object == its symbol: loose_equals decides
                -1
            }
        }
        (Val::Null, Val::Null) | (Val::Undef, Val::Undef) => 1,
        (Val::Null, Val::Undef) | (Val::Undef, Val::Null) => {
            if strict {
                0
            } else {
                1
            }
        }
        (Val::Obj(_), Val::Null) | (Val::Obj(_), Val::Undef) | (Val::Null, Val::Obj(_)) | (Val::Undef, Val::Obj(_)) => 0,
        _ => -1,
    }
}

pub fn truthy(v: &Val) -> bool {
    match v {
        Val::Undef => false,
        Val::Null => false,
        Val::Bool(b) => *b,
        Val::Num(n) => *n != 0.0 && *n == *n,
        Val::Str(h) => *h != 0,
        Val::Obj(_) => true,
        Val::Big(n) => *n != 0.0,
    }
}

impl Vm {
    /// ToBoolean (0n is a Val::Big, `truthy` has it).
    #[inline]
    pub fn truthy_v(&self, v: &Val) -> bool {
        truthy(v)
    }

    pub fn new() -> Vm {
        let mut vm = Vm {
            objs: Vec::new(),
            free_list: Vec::new(),
            colds: Vec::new(),
            free_colds: Vec::new(),
            evecs: Vec::new(),
            efree: Vec::new(),
            zct: Vec::new(),
            zct_spare: Vec::new(),
            zct_limit: ZCT_MIN,
            zct_min: ZCT_MIN,
            freed: Vec::new(),
            live: 0,
            rc_on: true,
            rc_check: false,
            rc_errors: 0,
            rc_log: Vec::new(),
            rc_freed: 0,
            reclaims: 0,
            shapes: Vec::new(),
            free_shapes: Vec::new(),
            protos: Vec::new(),
            atoms: HashMap::new(),
            atom_cache: Vec::new(),
            idx_keys: Vec::new(),
            idx_pending: Vec::new(),
            fa_tab: Vec::new(),
            atom_hash: Vec::new(),
            fa_seen: 0,
            fa_count: 0,
            idx_vals: Vec::new(),
            idx_count: 0,
            del_count: 0,
            idx_seen: 0,
            atom_names: Vec::new(),
            atom_vals: Vec::new(),
            char_vals: Vec::new(),
            atom_kinds: Vec::new(),
            tostr_vals: Vec::new(),
            a_tostringtag: -1,
            type_vals: Vec::new(),
            symbol_atoms: HashMap::new(),
            stack: Vec::new(),
            frames: Vec::new(),
            handlers: Vec::new(),
            global: -1,
            object_proto: -1,
            function_proto: -1,
            array_proto: -1,
            string_proto: -1,
            number_proto: -1,
            boolean_proto: -1,
            symbol_proto: -1,
            error_proto: -1,
            type_error_proto: -1,
            range_error_proto: -1,
            reference_error_proto: -1,
            syntax_error_proto: -1,
            regexp_proto: -1,
            date_proto: -1,
            map_proto: -1,
            set_proto: -1,
            iter_proto: -1,
            promise_proto: -1,
            iterator_proto: -1,
            map_iter_proto: -1,
            set_iter_proto: -1,
            string_iter_proto: -1,
            generator_proto: -1,
            generator_function_proto: -1,
            async_function_proto: -1,
            gen_yielded: false,
            gen_raw: false,
            gen_done: false,
            gen_awaiting: false,
            async_generator_proto: -1,
            async_generator_function_proto: -1,
            async_from_sync: -1,
            array_generic: -1,
            cmp_kinds: Vec::new(),
            proxy_hooks: -1,
            thrower: -1,
            callee_pair: -1,
            args_shapes: vec![-1, -1],
            regex_shape: -1,
            tdz_obj: -1,
            bigints: HashMap::new(),
            bigint_proto: -1,
            array_values_fn: -1,
            roots: Vec::new(),
            throwing: false,
            exc: Val::Undef,
            out: Vec::new(),
            echo: false,
            regs: false,
            pcomp: false,
            rcode: Vec::new(),
            kvals: Vec::new(),
            rtab: Vec::new(),
            strs: Vec::new(),
            sfree: Vec::new(),
            sfreed: Vec::new(),
            szct: Vec::new(),
            szct_spare: Vec::new(),
            sfree_count: 0,
            szct_limit: ZCT_MIN,
            snew: 0,
            slimit: SLIMIT_MIN,
            slimit_min: SLIMIT_MIN,
            sbytes: 0,
            arg_pool: Vec::new(),
            sbytes_limit: SBYTES_MIN,
            sempty: Rc::new(String::new()),
            rpic: Vec::new(),
            ic2: Vec::new(),
            mcache: Vec::new(),
            rx_shapes: Vec::new(),
            gn_memo: Vec::new(),
            ai_memo: Vec::new(),
            js_plan: Vec::new(),
            js_slots: Vec::new(),
            js_atoms: Vec::new(),
            js_kq: Vec::new(),
            js_epoch: -1,
            js_depth: 0,
            js_pshape: -1,
            pic: Vec::new(),
            shape_epoch: 0,
            hi_fp: -1,
            hi_op: -1,
            hi_epoch: -1,
            io_shape: -1,
            io_slot: -1,
            a_apply: -1,
            gc_flag: false,
            pic_way: 0,
            alloc_count: 0,
            gc_threshold: 200000,
            made: 0,
            made_limit: MADE_MIN,
            native_depth: 0,
            slice_ms: 0.0,
            slice_t0: 0.0,
            ticks: SLICE_TICKS,
            suspended: false,
            slicing: false,
            prof_on: false,
            prof_t: 0.0,
            prof_self: Vec::new(),
            prof_incl: Vec::new(),
            prof_mark: Vec::new(),
            prof_n: 0,
            jit_on: false,
            jit_min: 64,
            jit_fn: Vec::new(),
            jit_id: 0,
            jit_hot: Vec::new(),
            jit_miss: Vec::new(),
            jit_res: Vec::new(),
            jit_cache_at: Vec::new(),
            jit_caches: Vec::new(),
            jit_calls: Vec::new(),
            prelude_protos: 0,
            temp_roots: Vec::new(),
            any_setter: false,
            idx_protos: false,
            strict_at: -1,
            priv_seq: 0,
            setter_atoms: Vec::new(),
            proxy_made: false,
            rng: 42,
            gc_runs: 0,
            symbols: Vec::new(),
            symbol_registry: HashMap::new(),
            jobs: Vec::new(),
            job_head: 0,
            regexes: Vec::new(),
            regex_cache: HashMap::new(),
            regex_shown: Vec::new(),
            sc_s: Rc::new(String::new()),
            sc_ascii: true,
            sc_len: 0,
            sc_units: Vec::new(),
            rx_s: Rc::new(String::new()),
            rx_units: Vec::new(),
            norm: crate::normalize::Norm::new(),
            intl: crate::intl::Intl::new(),
            host_sync: Box::new(NoHostSync {}),
            host_sync_set: false,
        };
        vm.shapes.push(Shape::new());
        vm.colds.push(Cold::new());
        vm.evecs.push(elems_of(Vec::new()));
        for n in first_atoms() {
            vm.intern(n.as_str());
        }
        vm
    }

    // ---- string units (a short string is scanned each time)

    fn str_cache(&mut self, s: &Rc<String>) {
        if self.sc_s.as_bytes().len() == s.as_bytes().len() && same_str(&self.sc_s, s) {
            // the same text in another box: that box from now on, so the
            // next calls with it are a pointer compare, not a byte compare
            self.sc_s = s.clone();
            return;
        }
        self.sc_s = s.clone();
        self.sc_ascii = jsstr::is_ascii(s.as_str());
        if self.sc_ascii {
            self.sc_units = Vec::new();
            self.sc_len = s.as_bytes().len() as int;
        } else {
            self.sc_units = jsstr::units(s.as_str());
            self.sc_len = self.sc_units.len() as int;
        }
    }

    /// Fills `rx_units` with `s`'s UTF-16 units, unless it holds them.
    pub fn rx_cache(&mut self, s: &Rc<String>) {
        if self.rx_s.as_bytes().len() == s.as_bytes().len() && same_str(&self.rx_s, s) {
            self.rx_s = s.clone();
            return;
        }
        self.rx_s = s.clone();
        self.rx_units = jsstr::units(s.as_str());
    }

    /// `s`'s UTF-16 units out of the rx cache (computed when it holds
    /// others), for a caller that may run code that uses the cache; rx_put
    /// gives them back.
    pub fn rx_take(&mut self, s: &Rc<String>) -> Vec<int> {
        self.rx_cache(s);
        self.rx_s = Rc::new(String::new());
        let mut spare: Vec<int> = Vec::new();
        take_ints(&mut self.rx_units, &mut spare)
    }

    pub fn rx_put(&mut self, s: &Rc<String>, units: Vec<int>) {
        self.rx_s = s.clone();
        self.rx_units = units;
    }

    /// The UTF-16 unit at `i`, -1 out of range.
    pub fn str_at(&mut self, s: &Rc<String>, i: int) -> int {
        if s.as_bytes().len() < 256 {
            return jsstr::at(s.as_str(), i);
        }
        self.str_cache(s);
        if i < 0 || i >= self.sc_len {
            return -1;
        }
        if self.sc_ascii {
            return s.as_bytes()[i as usize] as int;
        }
        self.sc_units[i as usize]
    }

    /// The length in UTF-16 units.
    pub fn str_len(&mut self, s: &Rc<String>) -> int {
        if s.as_bytes().len() < 256 {
            return jsstr::len(s.as_str());
        }
        self.str_cache(s);
        self.sc_len
    }

    /// Units [a, b) clamped to the string.
    pub fn str_slice(&mut self, s: &Rc<String>, a: int, b: int) -> String {
        if s.as_bytes().len() < 256 || a >= b {
            return jsstr::slice(s.as_str(), a, b);
        }
        self.str_cache(s);
        let n = self.sc_len;
        let x = if a < 0 { 0 } else if a > n { n } else { a };
        let y = if b < 0 { 0 } else if b > n { n } else { b };
        if x >= y {
            return String::new();
        }
        if self.sc_ascii {
            return jsstr::byte_slice(s.as_str(), x, y);
        }
        jsstr::from_units(&self.sc_units, x, y)
    }

    /// The first `pat` at or after `from`, -1 when none.
    pub fn str_index_of(&mut self, s: &Rc<String>, pat: &str, from: int) -> int {
        if s.as_bytes().len() < 256 {
            return jsstr::index_of(s.as_str(), pat, from);
        }
        self.str_cache(s);
        if self.sc_ascii && jsstr::is_ascii(pat) {
            return jsstr::byte_index_of(s.as_str(), pat, from);
        }
        if self.sc_ascii {
            return -1;
        }
        let p = jsstr::units(pat);
        let n = self.sc_len;
        let m = p.len() as int;
        let mut i = if from < 0 { 0 } else { from };
        if m == 0 {
            return if i > n { n } else { i };
        }
        while i + m <= n {
            let mut k: int = 0;
            while k < m && self.sc_units[(i + k) as usize] == p[k as usize] {
                k += 1;
            }
            if k == m {
                return i;
            }
            i += 1;
        }
        -1
    }

    /// Notes that some object has `atom` as a setter or read-only.
    pub fn mark_setter(&mut self, atom: int) {
        if atom < 0 {
            return;
        }
        while self.setter_atoms.len() <= atom as usize {
            self.setter_atoms.push(false);
        }
        self.setter_atoms[atom as usize] = true;
    }

    pub fn setter_atom(&self, atom: int) -> bool {
        atom >= 0 && (atom as usize) < self.setter_atoms.len() && self.setter_atoms[atom as usize]
    }

    /// add, noting a setter or read-only property for mark_setter.
    // ---- own properties, through the object's shape


    /// The slot inline cache `c` names for own property `atom` of `o`,
    /// -1 on a miss. A cache is shape * 65536 + slot for a shared shape,
    /// which never changes, and the slot alone for an owned one, checked
    /// against its key. `plain`: a writable data property; else any data
    /// property.
    #[inline(always)]
    pub fn ic_slot(&self, o: int, atom: int, c: int, plain: bool) -> int {
        let sh = self.objs[o as usize].shape as int;
        if c >= 65536 {
            return if (c >> 16) == sh { c & 65535 } else { -1 };
        }
        if c >= 0 && c < self.shapes[sh as usize].nkeys {
            let s = &self.shapes[self.holder(sh)];
            if s.keys[c as usize] == atom {
                let a = s.attrs[c as usize];
                let bad = if plain { P_READONLY | P_ACCESSOR } else { P_ACCESSOR };
                if (a & bad) == 0 {
                    return c;
                }
            }
        }
        -1
    }

    /// The inline cache for slot `slot` of `o` (an Op field: 32 bits).
    #[inline]
    pub fn ic_of(&self, o: int, slot: int) -> int {
        let sh = self.objs[o as usize].shape as int;
        if self.shapes[sh as usize].owned || slot >= 65536 || sh >= 32768 {
            return slot;
        }
        sh * 65536 + slot
    }

    /// The slot of own data property `atom` of `o` for the get op before
    /// `pc`, filling its cache; -1 when there is none.
    #[inline]
    pub fn ic_get(&mut self, o: int, atom: int, c: int, pi: usize, pc: int) -> int {
        let hit = self.ic_slot(o, atom, c, false);
        if hit >= 0 {
            return hit;
        }
        let slot = self.find(o, atom);
        if slot < 0 || (self.attr_at(o, slot) & P_ACCESSOR) != 0 {
            return -1;
        }
        let nc = self.ic_of(o, slot);
        self.protos[pi].code[(pc - 1) as usize].c = (nc as i32);
        slot
    }

    /// `name = v` for a global (`create` 1 when a missing one is made even
    /// in strict code); a writable data property's slot goes to the cache
    /// of the op before `pc`.
    pub fn set_global(&mut self, atom: int, v: Val, create: int, pi: usize, pc: int) {
        let g = self.global;
        let slot = self.find(g, atom);
        if slot >= 0 && (self.attr_at(g, slot) & (P_READONLY | P_ACCESSOR)) == 0 {
            let nc = self.ic_of(g, slot);
            self.protos[pi].code[(pc - 1) as usize].c = (nc as i32);
            self.put_slot(g, slot, v);
        } else if slot < 0 && create == 0 && self.strict_now() && !self.has_atom(g, atom) {
            // strict code does not create globals by assignment
            let n = self.atom_str(atom);
            self.throw_ref(format!("{} is not defined", n).as_str());
        } else {
            self.set_obj(g, atom, v);
        }
    }

    /// ToNumeric of `v` (a BigInt stays).
    pub fn to_numeric(&mut self, v: Val) -> Val {
        if let Val::Num(_) = v {
            return v;
        }
        if self.class_of(&v) == C_BIGINT {
            return v;
        }
        let n = self.to_number(&v);
        Val::Num(n)
    }

    /// -v (unary minus), BigInts too.
    pub fn neg_val(&mut self, v: Val) -> Val {
        let p = if is_obj(&v) { self.to_primitive(&v, "number") } else { v };
        if self.throwing {
            return Val::Undef;
        }
        if self.class_of(&p) == C_BIGINT {
            let t = self.big_text(&p);
            let r = crate::bigint::negate(t.as_str());
            return self.bigint_val(r.as_str());
        }
        let n = self.to_number(&p);
        Val::Num(-n)
    }

    /// `v` + d (d 1 or -1) for ++ / --, BigInts too.
    pub fn inc_val(&mut self, v: Val, d: double) -> Val {
        if let Val::Big(n) = v {
            return self.big_step(n, d);
        }
        if self.class_of(&v) == C_BIGINT {
            let t = self.big_text(&v);
            let r = if d > 0.0 { crate::bigint::add(t.as_str(), "1") } else { crate::bigint::sub(t.as_str(), "1") };
            return self.bigint_val(r.as_str());
        }
        let n = match v {
            Val::Num(x) => x,
            _ => self.to_number(&v),
        };
        Val::Num(n + d)
    }

    /// As ic_get for a writable data property, for a set.
    #[inline]
    pub fn ic_set(&mut self, o: int, atom: int, c: int, pi: usize, pc: int) -> int {
        let hit = self.ic_slot(o, atom, c, true);
        if hit >= 0 {
            return hit;
        }
        let slot = self.find(o, atom);
        if slot < 0 || (self.attr_at(o, slot) & (P_READONLY | P_ACCESSOR)) != 0 {
            return -1;
        }
        let nc = self.ic_of(o, slot);
        self.protos[pi].code[(pc - 1) as usize].c = (nc as i32);
        slot
    }


    /// The shape that holds shape `sh`'s keys (Shape.kref).
    #[inline(always)]
    pub fn holder(&self, sh: int) -> usize {
        self.shapes[sh as usize].kref as usize
    }

    /// The slot of own property `atom` of `o`, -1 when absent.
    #[inline(always)]
    pub fn find(&self, o: int, atom: int) -> int {
        let sh = self.objs[o as usize].shape as int;
        self.shapes[self.holder(sh)].find(atom, self.shapes[sh as usize].nkeys)
    }

    #[inline]
    pub fn nkeys(&self, o: int) -> int {
        self.shapes[self.objs[o as usize].shape as usize].nkeys
    }

    #[inline]
    pub fn key_at(&self, o: int, slot: int) -> int {
        self.shapes[self.holder(self.objs[o as usize].shape as int)].keys[slot as usize]
    }

    #[inline]
    pub fn attr_at(&self, o: int, slot: int) -> int {
        self.shapes[self.holder(self.objs[o as usize].shape as int)].attrs[slot as usize]
    }

    /// The own keys of `o` in order.
    pub fn keys_of(&self, o: int) -> Vec<int> {
        self.shape_keys(self.objs[o as usize].shape as int)
    }

    /// Shape `sh`'s keys in order.
    pub fn shape_keys(&self, sh: int) -> Vec<int> {
        let n = self.shapes[sh as usize].nkeys as usize;
        let h = &self.shapes[self.holder(sh)];
        let mut out: Vec<int> = Vec::with_capacity(n);
        let mut i: usize = 0;
        while i < n {
            out.push(h.keys[i]);
            i += 1;
        }
        out
    }

    /// Shape `sh`'s attributes in order.
    pub fn shape_attrs(&self, sh: int) -> Vec<int> {
        let n = self.shapes[sh as usize].nkeys as usize;
        let h = &self.shapes[self.holder(sh)];
        let mut out: Vec<int> = Vec::with_capacity(n);
        let mut i: usize = 0;
        while i < n {
            out.push(h.attrs[i]);
            i += 1;
        }
        out
    }

    /// Adds own property `atom` (absent) to `o`; its slot.
    pub fn add_own(&mut self, o: int, atom: int, v: Val, attr: int) -> int {
        let sh = self.objs[o as usize].shape as int;
        if self.shapes[sh as usize].owned {
            // the chain caches learn no dictionary receiver (pic_learn,
            // wjit cache_fill): only a prototype's new key stales them
            if self.objs[o as usize].is_proto {
                self.shape_epoch += 1;
            }
            self.push_val(o, v);
            self.shapes[sh as usize].nkeys += 1;
            return self.shapes[sh as usize].push(atom, attr);
        }
        let slot = self.shapes[sh as usize].nkeys;
        let mut next: int = -1;
        {
            let s = &self.shapes[sh as usize];
            let mut i: usize = 0;
            while i < s.t_atom.len() {
                if s.t_atom[i] == atom && s.t_attr[i] == attr {
                    next = s.t_to[i];
                    break;
                }
                i += 1;
            }
            if next < 0 && !s.t_map.is_empty() {
                next = match s.t_map.get(&(atom * 32 + attr)) {
                    Some(t) => *t,
                    None => -1,
                };
            }
        }
        if next < 0 {
            if slot >= SHAPE_MAX {
                // a dictionary
                self.own_shape(o);
                self.shape_epoch += 1;
                let sh2 = self.objs[o as usize].shape as int;
                self.push_val(o, v);
                self.shapes[sh2 as usize].nkeys += 1;
                return self.shapes[sh2 as usize].push(atom, attr);
            }
            let h = self.holder(sh);
            next = self.shapes.len() as int;
            let mut ns = Shape::new();
            if (self.shapes[h].keys.len() as int) == slot {
                // the chain's newest shape: its holder gets the key
                self.shapes[h].push(atom, attr);
                ns.kref = h as int;
            } else {
                // a branch: a holder of its own
                ns = self.shapes[h].copy(slot);
                ns.owned = false;
                ns.push(atom, attr);
                ns.kref = next;
            }
            ns.nkeys = slot + 1;
            self.shapes.push(ns);
            let s = &mut self.shapes[sh as usize];
            if (s.t_atom.len() as int) < T_LINEAR {
                s.t_atom.push(atom);
                s.t_attr.push(attr);
                s.t_to.push(next);
            } else {
                s.t_map.insert(atom * 32 + attr, next);
            }
        }
        self.objs[o as usize].shape = next as i32;
        // the chain's holder has as many keys as the most an object on the
        // chain has got: the values get room for that many at once
        let most = self.shapes[self.holder(next)].keys.len();
        slot_room(&mut self.objs[o as usize], most);
        self.push_val(o, v);
        slot
    }

    /// Gives `o` an owned copy of its shape, to change in place.
    pub fn own_shape(&mut self, o: int) {
        let sh = self.objs[o as usize].shape as int;
        if self.shapes[sh as usize].owned {
            return;
        }
        let n = self.shapes[sh as usize].nkeys;
        let ns = self.shapes[self.holder(sh)].copy(n);
        let id = if self.free_shapes.is_empty() {
            let i = self.shapes.len() as int;
            let mut ns = ns;
            ns.kref = i;
            self.shapes.push(ns);
            i
        } else {
            let i = self.free_shapes.pop().unwrap();
            let mut ns = ns;
            ns.kref = i;
            self.shapes[i as usize] = ns;
            self.shape_epoch += 1;
            i
        };
        self.objs[o as usize].shape = id as i32;
    }

    pub fn set_attr(&mut self, o: int, slot: int, attr: int) {
        if self.attr_at(o, slot) == attr {
            return;
        }
        self.own_shape(o);
        self.shape_epoch += 1;
        let sh = self.objs[o as usize].shape as int;
        self.shapes[sh as usize].attrs[slot as usize] = attr;
    }

    pub fn remove_own(&mut self, o: int, slot: int) {
        self.del_count += 1;
        self.own_shape(o);
        self.shape_epoch += 1;
        let sh = self.objs[o as usize].shape as int;
        if self.shapes[sh as usize].nkeys > TOMB_AT {
            // a big dictionary: a tombstone, not every later key and
            // value moved down; compacted once half are tombstones
            let tomb = self.intern(TOMB_NAME);
            self.shapes[sh as usize].bury(slot, tomb);
            self.put_slot(o, slot, Val::Undef);
            if self.shapes[sh as usize].ndead * 2 > self.shapes[sh as usize].nkeys {
                self.compact_dict(o);
            }
            return;
        }
        self.shapes[sh as usize].remove(slot);
        self.remove_val(o, slot);
    }

    /// `o`'s dictionary without its tombstones (remove_own), and before
    /// anything that copies its keys and values as they are.
    pub fn compact_dict(&mut self, o: int) {
        let sh = self.objs[o as usize].shape as int;
        if !self.shapes[sh as usize].owned || self.shapes[sh as usize].ndead == 0 {
            return;
        }
        let tomb = self.intern(TOMB_NAME);
        let mut keep: Vec<bool> = Vec::new();
        for k in self.shapes[sh as usize].keys.iter() {
            keep.push(*k != tomb);
        }
        self.shapes[sh as usize].compact(&keep);
        self.objs[o as usize].retain_slots(&keep);
        self.shape_epoch += 1;
    }

    /// Puts `o` back on the empty shape, freeing an owned one.
    #[inline]
    pub fn free_shape(&mut self, o: int) {
        let sh = self.objs[o as usize].shape as int;
        if self.shapes[sh as usize].owned {
            self.shapes[sh as usize] = Shape::new();
            self.free_shapes.push(sh);
            self.shape_epoch += 1;
        }
        self.objs[o as usize].shape = 0;
    }

    pub fn add_prop(&mut self, o: int, atom: int, v: Val, attr: int) -> int {
        if (attr & (P_READONLY | P_ACCESSOR)) != 0 {
            self.mark_setter(atom);
        }
        self.add_own(o, atom, v, attr)
    }

    /// setter_atoms and proxy_made from the heap (a restored snapshot).
    pub fn find_setters(&mut self) {
        self.setter_atoms = Vec::new();
        self.proxy_made = false;
        let mut i: usize = 0;
        while i < self.objs.len() {
            if !self.objs[i].free {
                if self.objs[i].class == C_PROXY {
                    self.proxy_made = true;
                }
                // is_proto is not in a snapshot
                let p = self.objs[i].proto;
                if p >= 0 {
                    self.objs[p as usize].is_proto = true;
                }
            }
            i += 1;
        }
        // every shape's, used or not (a superset is safe)
        let mut si: usize = 0;
        while si < self.shapes.len() {
            let mut k: usize = 0;
            // a holder's keys, the whole chain's
            while self.shapes[si].kref == si as int && k < self.shapes[si].keys.len() {
                if (self.shapes[si].attrs[k] & (P_READONLY | P_ACCESSOR)) != 0 {
                    let a = self.shapes[si].keys[k];
                    self.mark_setter(a);
                }
                k += 1;
            }
            si += 1;
        }
    }

    // ---- the string table

    /// An empty string table (handle 0 is "").
    pub fn reset_strs(&mut self) {
        self.strs = Vec::new();
        self.sfree = Vec::new();
        self.sfreed = Vec::new();
        self.szct = Vec::new();
        self.snew = 0;
        self.slimit = self.slimit_min;
        self.sbytes = 0;
        self.sbytes_limit = SBYTES_MIN;
        let e = self.sempty.clone();
        self.strs.push(SEnt::new(e, 1));
    }

    fn new_handle(&mut self, s: Rc<String>, flag: int) -> int {
        let h = self.take_handle(s.as_bytes().len() as int, flag);
        self.strs[h as usize].s = s;
        h
    }

    /// A string handle for `n` bytes of text, its slot holding the box it
    /// had (a reused one) or the empty string: the caller puts the text in.
    fn take_handle(&mut self, n: int, flag: int) -> int {
        if self.strs.is_empty() {
            self.reset_strs();
        }
        self.snew += 1;
        self.sbytes += n;
        if self.snew >= self.slimit || self.sbytes >= self.sbytes_limit {
            self.gc_flag = true;
        }
        let h = if !self.sfree.is_empty() {
            let h = self.sfree.pop().unwrap();
            let i = h as usize;
            self.strs[i].up = -1;
            self.strs[i].len = 0;
            self.strs[i].ulen = -1;
            self.strs[i].flag = flag as i32;
            self.strs[i].rc = 0;
            self.strs[i].atom = -1;
            h
        } else {
            let e = self.sempty.clone();
            self.strs.push(SEnt::new(e, flag as i32));
            (self.strs.len() - 1) as int
        };
        // a new string is held by nothing counted yet: reclaim() frees it
        // unless a root or the heap has it by then
        self.strs[h as usize].inz = flag == 0;
        if flag == 0 {
            self.szct.push(h);
            if (self.szct.len() as int) >= self.szct_limit {
                self.gc_flag = true;
            }
        }
        h
    }

    /// One more counted reference to string handle `h`.
    #[inline]
    pub fn sinc(&mut self, h: int) {
        if h > 0 {
            self.strs[h as usize].rc += 1;
        }
    }

    /// One counted reference to `h` fewer; at 0 it joins szct.
    #[inline]
    pub fn sdec(&mut self, h: int) {
        if h > 0 {
            let i = h as usize;
            self.strs[i].rc -= 1;
            if self.strs[i].rc <= 0 && !self.strs[i].inz && self.strs[i].flag == 0 {
                self.strs[i].inz = true;
                self.szct.push(h);
                if (self.szct.len() as int) >= self.szct_limit {
                    self.gc_flag = true;
                }
            }
        }
    }

    /// Frees string handle `h` (no count, no root): its buffer goes, and
    /// the handle its text lay in (a prefix's) loses a reference.
    fn free_str(&mut self, h: int) {
        let i = h as usize;
        let up = self.strs[i].up;
        if up < 0 {
            self.sbytes -= self.strs[i].s.as_bytes().len() as int;
            if self.sbytes < 0 {
                self.sbytes = 0;
            }
        }
        if self.snew > 0 {
            self.snew -= 1;
        }
        rc_vacate(&mut self.strs[i].s, &self.sempty);
        self.strs[i].up = -1;
        self.strs[i].len = 0;
        self.strs[i].off = 0;
        self.strs[i].flag = 2;
        self.strs[i].inz = false;
        self.sfreed.push(h);
        self.sfree_count += 1;
        if up >= 0 {
            self.sdec(up as int);
        }
    }

    /// A string value for a shared string.
    /// A string method's unchanged result: `this` itself when it is a
    /// string value (no copy, no new handle), else a handle on `s`.
    pub fn same_str_val(&mut self, this: &Val, s: Rc<String>) -> Val {
        if let Val::Str(_) = this {
            return this.clone();
        }
        self.rc_val(s)
    }

    pub fn rc_val(&mut self, s: Rc<String>) -> Val {
        if s.is_empty() {
            return Val::Str(0);
        }
        Val::Str(self.new_handle(s, 0))
    }

    #[inline(never)]
    pub fn str_val(&mut self, s: &str) -> Val {
        if s.is_empty() {
            return Val::Str(0);
        }
        let h = self.take_handle(s.as_bytes().len() as int, 0);
        rc_fill_str(&mut self.strs[h as usize].s, s);
        Val::Str(h)
    }

    pub fn string_val(&mut self, s: String) -> Val {
        if s.is_empty() {
            return Val::Str(0);
        }
        let h = self.take_handle(s.as_bytes().len() as int, 0);
        rc_refill(&mut self.strs[h as usize].s, s);
        Val::Str(h)
    }

    /// A string value that is never swept (constants, atoms' names).
    pub fn pin_rc(&mut self, s: Rc<String>) -> Val {
        if s.is_empty() {
            return Val::Str(0);
        }
        Val::Str(self.new_handle(s, 1))
    }

    /// The text of string handle `h`.
    #[inline]
    pub fn sref(&self, h: int) -> &str {
        if self.strs[h as usize].up < 0 {
            self.strs[h as usize].s.as_str()
        } else {
            self.sref_up(h)
        }
    }

    #[inline(never)]
    fn sref_up(&self, h: int) -> &str {
        if self.strs[h as usize].off != 0 {
            return self.sref_front(h);
        }
        let mut x = self.strs[h as usize].up;
        while self.strs[x as usize].up >= 0 {
            x = self.strs[x as usize].up;
        }
        jsstr::prefix(self.strs[x as usize].s.as_str(), self.strs[h as usize].len as int)
    }

    /// A front view's text (str_prepend): `len` bytes from `off` of its
    /// buffer's handle (always `up` itself).
    #[inline(never)]
    fn sref_front(&self, h: int) -> &str {
        let x = self.strs[h as usize].up;
        jsstr::mid(self.strs[x as usize].s.as_str(), self.strs[h as usize].off as int, self.strs[h as usize].len as int)
    }

    /// String handle `h` as a shared string.
    #[inline]
    /// Handle `h`'s length in UTF-16 units, kept on the handle.
    pub fn hlen(&mut self, h: int) -> int {
        let u = self.strs[h as usize].ulen;
        if u >= 0 {
            return u as int;
        }
        let n = jsstr::len(self.sref(h));
        self.strs[h as usize].ulen = n as i32;
        n
    }

    /// The UTF-16 unit at `i` of handle `h` when its text is ASCII (its
    /// length is its bytes): -1 out of range; -2 when it is not ASCII
    /// (the caller's general path then).
    pub fn hunit(&mut self, h: int, i: int) -> int {
        let n = self.hlen(h);
        let s = self.sref(h);
        if (n as usize) != s.as_bytes().len() {
            return -2;
        }
        if i < 0 || i >= n {
            return -1;
        }
        jsstr::byte_at(s, i)
    }

    pub fn sget(&self, h: int) -> Rc<String> {
        if self.strs[h as usize].up < 0 {
            self.strs[h as usize].s.clone()
        } else {
            Rc::new(String::from(self.sref_up(h)))
        }
    }

    /// Handle `h`'s string with `b` appended in h's own buffer, when
    /// nothing else holds that buffer: the new string's handle (h stays the
    /// old text, a prefix of it), or -1 and nothing changed.
    pub fn str_append(&mut self, h: int, b: &str) -> int {
        let i = h as usize;
        if h <= 0 || self.strs[i].up >= 0 || self.strs[i].flag != 0 {
            return -1;
        }
        let n = self.strs[i].s.as_bytes().len() as int;
        if !rc_push(&mut self.strs[i].s, b) {
            return -1;
        }
        let rc = self.strs[i].s.clone();
        let e = self.sempty.clone();
        self.strs[i].s = e;
        let h2 = self.new_handle(rc, 0);
        if self.strs[i].ulen >= 0 {
            self.strs[h2 as usize].ulen = self.strs[i].ulen + jsstr::len(b) as i32;
        }
        // only the appended bytes are new
        self.sbytes -= n;
        self.strs[i].up = h2 as i32;
        self.strs[i].len = n as i32;
        // h's text now lies in h2's buffer: h holds h2
        self.strs[h2 as usize].rc += 1;
        h2
    }

    /// A string's length in bytes.
    pub fn hbytes(&self, h: int) -> int {
        if self.strs[h as usize].up < 0 {
            return self.strs[h as usize].s.as_bytes().len() as int;
        }
        self.strs[h as usize].len as int
    }

    /// `b` + string `y` without copying y each time (`s = x + s` in a
    /// loop): the text goes into a buffer with room before it, held by a
    /// handle of its own that no value has, and the result is a view of
    /// it (up, off, len). When y is such a view starting at its buffer's
    /// text and nothing else holds the buffer, b is written into the room
    /// before it; otherwise y moves into a new buffer whose room is an
    /// eighth of the text (all of it when y was a view whose room ran
    /// out, so a long run of prepends copies each byte O(1) times). Rust
    /// only (prepend_ok); -1 when it does not apply.
    #[inline(never)]
    pub fn str_prepend(&mut self, b: &str, y: int) -> int {
        if !prepend_ok() || y <= 0 {
            return -1;
        }
        let yi = y as usize;
        let bl = b.as_bytes().len() as int;
        let r = self.strs[yi].up as int;
        let view = r >= 0 && self.strs[r as usize].up < 0 && self.strs[r as usize].flag == 0 && self.strs[yi].off > 0;
        if view {
            let ri = r as usize;
            let off = self.strs[yi].off as int;
            if off == (self.strs[ri].len as int) && off >= bl && rc_front_write(&mut self.strs[ri].s, off - bl, b) {
                self.strs[ri].len = (off - bl) as i32;
                let n = (self.strs[yi].len as int) + bl;
                return self.str_view(r, off - bl, n, y, b);
            }
        }
        let yt = self.sget(y);
        let total = bl + (yt.as_bytes().len() as int);
        let room = if view { total } else if total / 8 > 64 { total / 8 } else { 64 };
        let mut s = String::new();
        let mut k: int = 0;
        while k < room {
            s.push(' ');
            k += 1;
        }
        s.push_str(b);
        s.push_str(yt.as_str());
        let root = self.new_handle(Rc::new(s), 0);
        self.strs[root as usize].len = room as i32;
        self.str_view(root, room, total, y, b)
    }

    /// A new handle for the `n` bytes at `off` of handle `root`'s buffer:
    /// `b` before the text of `y`.
    fn str_view(&mut self, root: int, off: int, n: int, y: int, b: &str) -> int {
        let h = self.take_handle(b.as_bytes().len() as int, 0);
        let i = h as usize;
        self.strs[i].s = self.sempty.clone();
        self.strs[i].up = root as i32;
        self.strs[i].off = off as i32;
        self.strs[i].len = n as i32;
        if self.strs[y as usize].ulen >= 0 {
            self.strs[i].ulen = self.strs[y as usize].ulen + jsstr::len(b) as i32;
        }
        self.strs[root as usize].rc += 1;
        h
    }

    /// Links the strings of protos from a compiler (Proto.cstrs): their
    /// constants and sources become pinned string values.
    pub fn link_strs(&mut self, p: &mut Proto) {
        trim_proto(p);
        let n = p.cstrs.len();
        if n == 0 {
            return;
        }
        let mut map: Vec<Val> = Vec::new();
        let mut i: usize = 0;
        while i < n {
            let s = p.cstrs[i].clone();
            map.push(self.pin_rc(s));
            i += 1;
        }
        let nc = p.consts.len();
        i = 0;
        while i < nc {
            if let Val::Str(k) = p.consts[i] {
                p.consts[i] = map[k as usize];
            }
            i += 1;
        }
        if let Val::Str(k) = p.src {
            p.src = map[k as usize];
        }
        p.cstrs = Vec::new();
    }

    /// collect(): marks the strings the roots and the live objects (marked)
    /// hold, then frees the rest of the table.
    fn mark_strs(&mut self) {
        // the counts start again from the heap (as the objects' in trace)
        let ns = self.strs.len();
        let mut old: Vec<i32> = Vec::new();
        if self.rc_check {
            old = srcs_of(&self.strs);
        }
        let mut i: usize = 0;
        while i < ns {
            self.strs[i].rc = 0;
            i += 1;
        }
        let total = self.objs.len();
        i = 0;
        while i < total {
            if self.objs[i].marked {
                let p = self.colds[self.objs[i].cold as usize].prim;
                self.smark_count(p);
                let nv = self.objs[i].nv as usize;
                let mut j: usize = 0;
                while j < nv {
                    let v = self.objs[i].get_slot(j);
                    self.smark_count(v);
                    j += 1;
                }
                let ne = self.evecs[self.objs[i].elems as usize].v.len();
                j = 0;
                while j < ne {
                    let v = self.evecs[self.objs[i].elems as usize].v[j];
                    self.smark_count(v);
                    j += 1;
                }
                let ne2 = self.colds[self.objs[i].cold as usize].elems2.len();
                j = 0;
                while j < ne2 {
                    let v = self.colds[self.objs[i].cold as usize].elems2[j];
                    self.smark_count(v);
                    j += 1;
                }
            }
            i += 1;
        }
        let mut roots: Vec<int> = Vec::new();
        self.smark_roots(&mut roots);
        // a prefix keeps the handles its text lies in
        i = 1;
        while i < ns {
            if self.strs[i].mark && self.strs[i].up >= 0 {
                let mut u = self.strs[i].up;
                while u >= 0 && !self.strs[u as usize].mark {
                    self.strs[u as usize].mark = true;
                    u = self.strs[u as usize].up;
                }
            }
            i += 1;
        }
        // and holds a reference to the next one
        i = 1;
        while i < ns {
            if self.strs[i].mark && self.strs[i].up >= 0 {
                let u = self.strs[i].up as usize;
                self.strs[u].rc += 1;
            }
            i += 1;
        }
        if self.rc_check {
            i = 1;
            while i < ns {
                if self.strs[i].mark && self.strs[i].flag == 2 {
                    // a freed string is still referenced
                    self.rc_errors += 1;
                    if self.rc_errors <= 20 {
                        self.rc_log.push(format!("rc: freed string #{} referenced", i));
                    }
                }
                if self.strs[i].mark && self.strs[i].flag == 0 && i < old.len() && old[i] < self.strs[i].rc {
                    // a reference to a string was not counted
                    self.rc_errors += 1;
                    if self.rc_errors <= 20 {
                        let nr = self.strs[i].rc;
                        let t = String::from(self.sref(i as int));
                        self.rc_log.push(format!("rc: low string count {} < {} (#{} {:?})", old[i], nr, i, t));
                    }
                }
                i += 1;
            }
        }
        let nfr = self.sfreed.len();
        i = 0;
        while i < nfr {
            let h = self.sfreed[i];
            self.sfree.push(h);
            i += 1;
        }
        self.sfreed.clear();
        self.szct.clear();
        let mut live: int = 0;
        let mut bytes: int = 0;
        i = 1;
        while i < ns {
            self.strs[i].inz = false;
            if self.strs[i].flag == 0 && !self.strs[i].mark {
                rc_vacate(&mut self.strs[i].s, &self.sempty);
                self.strs[i].up = -1;
                self.strs[i].off = 0;
                self.strs[i].flag = 2;
                self.sfreed.push(i as int);
            } else if self.strs[i].flag != 2 {
                live += 1;
                bytes += self.strs[i].s.as_bytes().len() as int;
                if self.strs[i].flag == 0 {
                    rc_trim(&mut self.strs[i].s);
                }
                if self.strs[i].flag == 0 && self.strs[i].rc == 0 {
                    // held by roots only
                    self.strs[i].inz = true;
                    self.szct.push(i as int);
                }
            }
            self.strs[i].mark = false;
            i += 1;
        }
        self.snew = 0;
        self.slimit = if live > self.slimit_min { live } else { self.slimit_min };
        self.sbytes = 0;
        self.sbytes_limit = if bytes > SBYTES_MIN { bytes } else { SBYTES_MIN };
        self.szct_limit = self.next_szct_limit();
    }

    /// mark_strs: a heap reference to a string, marked and counted.
    #[inline]
    fn smark_count(&mut self, v: Val) {
        if let Val::Str(h) = v {
            self.strs[h as usize].mark = true;
            if h > 0 {
                self.strs[h as usize].rc += 1;
            }
        }
    }

    /// A root's string marked; `list` gets each handle marked here.
    #[inline]
    fn smark_root(&mut self, v: Val, list: &mut Vec<int>) {
        if let Val::Str(h) = v {
            if !self.strs[h as usize].mark {
                self.strs[h as usize].mark = true;
                list.push(h);
            }
        }
    }

    /// Marks the strings the roots hold (not the heap): the stack, the
    /// frames, the pending exception and jobs, and the machine's tables.
    fn smark_roots(&mut self, list: &mut Vec<int>) {
        let n = self.stack.len();
        let mut i: usize = 0;
        while i < n {
            let v = self.stack[i];
            self.smark_root(v, list);
            i += 1;
        }
        let nf = self.frames.len();
        i = 0;
        while i < nf {
            let t = self.frames[i].this_val;
            let nt = self.frames[i].new_target;
            self.smark_root(t, list);
            self.smark_root(nt, list);
            i += 1;
        }
        let x = self.exc;
        self.smark_root(x, list);
        i = 0;
        while i < self.temp_roots.len() {
            let v = self.temp_roots[i];
            self.smark_root(v, list);
            i += 1;
        }
        i = 0;
        while i < self.jobs.len() {
            let v = self.jobs[i];
            self.smark_root(v, list);
            i += 1;
        }
        // atom_vals are not scanned: atom_val pins each one (flag 1),
        // so reclaim_strs never frees them
        i = 0;
        while i < self.type_vals.len() {
            let v = self.type_vals[i];
            self.smark_root(v, list);
            i += 1;
        }
        // kvals are not scanned either: translate() copies a function's
        // constants, whose strings link_strs pinned (flag 1)
        i = 0;
        while i < self.regex_shown.len() {
            let v = self.regex_shown[i];
            self.smark_root(v, list);
            i += 1;
        }
    }

    /// reclaim(): frees the strings of szct that neither the heap nor a
    /// root holds, and what only they held (a prefix's next handle).
    fn reclaim_strs(&mut self) {
        // handles freed by the last reclaim may be reused now
        let nfr = self.sfreed.len();
        let mut i: usize = 0;
        while i < nfr {
            let h = self.sfreed[i];
            self.sfree.push(h);
            i += 1;
        }
        self.sfreed.clear();
        if self.szct.is_empty() {
            return;
        }
        let mut marked: Vec<int> = Vec::new();
        self.smark_roots(&mut marked);
        let mut keep: Vec<int> = Vec::new();
        while !self.szct.is_empty() {
            let list = take_ints(&mut self.szct, &mut self.szct_spare);
            let n = list.len();
            i = 0;
            while i < n {
                let h = list[i];
                i += 1;
                let k = h as usize;
                if !self.strs[k].inz {
                    continue;
                }
                self.strs[k].inz = false;
                if self.strs[k].flag != 0 || self.strs[k].rc > 0 {
                    continue;
                }
                if self.strs[k].mark {
                    // a root holds it
                    self.strs[k].inz = true;
                    keep.push(h);
                    continue;
                }
                self.free_str(h);
            }
            give_ints(&mut self.szct_spare, list);
        }
        self.szct = keep;
        let nm = marked.len();
        i = 0;
        while i < nm {
            let h = marked[i] as usize;
            self.strs[h].mark = false;
            i += 1;
        }
        self.szct_limit = self.next_szct_limit();
    }

    fn next_szct_limit(&self) -> int {
        let n = (self.szct.len() as int) * 2;
        if n > self.zct_min {
            n
        } else {
            self.zct_min
        }
    }

    pub fn intern(&mut self, s: &str) -> int {
        // a direct-mapped cache in front of the map, by a cheap hash of the
        // length and some of the bytes
        let n = s.as_bytes().len();
        let mut h: int = (n as int) & 65535;
        if n > 0 {
            h = (h * 31 + (s.as_bytes()[0] as int)) & 65535;
            h = (h * 31 + (s.as_bytes()[n - 1] as int)) & 65535;
            h = (h * 31 + (s.as_bytes()[n / 2] as int)) & 65535;
            if n > 3 {
                h = (h * 31 + (s.as_bytes()[1] as int)) & 65535;
                h = (h * 31 + (s.as_bytes()[n - 2] as int)) & 65535;
            }
        }
        let k = (h & 4095) as usize;
        if self.atom_cache.is_empty() {
            let mut i = 0;
            while i < 4096 {
                self.atom_cache.push(-1);
                i += 1;
            }
        }
        let c = self.atom_cache[k];
        if c >= 0 && (c as usize) < self.atom_names.len() && self.atom_names[c as usize].as_str() == s {
            return c;
        }
        self.fa_catch_up();
        let hs = str_hash(s);
        let f = self.fa_find(s, hs);
        let a = if f >= 0 {
            f
        } else {
            // an array index: its atom may be in the index table only
            let ix = if n > 0 && n <= 9 && s.as_bytes()[0] >= 48 && s.as_bytes()[0] <= 57 { jsstr::array_index(s) } else { -1 };
            if ix >= 0 {
                self.index_atom(ix as int)
            } else {
                let a = self.atom_names.len() as int;
                self.atom_names.push(String::from(s));
                self.atom_hash.push(hs);
                self.fa_insert(a, hs);
                self.fa_seen = a + 1;
                self.idx_pending.push(a);
                a
            }
        };
        self.atom_cache[k] = a;
        a
    }

    /// Indexes the atoms made elsewhere (a compiler's, a snapshot's) in
    /// intern's table.
    fn fa_catch_up(&mut self) {
        let n = self.atom_names.len() as int;
        while self.fa_seen < n {
            let a = self.fa_seen;
            let h = str_hash(self.atom_names[a as usize].as_str());
            self.atom_hash.push(h);
            self.fa_insert(a, h);
            self.fa_seen += 1;
        }
    }

    fn fa_insert(&mut self, a: int, h: int) {
        let n = self.fa_tab.len() as int;
        if (self.fa_count + 1) * 2 > n {
            let nn: int = if n == 0 { 4096 } else { n * 2 };
            let mut nt: Vec<int> = Vec::with_capacity(nn as usize);
            let mut j: int = 0;
            while j < nn {
                nt.push(-1);
                j += 1;
            }
            let old = self.fa_tab.clone();
            self.fa_tab = nt;
            self.fa_count = 0;
            let mut q: usize = 0;
            while q < old.len() {
                let b = old[q];
                if b >= 0 {
                    let hb = self.atom_hash[b as usize];
                    self.fa_put(b, hb);
                }
                q += 1;
            }
        }
        self.fa_put(a, h);
    }

    fn fa_put(&mut self, a: int, h: int) {
        let mask = (self.fa_tab.len() as int) - 1;
        let mut i = h & mask;
        while self.fa_tab[i as usize] >= 0 {
            i = (i + 1) & mask;
        }
        self.fa_tab[i as usize] = a;
        self.fa_count += 1;
    }

    fn fa_find(&self, s: &str, h: int) -> int {
        if self.fa_count == 0 {
            return -1;
        }
        let mask = (self.fa_tab.len() as int) - 1;
        let mut i = h & mask;
        loop {
            let a = self.fa_tab[i as usize];
            if a < 0 {
                return -1;
            }
            if self.atom_hash[a as usize] == h && self.atom_names[a as usize].as_str() == s {
                return a;
            }
            i = (i + 1) & mask;
        }
    }

    /// The atom named `s` when there is one (nothing is made), else -1.
    pub fn atom_lookup(&self, s: &str) -> int {
        let f = self.fa_find(s, str_hash(s));
        if f >= 0 {
            return f;
        }
        let mut a = self.fa_seen;
        while a < (self.atom_names.len() as int) {
            if self.atom_names[a as usize].as_str() == s {
                return a;
            }
            a += 1;
        }
        match self.atoms.get(s) {
            Some(x) => *x,
            None => -1,
        }
    }

    /// Puts the index atoms only the index table knows into `atoms` (before
    /// a compiler gets a copy of it).
    pub fn atoms_sync(&mut self) {
        let n = self.idx_pending.len();
        let mut j: usize = 0;
        while j < n {
            let a = self.idx_pending[j];
            let name = self.atom_names[a as usize].clone();
            self.atoms.insert(name, a);
            j += 1;
        }
        self.idx_pending.clear();
    }

    /// The name of atom `a` as a string value, shared.
    /// The kind of key atom `a` is (Vm.atom_kinds).
    pub fn atom_kind(&mut self, a: int) -> int {
        while self.atom_kinds.len() <= a as usize {
            self.atom_kinds.push(0);
        }
        let k = self.atom_kinds[a as usize];
        if k != 0 {
            return k as int;
        }
        let n: int = if self.atom_names[a as usize].starts_with('\u{1}') {
            4
        } else if self.symbol_atoms.contains_key(&a) {
            3
        } else if is_index_key(self.atom_names[a as usize].as_str()) {
            2
        } else {
            1
        };
        self.atom_kinds[a as usize] = n as i32;
        n
    }

    pub fn atom_val(&mut self, a: int) -> Val {
        while self.atom_vals.len() <= a as usize {
            self.atom_vals.push(Val::Undef);
        }
        if let Val::Undef = self.atom_vals[a as usize] {
            let rc = Rc::new(self.atom_names[a as usize].clone());
            self.atom_vals[a as usize] = self.pin_rc(rc);
        }
        self.atom_vals[a as usize].clone()
    }

    /// The string of one UTF-16 unit `c`; ASCII ones are shared.
    pub fn unit_val(&mut self, c: int) -> Val {
        if c >= 128 {
            return self.string_val(jsstr::from_unit(c));
        }
        if self.char_vals.is_empty() {
            let mut k: int = 0;
            while k < 128 {
                let v = self.pin_rc(Rc::new(jsstr::from_unit(k)));
                self.char_vals.push(v);
                k += 1;
            }
        }
        self.char_vals[c as usize].clone()
    }

    /// `typeof v` as a string value, shared.
    pub fn type_val(&mut self, v: &Val) -> Val {
        if self.type_vals.is_empty() {
            let sv27 = self.str_val("undefined");
            self.type_vals.push(sv27);
            let sv26 = self.str_val("object");
            self.type_vals.push(sv26);
            let sv25 = self.str_val("boolean");
            self.type_vals.push(sv25);
            let sv24 = self.str_val("number");
            self.type_vals.push(sv24);
            let sv23 = self.str_val("string");
            self.type_vals.push(sv23);
            let sv22 = self.str_val("function");
            self.type_vals.push(sv22);
            let sv21 = self.str_val("symbol");
            self.type_vals.push(sv21);
            let sv20 = self.str_val("bigint");
            self.type_vals.push(sv20);
        }
        let i = self.type_index(v);
        self.type_vals[i].clone()
    }

    pub fn atom_str(&self, a: int) -> String {
        self.atom_names[a as usize].clone()
    }

    pub fn is_symbol_atom(&self, a: int) -> bool {
        self.symbol_atoms.contains_key(&a)
    }

    // ---- allocation

    pub fn alloc(&mut self, class: i32, proto: int) -> int {
        self.alloc_count += 1;
        self.made += 1;
        self.live += 1;
        if proto >= 0 {
            let p = crate::rvm::obj_mut(&mut self.objs, proto as usize);
            p.rc += 1;
            p.is_proto = true;
        }
        let i = if !self.free_list.is_empty() {
            let i = self.free_list.pop().unwrap();
            let o = crate::rvm::obj_mut(&mut self.objs, i as usize);
            o.class = class;
            o.proto = proto as i32;
            o.free = false;
            o.marked = false;
            if self.rc_on {
                o.zct = true;
            }
            i
        } else {
            self.alloc_slot(class, proto)
        };
        if self.rc_on {
            self.zct.push(i);
        }
        if self.alloc_count >= self.gc_threshold || (self.zct.len() as int) >= self.zct_limit || self.made >= self.made_limit {
            self.gc_flag = true;
        }
        i
    }

    /// A new slot at the end of objs (alloc's free list was empty).
    #[inline(never)]
    fn alloc_slot(&mut self, class: i32, proto: int) -> int {
        grow_objs(&mut self.objs);
        self.objs.push(JsObj::new(class, proto as i32));
        let i = (self.objs.len() as int) - 1;
        if self.rc_on {
            self.objs[i as usize].zct = true;
        }
        i
    }

    pub fn new_object(&mut self) -> int {
        let p = self.object_proto;
        self.alloc(C_OBJECT, p)
    }

    pub fn new_array(&mut self, elems: Vec<Val>) -> int {
        let p = self.array_proto;
        let a = self.alloc(C_ARRAY, p);
        self.put_elems(a, elems);
        a
    }

    pub fn new_error(&mut self, proto: int, msg: &str) -> int {
        let e = self.alloc(C_ERROR, proto);
        if !msg.is_empty() {
            let sv19 = self.str_val(msg);
            self.add_own(e, A_MESSAGE, sv19, P_HIDDEN);
        }
        self.add_stack(e);
        e
    }

    /// `stack` of a new error: its name and message, then a line for each
    /// function running, innermost first (as V8 writes it, without places)
    pub fn add_stack(&mut self, e: int) {
        let ev = Val::Obj(e);
        let nv = self.get_obj(e, A_NAME, &ev);
        let mv = self.get_obj(e, A_MESSAGE, &ev);
        if self.throwing {
            return;
        }
        let name = match &nv {
            Val::Str(s) => self.sref(*s).to_string(),
            _ => String::from("Error"),
        };
        let msg = match &mv {
            Val::Str(s) => self.sref(*s).to_string(),
            _ => String::new(),
        };
        let mut text = if msg.is_empty() { name } else if name.is_empty() { msg } else { format!("{}: {}", name, msg) };
        let mut k = self.frames.len() as int - 1;
        let mut lines = 0;
        while k >= 0 && lines < 10 {
            let p = self.frames[k as usize].proto;
            if p >= self.prelude_protos {
                let fname = self.protos[p as usize].name.clone();
                if fname.is_empty() {
                    text.push_str("\n    at <anonymous>");
                } else {
                    text.push_str("\n    at ");
                    text.push_str(fname.as_str());
                }
                lines += 1;
            }
            k -= 1;
        }
        let sv14 = self.string_val(text);
        self.add_own(e, A_STACK, sv14, P_HIDDEN);
    }

    pub fn throw_val(&mut self, v: Val) {
        self.exc = v;
        self.throwing = true;
    }

    /// The host __hostSync(text) asks: its answer is the call's value.
    pub fn set_host_sync(&mut self, h: Box<dyn HostSync>) {
        self.host_sync = h;
        self.host_sync_set = true;
    }

    pub fn throw_type(&mut self, msg: &str) {
        let p = self.type_error_proto;
        let e = self.new_error(p, msg);
        self.throw_val(Val::Obj(e));
    }

    pub fn throw_range(&mut self, msg: &str) {
        let p = self.range_error_proto;
        let e = self.new_error(p, msg);
        self.throw_val(Val::Obj(e));
    }

    pub fn throw_ref(&mut self, msg: &str) {
        let p = self.reference_error_proto;
        let e = self.new_error(p, msg);
        self.throw_val(Val::Obj(e));
    }

    pub fn throw_syntax(&mut self, msg: &str) {
        let p = self.syntax_error_proto;
        let e = self.new_error(p, msg);
        self.throw_val(Val::Obj(e));
    }

    // ---- conversions

    pub fn class_of(&self, v: &Val) -> i32 {
        match v {
            Val::Obj(o) => self.objs[*o as usize].class,
            Val::Big(_) => C_BIGINT,
            _ => -1,
        }
    }

    pub fn is_callable(&self, v: &Val) -> bool {
        let c = self.class_of(v);
        c == C_FUNCTION || c == C_NATIVE || c == C_BOUND || (c == C_PROXY && (self.objs[obj_of(v) as usize].pos & 1) != 0)
    }

    pub fn type_of(&self, v: &Val) -> String {
        let i = self.type_index(v);
        let s = if i == 0 {
            "undefined"
        } else if i == 1 {
            "object"
        } else if i == 2 {
            "boolean"
        } else if i == 3 {
            "number"
        } else if i == 4 {
            "string"
        } else if i == 5 {
            "function"
        } else if i == 6 {
            "symbol"
        } else {
            "bigint"
        };
        String::from(s)
    }

    /// typeof as 0 undefined, 1 object, 2 boolean, 3 number, 4 string,
    /// 5 function, 6 symbol, 7 bigint
    pub fn type_index(&self, v: &Val) -> usize {
        match v {
            Val::Undef => 0,
            Val::Null => 1,
            Val::Bool(_) => 2,
            Val::Num(_) => 3,
            Val::Str(_) => 4,
            Val::Big(_) => 7,
            Val::Obj(o) => {
                let c = self.objs[*o as usize].class;
                if c == C_FUNCTION || c == C_NATIVE || c == C_BOUND || (c == C_PROXY && (self.objs[*o as usize].pos & 1) != 0) {
                    5
                } else if c == C_SYMBOL {
                    6
                } else if c == C_BIGINT {
                    7
                } else {
                    1
                }
            }
        }
    }

    /// ToPrimitive: `hint` "number", "string" or "default"
    pub fn to_primitive(&mut self, v: &Val, hint: &str) -> Val {
        let o = match v {
            Val::Obj(o) => *o,
            _ => return v.clone(),
        };
        let class = self.objs[o as usize].class;
        if class == C_SYMBOL || class == C_BIGINT {
            return v.clone();
        }
        // @@toPrimitive
        let tp = self.get_obj(o, A_TOPRIMITIVE, v);
        if self.throwing {
            return Val::Undef;
        }
        if self.is_callable(&tp) {
            let sv18 = self.str_val(hint);
            let r = self.call_fixed(tp, v.clone(), 1, sv18, Val::Undef, Val::Undef);
            if self.throwing {
                return Val::Undef;
            }
            if is_obj(&r) && self.class_of(&r) != C_SYMBOL && self.class_of(&r) != C_BIGINT {
                self.throw_type("Cannot convert object to primitive value");
                return Val::Undef;
            }
            return r;
        }
        if !matches!(tp, Val::Undef | Val::Null) {
            self.throw_type("Symbol.toPrimitive is not a function");
            return Val::Undef;
        }
        let order = if hint == "string" { vec![A_TOSTRING, A_VALUEOF] } else { vec![A_VALUEOF, A_TOSTRING] };
        for a in order {
            let f = self.get_obj(o, a, v);
            if self.throwing {
                return Val::Undef;
            }
            if self.is_callable(&f) {
                let r = self.call_fixed(f, v.clone(), 0, Val::Undef, Val::Undef, Val::Undef);
                if self.throwing {
                    return Val::Undef;
                }
                if !(is_obj(&r) && self.class_of(&r) != C_SYMBOL && self.class_of(&r) != C_BIGINT) {
                    return r;
                }
            }
        }
        self.throw_type("Cannot convert object to primitive value");
        Val::Undef
    }

    pub fn to_number(&mut self, v: &Val) -> double {
        match v {
            Val::Num(n) => *n,
            Val::Undef => nan(),
            Val::Null => 0.0,
            Val::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Val::Str(s) => string_to_number(self.sref(*s)),
            Val::Big(_) => {
                self.throw_type("Cannot convert a BigInt value to a number");
                nan()
            }
            Val::Obj(o) => {
                if self.objs[*o as usize].class == C_SYMBOL {
                    self.throw_type("Cannot convert a Symbol value to a number");
                    return nan();
                }
                if self.objs[*o as usize].class == C_BIGINT {
                    self.throw_type("Cannot convert a BigInt value to a number");
                    return nan();
                }
                let p = self.to_primitive(v, "number");
                if self.throwing {
                    return nan();
                }
                self.to_number(&p)
            }
        }
    }

    pub fn to_str(&mut self, v: &Val) -> Rc<String> {
        match v {
            Val::Str(s) => self.sget(*s),
            Val::Num(n) => Rc::new(number_to_string(*n)),
            Val::Undef => Rc::new(String::from("undefined")),
            Val::Null => Rc::new(String::from("null")),
            Val::Bool(b) => Rc::new(String::from(if *b { "true" } else { "false" })),
            Val::Big(n) => Rc::new(crate::bigint::int_text(*n as int)),
            Val::Obj(o) => {
                if self.objs[*o as usize].class == C_SYMBOL {
                    self.throw_type("Cannot convert a Symbol value to a string");
                    return Rc::new(String::new());
                }
                if self.objs[*o as usize].class == C_BIGINT {
                    let pv = self.colds[self.objs[*o as usize].cold as usize].prim;
                    if let Val::Str(s) = pv {
                        return self.sget(s);
                    }
                }
                let p = self.to_primitive(v, "string");
                if self.throwing {
                    return Rc::new(String::new());
                }
                self.to_str(&p)
            }
        }
    }

    pub fn to_string(&mut self, v: &Val) -> String {
        let r = self.to_str(v);
        r.as_ref().clone()
    }

    pub fn to_object(&mut self, v: &Val) -> int {
        match v {
            Val::Obj(o) => {
                if self.objs[*o as usize].class == C_BIGINT {
                    let p = self.bigint_proto;
                    let b = self.alloc(C_OBJECT, p);
                    self.put_prim(b, v.clone());
                    return b;
                }
                if self.objs[*o as usize].class == C_SYMBOL {
                    // a Symbol object: the symbol in prim
                    let p = self.symbol_proto;
                    let b = self.alloc(C_OBJECT, p);
                    self.put_prim(b, v.clone());
                    return b;
                }
                *o
            }
            Val::Undef | Val::Null => {
                self.throw_type("Cannot convert undefined or null to object");
                -1
            }
            Val::Big(_) => {
                let p = self.bigint_proto;
                let b = self.alloc(C_OBJECT, p);
                self.put_prim(b, v.clone());
                b
            }
            Val::Bool(_) => {
                let p = self.boolean_proto;
                let o = self.alloc(C_BOOLEAN, p);
                self.put_prim(o, v.clone());
                o
            }
            Val::Num(_) => {
                let p = self.number_proto;
                let o = self.alloc(C_NUMBER, p);
                self.put_prim(o, v.clone());
                o
            }
            Val::Str(_) => {
                let p = self.string_proto;
                let o = self.alloc(C_STRING, p);
                self.put_prim(o, v.clone());
                o
            }
        }
    }

    pub fn strict_equals(&self, a: &Val, b: &Val) -> bool {
        match (a, b) {
            (Val::Num(x), Val::Num(y)) => x == y,
            (Val::Str(x), Val::Str(y)) => x == y || self.sref(*x) == self.sref(*y),
            (Val::Undef, Val::Undef) => true,
            (Val::Null, Val::Null) => true,
            (Val::Bool(x), Val::Bool(y)) => x == y,
            (Val::Obj(x), Val::Obj(y)) => x == y,
            (Val::Big(x), Val::Big(y)) => x == y,
            _ => false,
        }
    }

    pub fn same_value_zero(&self, a: &Val, b: &Val) -> bool {
        match (a, b) {
            (Val::Num(x), Val::Num(y)) => x == y || (is_nan(*x) && is_nan(*y)),
            _ => self.strict_equals(a, b),
        }
    }

    pub fn same_value(&self, a: &Val, b: &Val) -> bool {
        match (a, b) {
            (Val::Num(x), Val::Num(y)) => {
                if is_nan(*x) && is_nan(*y) {
                    return true;
                }
                if *x == 0.0 && *y == 0.0 {
                    return (1.0 / *x) == (1.0 / *y);
                }
                x == y
            }
            _ => self.strict_equals(a, b),
        }
    }

    pub fn loose_equals(&mut self, a: &Val, b: &Val) -> bool {
        match (a, b) {
            (Val::Undef, Val::Null) | (Val::Null, Val::Undef) => return true,
            (Val::Undef, Val::Undef) | (Val::Null, Val::Null) => return true,
            (Val::Undef, _) | (Val::Null, _) | (_, Val::Undef) | (_, Val::Null) => return false,
            (Val::Num(x), Val::Num(y)) => return x == y,
            (Val::Str(x), Val::Str(y)) => return x == y || self.sref(*x) == self.sref(*y),
            (Val::Bool(x), Val::Bool(y)) => return x == y,
            (Val::Big(x), Val::Big(y)) => return x == y,
            (Val::Obj(x), Val::Obj(y)) => {
                if x == y {
                    return true;
                }
                // a Symbol object and a symbol: the object's primitive
                let (cx, cy) = (self.objs[*x as usize].class, self.objs[*y as usize].class);
                if (cx == C_SYMBOL) != (cy == C_SYMBOL) {
                    let obj = if cx == C_SYMBOL { b } else { a };
                    let sym = if cx == C_SYMBOL { a } else { b };
                    let p = self.to_primitive(obj, "default");
                    if self.throwing {
                        return false;
                    }
                    return self.strict_equals(&p, sym);
                }
                return false;
            }
            _ => {}
        }
        if let Val::Bool(x) = a {
            let n = Val::Num(if *x { 1.0 } else { 0.0 });
            return self.loose_equals(&n, b);
        }
        if let Val::Bool(y) = b {
            let n = Val::Num(if *y { 1.0 } else { 0.0 });
            return self.loose_equals(a, &n);
        }
        match (a, b) {
            (Val::Num(x), Val::Str(s)) => return *x == string_to_number(self.sref(*s)),
            (Val::Str(s), Val::Num(y)) => return string_to_number(self.sref(*s)) == *y,
            _ => {}
        }
        if self.class_of(a) == C_BIGINT && self.class_of(b) == C_BIGINT {
            // a small one (Val::Big) and a large one (an object) differ
            return self.strict_equals(a, b);
        }
        if (self.class_of(a) == C_BIGINT) != (self.class_of(b) == C_BIGINT) && !(is_obj(a) && self.class_of(a) != C_BIGINT) && !(is_obj(b) && self.class_of(b) != C_BIGINT) {
            return self.big_loose_eq(a, b);
        }
        if is_obj(a) && !is_obj(b) {
            if self.class_of(a) == C_SYMBOL {
                return false;
            }
            let p = self.to_primitive(a, "default");
            if self.throwing {
                return false;
            }
            return self.loose_equals(&p, b);
        }
        if is_obj(b) && !is_obj(a) {
            if self.class_of(b) == C_SYMBOL {
                return false;
            }
            let p = self.to_primitive(b, "default");
            if self.throwing {
                return false;
            }
            return self.loose_equals(a, &p);
        }
        false
    }

    // ---- property keys

    /// A property key: (array index, atom). The index is -1 when the key is
    /// not one; the atom is -1 when it was not interned (an index).
    pub fn to_key(&mut self, k: &Val) -> (int, int) {
        // a string first (the common key), outside the match: a match on
        // every Val kind compiles to a jump table here
        if let Val::Str(s) = k {
            let h = *s as usize;
            let known = self.strs[h].atom;
            if known >= 0 {
                return (-1, known as int);
            }
            let i = jsstr::array_index(self.sref(*s));
            if i >= 0 {
                return (i, -1);
            }
            let t = self.sget(*s);
            let a = self.intern(t.as_str());
            self.strs[h].atom = a as i32;
            return (-1, a);
        }
        match k {
            Val::Num(n) => {
                if *n >= 0.0 && *n < 1000000000.0 {
                    let i = *n as int;
                    if (i as double) == *n {
                        return (i, -1);
                    }
                }
                let s = number_to_string(*n);
                let a = self.intern(s.as_str());
                (-1, a)
            }
            Val::Obj(o) => {
                if self.objs[*o as usize].class == C_SYMBOL {
                    return (-1, self.objs[*o as usize].pos as int);
                }
                if self.objs[*o as usize].class == C_BIGINT {
                    return self.big_key(k);
                }
                let p = self.to_primitive(k, "string");
                if self.throwing {
                    return (-1, A_LENGTH);
                }
                self.to_key(&p)
            }
            Val::Big(_) => self.big_key(k),
            _ => {
                let s = self.to_string(k);
                let a = self.intern(s.as_str());
                (-1, a)
            }
        }
    }

    /// to_key of a BigInt: a small one below 10^9 an index, as a number
    /// would be; otherwise its text (a large one is never an index).
    #[inline(never)]
    fn big_key(&mut self, k: &Val) -> (int, int) {
        if let Val::Big(n) = k {
            if *n >= 0.0 && *n < 1000000000.0 {
                return (*n as int, -1);
            }
        }
        let t = self.big_text(k);
        let a = self.intern(t.as_str());
        (-1, a)
    }

    pub fn index_atom(&mut self, i: int) -> int {
        if i < 0 {
            let s = format!("{}", i);
            return self.intern(s.as_str());
        }
        let c = self.idx_get(i);
        if c >= 0 {
            return c;
        }
        let s = int_to_string(i);
        if i > 999999999 {
            return self.intern(s.as_str());
        }
        // an atom of this name made elsewhere (a compiler's) and not seen yet
        if self.idx_seen < (self.atom_names.len() as int) {
            let c2 = self.index_atom_if(i);
            if c2 >= 0 {
                return c2;
            }
        }
        // a new one: in the index table only, no string hashing (and seen)
        let a = self.atom_names.len() as int;
        self.atom_names.push(s);
        self.idx_put(i, a);
        self.idx_pending.push(a);
        self.idx_seen = a + 1;
        if self.fa_seen == a {
            // left out of intern's table (intern finds it by its digits)
            self.atom_hash.push(-1);
            self.fa_seen = a + 1;
        }
        a
    }

    fn idx_get(&self, i: int) -> int {
        let n = self.idx_keys.len() as int;
        if n == 0 {
            return -1;
        }
        let mask = n - 1;
        let x = i * 40503;
        let mut h = (x ^ (x >> 15)) & mask;
        loop {
            let k = self.idx_keys[h as usize];
            if k == 0 {
                return -1;
            }
            if k == i + 1 {
                return self.idx_vals[h as usize];
            }
            h = (h + 1) & mask;
        }
    }

    fn idx_put(&mut self, i: int, a: int) {
        let n = self.idx_keys.len() as int;
        if (self.idx_count + 1) * 2 > n {
            // grow to twice the size (or 256) and put every entry back
            let nn: int = if n == 0 { 256 } else { n * 2 };
            let ok = self.idx_keys.clone();
            let ov = self.idx_vals.clone();
            let mut nk: Vec<int> = Vec::new();
            let mut nv: Vec<int> = Vec::new();
            let mut j: int = 0;
            while j < nn {
                nk.push(0);
                nv.push(0);
                j += 1;
            }
            self.idx_keys = nk;
            self.idx_vals = nv;
            self.idx_count = 0;
            let mut j: usize = 0;
            while j < ok.len() {
                if ok[j] != 0 {
                    self.idx_put(ok[j] - 1, ov[j]);
                }
                j += 1;
            }
        }
        let mask = self.idx_keys.len() as int - 1;
        let x = i * 40503;
        let mut h = (x ^ (x >> 15)) & mask;
        loop {
            let k = self.idx_keys[h as usize];
            if k == 0 {
                self.idx_keys[h as usize] = i + 1;
                self.idx_vals[h as usize] = a;
                self.idx_count += 1;
                return;
            }
            if k == i + 1 {
                return;
            }
            h = (h + 1) & mask;
        }
    }

    /// The atom of index `i` when there is one already, else -1: no
    /// object can have that key yet, so a read of it makes none. Atoms are
    /// only appended, so the index names among the new ones are added to
    /// the idx table first, and it then holds every index atom.
    pub fn index_atom_if(&mut self, i: int) -> int {
        if i > 999999999 {
            // past what array_index reads (9 digits)
            let s = format!("{}", i);
            return self.atom_lookup(s.as_str());
        }
        let n = self.atom_names.len() as int;
        while self.idx_seen < n {
            let k = jsstr::array_index(self.atom_names[self.idx_seen as usize].as_str());
            if k >= 0 {
                let a = self.idx_seen;
                self.idx_put(k as int, a);
            }
            self.idx_seen += 1;
        }
        self.idx_get(i)
    }

    pub fn key_val(&mut self, a: int) -> Val {
        if self.atom_kind(a) != 3 {
            // not a symbol's atom (kept per atom): no table lookup
            return self.atom_val(a);
        }
        match self.symbol_atoms.get(&a) {
            Some(sym) => Val::Obj(*sym),
            None => self.atom_val(a),
        }
    }

    // ---- property access

    pub fn function_prototype(&mut self, f: int) -> Val {
        // made on first use; a generator's instances inherit from it and it
        // from %GeneratorPrototype%, with no `constructor`
        let pi = self.objs[f as usize].func as int;
        let generator = self.objs[f as usize].class == C_FUNCTION && self.protos[pi as usize].generator;
        let proto = if generator && self.protos[pi as usize].is_async {
            self.async_generator_proto
        } else if generator {
            self.generator_proto
        } else {
            self.object_proto
        };
        let p = self.alloc(C_OBJECT, proto);
        if !generator {
            self.add_own(p, A_CONSTRUCTOR, Val::Obj(f), P_HIDDEN);
        }
        self.add_own(f, A_PROTOTYPE, Val::Obj(p), P_HIDDEN | P_FIXED);
        Val::Obj(p)
    }

    /// A function with a `prototype` of its own that is not made yet.
    pub fn lazy_prototype(&self, f: int) -> bool {
        // made (or set): an own property
        if self.find(f, A_PROTOTYPE) >= 0 {
            return false;
        }
        let p = &self.protos[self.objs[f as usize].func as usize];
        p.generator || p.constructible()
    }

    /// A function's built-in `length` / `name` (not stored, function_own)
    /// was deleted: a hidden private-named marker stands for that.
    pub fn vgone(&mut self, f: int, atom: int) -> bool {
        let m = self.vgone_atom(atom);
        self.find(f, m) >= 0
    }

    pub fn vgone_atom(&mut self, atom: int) -> int {
        if atom == A_NAME {
            self.intern("\u{1}gone:name")
        } else {
            self.intern("\u{1}gone:length")
        }
    }

    /// A function's own `length`, `name`, `prototype` when not stored.
    pub fn function_own(&mut self, f: int, atom: int) -> Val {
        let class = self.objs[f as usize].class;
        if (atom == A_LENGTH || atom == A_NAME) && (class == C_FUNCTION || class == C_BOUND) && self.vgone(f, atom) {
            return Val::Undef;
        }
        if class == C_FUNCTION {
            let pi = self.objs[f as usize].func as int;
            if atom == A_LENGTH {
                return Val::Num(self.protos[pi as usize].length as double);
            }
            if atom == A_NAME {
                let nm = self.protos[pi as usize].name.clone();
                return self.string_val(nm);
            }
            if atom == A_PROTOTYPE && self.find(f, A_PROTOTYPE) < 0 {
                let p = &self.protos[pi as usize];
                if p.generator || p.constructible() {
                    return self.function_prototype(f);
                }
            }
        } else if class == C_BOUND {
            if atom == A_NAME {
                let t = self.objs[f as usize].env as int;
                let n = self.get_obj(t, A_NAME, &Val::Obj(t));
                let s = self.to_string(&n);
                return self.string_val(format!("bound {}", s));
            }
            if atom == A_LENGTH {
                let t = self.objs[f as usize].env as int;
                let l = self.get_obj(t, A_LENGTH, &Val::Obj(t));
                let n = self.to_number(&l) - (self.evecs[self.objs[f as usize].elems as usize].v.len() as double);
                return Val::Num(if n < 0.0 { 0.0 } else { n });
            }
        }
        Val::Undef
    }

    /// `o[atom]` with `receiver` as `this` for getters.
    pub fn get_obj(&mut self, o: int, atom: int, receiver: &Val) -> Val {
        let mut cur = o;
        let mut hops = 0;
        while cur >= 0 {
            let slot = self.find(cur, atom);
            if slot >= 0 {
                let attr = self.attr_at(cur, slot);
                if (attr & (P_ACCESSOR | P_MAPPED)) != 0 {
                    if (attr & P_MAPPED) != 0 {
                        return self.mapped_get(cur, atom, slot);
                    }
                    let pair = obj_of(&self.objs[cur as usize].get_slot(slot as usize));
                    let g = self.evecs[self.objs[pair as usize].elems as usize].v[0].clone();
                    if self.is_callable(&g) {
                        return self.call_fixed(g, receiver.clone(), 0, Val::Undef, Val::Undef, Val::Undef);
                    }
                    return Val::Undef;
                }
                return self.objs[cur as usize].get_slot(slot as usize).clone();
            }
            let class = self.objs[cur as usize].class;
            if class != C_OBJECT {
                if class == C_PROXY {
                    return self.proxy_get_atom(cur, atom, receiver.clone());
                }
                if (class == C_ARRAY || class == C_ARGUMENTS) && atom == A_LENGTH {
                    return Val::Num(self.evecs[self.objs[cur as usize].elems as usize].v.len() as double);
                }
                if class == C_TYPED && self.canon_numeric(atom) {
                    return Val::Undef;
                }
                if class == C_FUNCTION || class == C_BOUND {
                    if atom == A_LENGTH || atom == A_NAME || atom == A_PROTOTYPE {
                        let v = self.function_own(cur, atom);
                        if !matches!(v, Val::Undef) {
                            return v;
                        }
                    }
                }
                if class == C_STRING && atom == A_LENGTH {
                    if let Val::Str(s) = self.colds[self.objs[cur as usize].cold as usize].prim.clone() {
                        let s = self.sget(s.clone());
                        return Val::Num(self.str_len(&s) as double);
                    }
                }
            }
            cur = self.objs[cur as usize].proto as int;
            hops += 1;
            if hops > 10000 {
                break;
            }
        }
        Val::Undef
    }

    pub fn get(&mut self, v: &Val, atom: int) -> Val {
        match v {
            Val::Obj(o) => self.get_obj(*o, atom, v),
            Val::Str(s) => {
                if atom == A_LENGTH {
                    return Val::Num(self.hlen(*s) as double);
                }
                let p = self.string_proto;
                self.get_obj(p, atom, v)
            }
            Val::Num(_) => {
                let p = self.number_proto;
                self.get_obj(p, atom, v)
            }
            Val::Bool(_) => {
                let p = self.boolean_proto;
                self.get_obj(p, atom, v)
            }
            Val::Big(_) => {
                let p = self.bigint_proto;
                self.get_obj(p, atom, v)
            }
            _ => {
                let name = self.atom_str(atom);
                let what = if matches!(v, Val::Null) { "null" } else { "undefined" };
                self.throw_type(format!("Cannot read properties of {} (reading '{}')", what, name).as_str());
                Val::Undef
            }
        }
    }

    pub fn get_index(&mut self, v: &Val, i: int) -> Val {
        match v {
            Val::Obj(o) => {
                let ob = &self.objs[*o as usize];
                if ob.class == C_ARRAY || ob.class == C_ARGUMENTS {
                    let n = self.evecs[ob.elems as usize].v.len() as int;
                    if i < n && ((ob.pos != 3 && !self.idx_protos) || !self.is_hole(*o, i)) {
                        return self.evecs[ob.elems as usize].v[i as usize].clone();
                    }
                    if ob.class == C_ARRAY {
                        // a hole or past the end: an own index property (an
                        // element property, an index far past the end), then
                        // the prototypes
                        let has_shape = ob.shape != 0;
                        let p = ob.proto as int;
                        if has_shape {
                            let a = self.index_atom(i);
                            if self.find(*o, a) >= 0 {
                                return self.get_obj(*o, a, v);
                            }
                        }
                        if p < 0 || (!self.idx_protos && (p == self.array_proto || p == self.object_proto)) {
                            return Val::Undef;
                        }
                        return self.get_index(&Val::Obj(p), i);
                    }
                }
                let class = ob.class;
                if class == C_STRING {
                    if let Val::Str(s) = self.colds[ob.cold as usize].prim.clone() {
                        let s = self.sget(s.clone());
                        let c = self.str_at(&s, i);
                        if c >= 0 {
                            return self.unit_val(c);
                        }
                    }
                }
                if class == C_TYPED {
                    return self.ta_get_fast(*o, i);
                }
                let a = self.index_atom_if(i);
                if a < 0 {
                    // no key of that name anywhere: only an element up the
                    // chain or a proxy can answer
                    let mut cur = *o;
                    loop {
                        let c = self.objs[cur as usize].class;
                        if c == C_PROXY {
                            let a = self.index_atom(i);
                            return self.get_obj(cur, a, v);
                        }
                        let p = self.objs[cur as usize].proto as int;
                        if p < 0 {
                            return Val::Undef;
                        }
                        let pc = self.objs[p as usize].class;
                        if pc == C_ARRAY || pc == C_ARGUMENTS || pc == C_TYPED || pc == C_STRING {
                            return self.get_index(&Val::Obj(p), i);
                        }
                        cur = p;
                    }
                }
                // an element up the chain (an array or string prototype) is
                // not an atom property
                let mut cur = *o;
                loop {
                    if self.objs[cur as usize].class == C_PROXY || self.find(cur, a) >= 0 {
                        break;
                    }
                    let p = self.objs[cur as usize].proto as int;
                    if p < 0 {
                        break;
                    }
                    let pc = self.objs[p as usize].class;
                    if pc == C_ARRAY || pc == C_ARGUMENTS || pc == C_TYPED || pc == C_STRING {
                        return self.get_index(&Val::Obj(p), i);
                    }
                    cur = p;
                }
                self.get_obj(*o, a, v)
            }
            Val::Str(s) => {
                let h = *s;
                let c0 = self.hunit(h, i);
                if c0 >= 0 {
                    return self.unit_val(c0);
                }
                if c0 == -1 {
                    return Val::Undef;
                }
                let s = self.sget(h);
                let c = self.str_at(&s, i);
                if c >= 0 {
                    return self.unit_val(c);
                }
                Val::Undef
            }
            _ => {
                let a = self.index_atom(i);
                self.get(v, a)
            }
        }
    }

    pub fn get_elem(&mut self, v: &Val, k: &Val) -> Val {
        if let Val::Num(n) = k {
            let i = *n as int;
            if (i as double) == *n && i >= 0 {
                return self.get_index(v, i);
            }
        }
        if matches!(v, Val::Undef) || matches!(v, Val::Null) {
            let ks = self.to_string(k);
            let what = if matches!(v, Val::Null) { "null" } else { "undefined" };
            self.throw_type(format!("Cannot read properties of {} (reading '{}')", what, ks).as_str());
            return Val::Undef;
        }
        let (i, a) = self.to_key(k);
        if self.throwing {
            return Val::Undef;
        }
        if i >= 0 {
            return self.get_index(v, i);
        }
        self.get(v, a)
    }

    /// A proxy up o's prototype chain (a write that misses goes to its trap).
    pub fn proxy_on_chain(&self, o: int) -> bool {
        let mut cur = self.objs[o as usize].proto as int;
        while cur >= 0 {
            if self.objs[cur as usize].class == C_PROXY {
                return true;
            }
            cur = self.objs[cur as usize].proto as int;
        }
        false
    }

    fn setter_on_chain(&mut self, o: int, atom: int, v: &Val, receiver: &Val) -> bool {
        // true when an accessor or read-only property up the chain took it
        let mut cur = self.objs[o as usize].proto as int;
        while cur >= 0 {
            if self.objs[cur as usize].class == C_PROXY {
                let k = self.key_val(atom);
                self.proxy_call(cur, PH_SET, vec![k, v.clone(), receiver.clone()]);
                return true;
            }
            if self.objs[cur as usize].class == C_TYPED {
                // a canonical numeric key (`NaN`, `-0`, `1.5`) on a typed
                // array is not an element: the write does nothing
                let n = self.atom_str(atom);
                if n.as_str() == "-0" || number_to_string(string_to_number(n.as_str())) == n {
                    return true;
                }
            }
            let slot = self.find(cur, atom);
            if slot >= 0 {
                let attr = self.attr_at(cur, slot);
                if (attr & P_ACCESSOR) != 0 {
                    let pair = obj_of(&self.objs[cur as usize].get_slot(slot as usize));
                    let s = self.evecs[self.objs[pair as usize].elems as usize].v[1].clone();
                    if self.is_callable(&s) {
                        self.call_fixed(s, receiver.clone(), 1, v.clone(), Val::Undef, Val::Undef);
                    } else if self.strict_now() {
                        let n = self.atom_str(atom);
                        self.throw_type(format!("Cannot set property {} of #<Object> which has only a getter", n).as_str());
                    }
                    return true;
                }
                if (attr & P_READONLY) != 0 {
                    if self.strict_now() {
                        let n = self.atom_str(atom);
                        self.throw_type(format!("Cannot assign to read only property '{}' of object '#<Object>'", n).as_str());
                    }
                    return true;
                }
                return false;
            }
            cur = self.objs[cur as usize].proto as int;
        }
        false
    }

    pub fn set_obj(&mut self, o: int, atom: int, v: Val) {
        if self.objs[o as usize].class == C_PROXY {
            let ok = self.proxy_set_atom(o, atom, v, Val::Obj(o));
            if !ok && !self.throwing && self.strict_now() {
                let n = self.atom_str(atom);
                self.throw_type(format!("'set' on proxy: trap returned falsish for property '{}'", n).as_str());
            }
            return;
        }
        let slot = self.find(o, atom);
        if slot >= 0 {
            let attr = self.attr_at(o, slot);
            if attr == 0 {
                self.put_slot(o, slot, v);
                return;
            }
            if (attr & P_MAPPED) != 0 {
                self.mapped_put(o, atom, &v);
                self.put_slot(o, slot, v);
                return;
            }
            if (attr & P_ACCESSOR) != 0 {
                let pair = obj_of(&self.objs[o as usize].get_slot(slot as usize));
                let s = self.evecs[self.objs[pair as usize].elems as usize].v[1].clone();
                if self.is_callable(&s) {
                    self.call_fixed(s, Val::Obj(o), 1, v, Val::Undef, Val::Undef);
                } else if self.strict_now() {
                    let n = self.atom_str(atom);
                    self.throw_type(format!("Cannot set property {} of #<Object> which has only a getter", n).as_str());
                }
                return;
            }
            if (attr & P_READONLY) != 0 {
                if self.strict_now() {
                    let n = self.atom_str(atom);
                    self.throw_type(format!("Cannot assign to read only property '{}' of object", n).as_str());
                }
                return;
            }
            self.put_slot(o, slot, v);
            return;
        }
        let class = self.objs[o as usize].class;
        if class == C_TYPED && self.canon_numeric(atom) {
            // not an element: the value is still converted
            self.ta_set(o, -1, &v);
            return;
        }
        if class == C_STRING && atom == A_LENGTH {
            // a String object's length is read-only
            if self.strict_now() {
                self.throw_type("Cannot assign to read only property 'length' of object '[object String]'");
            }
            return;
        }
        if class == C_ARGUMENTS && atom == A_LENGTH {
            // an arguments object's length is its own data property
            self.slow_args(o);
            self.set_obj(o, atom, v);
            return;
        }
        if class == C_ARRAY && atom == A_LENGTH {
            let n = self.to_number(&v);
            if self.objs[o as usize].pos == 2 {
                // a frozen array's length is read-only
                if self.strict_now() {
                    self.throw_type("Cannot assign to read only property 'length' of object '[object Array]'");
                }
                return;
            }
            self.set_length(o, n);
            return;
        }
        // a function's `prototype` not made yet is still its own property:
        // the chain (a class it inherits from) has no say
        let own_prototype = class == C_FUNCTION && atom == A_PROTOTYPE && self.lazy_prototype(o);
        // __proto__: Object.prototype's accessor, when the chain has it
        if !own_prototype && (atom == A_PROTO || self.setter_atom(atom) || (self.proxy_made && self.proxy_on_chain(o))) && self.setter_on_chain(o, atom, &v, &Val::Obj(o)) {
            return;
        }
        if !self.objs[o as usize].extensible {
            if self.strict_now() {
                let n = self.atom_str(atom);
                self.throw_type(format!("Cannot add property {}, object is not extensible", n).as_str());
            }
            return;
        }
        self.add_own(o, atom, v, 0);
    }

    /// `super[atom] = v` in a method of `home`: [[Set]] on home's
    /// prototype with `this` as the receiver (class code: strict).
    pub fn super_set(&mut self, home: int, atom: int, index: bool, v: Val, this: Val) {
        let start = self.objs[home as usize].proto as int;
        if start < 0 {
            self.throw_type("Cannot set properties of null (super)");
            return;
        }
        let mut o = start;
        while o >= 0 {
            if self.objs[o as usize].class == C_PROXY {
                break;
            }
            let slot = self.find(o, atom);
            if slot >= 0 {
                let attr = self.attr_at(o, slot);
                if (attr & P_ACCESSOR) != 0 {
                    let pair = obj_of(&self.objs[o as usize].get_slot(slot as usize));
                    let s = self.evecs[self.objs[pair as usize].elems as usize].v[1].clone();
                    if self.is_callable(&s) {
                        self.call_fixed(s, this, 1, v, Val::Undef, Val::Undef);
                    } else {
                        let n = self.atom_str(atom);
                        self.throw_type(format!("Cannot set property {} which has only a getter", n).as_str());
                    }
                    return;
                }
                if (attr & P_READONLY) != 0 {
                    let n = self.atom_str(atom);
                    self.throw_type(format!("Cannot assign to read only property '{}' of object", n).as_str());
                    return;
                }
                break;
            }
            o = self.objs[o as usize].proto as int;
        }
        let r = obj_of(&this);
        if r < 0 {
            let n = self.atom_str(atom);
            self.throw_type(format!("Cannot create property '{}' on a primitive", n).as_str());
            return;
        }
        let class = self.objs[r as usize].class;
        if class == C_PROXY || class == C_ARRAY || index {
            self.set_obj(r, atom, v);
            return;
        }
        let slot = self.find(r, atom);
        if slot >= 0 {
            let attr = self.attr_at(r, slot);
            if (attr & (P_ACCESSOR | P_READONLY)) != 0 {
                let n = self.atom_str(atom);
                self.throw_type(format!("Cannot assign to read only property '{}' of object", n).as_str());
                return;
            }
            self.put_slot(r, slot, v);
            return;
        }
        if !self.objs[r as usize].extensible {
            let n = self.atom_str(atom);
            self.throw_type(format!("Cannot add property {}, object is not extensible", n).as_str());
            return;
        }
        self.add_own(r, atom, v, 0);
    }

    fn strict_now(&self) -> bool {
        if (self.frames.len() as int) == self.strict_at {
            return true;
        }
        if self.frames.is_empty() {
            return false;
        }
        let f = &self.frames[self.frames.len() - 1];
        self.protos[f.proto as usize].strict
    }

    pub fn set_length(&mut self, o: int, n: double) {
        if n != n || n < 0.0 || n > 4294967295.0 || n.floor() != n {
            self.throw_range("Invalid array length");
            return;
        }
        let len = n as int;
        let cur = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
        if len > cur + 100000000 {
            // elements are stored densely: a length this far out is refused
            self.throw_range("Invalid array length");
            return;
        }
        if len < cur && self.objs[o as usize].pos == 3 {
            // element properties at len.. go too, down to a non-configurable one
            let mut k = cur - 1;
            while k >= len {
                if self.is_hole(o, k) {
                    let a = self.index_atom(k);
                    let sl = self.find(o, a);
                    if sl >= 0 {
                        if (self.attr_at(o, sl) & P_FIXED) != 0 {
                            self.cut_elems(o, k + 1);
                            if self.colds[self.objs[o as usize].cold as usize].elems2.len() > (k + 1) as usize {
                                self.cut_elems2(o, k + 1);
                            }
                            if self.strict_now() {
                                self.throw_type(format!("Cannot delete property '{}' of [object Array]", k).as_str());
                            }
                            return;
                        }
                        self.remove_own(o, sl);
                    }
                }
                k -= 1;
            }
        }
        if len < cur {
            self.cut_elems(o, len);
            if self.colds[self.objs[o as usize].cold as usize].elems2.len() > len as usize {
                self.cut_elems2(o, len);
            }
        } else {
            let mut i = cur;
            while i < len {
                self.push_elem(o, Val::Undef);
                i += 1;
            }
            let mut h = cur;
            while h < len {
                self.mark_hole(o, h);
                h += 1;
            }
        }
    }

    pub fn set(&mut self, target: &Val, atom: int, v: Val) {
        match target {
            Val::Obj(o) => self.set_obj(*o, atom, v),
            Val::Undef | Val::Null => {
                let n = self.atom_str(atom);
                let what = if matches!(target, Val::Null) { "null" } else { "undefined" };
                self.throw_type(format!("Cannot set properties of {} (setting '{}')", what, n).as_str());
            }
            _ => {
                if self.strict_now() && !self.any_setter {
                    let n = self.atom_str(atom);
                    self.throw_type(format!("Cannot create property '{}' on primitive", n).as_str());
                }
            }
        }
    }

    /// A CanonicalNumericIndexString key that is not an array index
    /// (`-0`, `1.5`, `-1`, `Infinity`, `NaN`): on a typed array it names no
    /// property.
    pub fn canon_numeric(&mut self, atom: int) -> bool {
        let k = self.atom_kind(atom);
        if k == 3 || k == 4 {
            return false;
        }
        // what a number prints as starts with a digit, '-', "Infinity" or
        // "NaN": any other key without converting it (a typed array's
        // methods and accessors are looked up through here)
        let c0 = jsstr::first_byte(self.atom_names[atom as usize].as_str());
        if !((c0 >= 48 && c0 <= 57) || c0 == 45 || c0 == 73 || c0 == 78) {
            return false;
        }
        let n = self.atom_str(atom);
        n.as_str() == "-0" || number_to_string(string_to_number(n.as_str())) == n
    }

    /// An array whose length was made read-only (defineProperty): it keeps
    /// an own `length` property then.
    pub fn len_readonly(&self, o: int) -> bool {
        self.objs[o as usize].shape != 0 && self.find(o, A_LENGTH) >= 0
    }

    pub fn set_index(&mut self, o: int, i: int, v: Val) {
        let class = self.objs[o as usize].class;
        if o == self.array_proto || o == self.object_proto {
            self.idx_protos = true;
        }
        if class == C_TYPED {
            if let Val::Num(x) = v {
                if self.ta_put_num(o, i, x) {
                    return;
                }
            }
            self.ta_set(o, i, &v);
            return;
        }
        if class == C_ARRAY || class == C_ARGUMENTS {
            let len = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
            if i < len {
                if class == C_ARRAY && self.objs[o as usize].pos == 2 {
                    // a frozen array's elements are read-only
                    if self.strict_now() {
                        self.throw_type(format!("Cannot assign to read only property '{}' of object '[object Array]'", i).as_str());
                    }
                    return;
                }
                if class == C_ARRAY && self.objs[o as usize].pos == 3 && self.is_hole(o, i) {
                    // an element kept as a property (attributes, accessor)
                    let a = self.index_atom(i);
                    if self.find(o, a) >= 0 {
                        self.set_obj(o, a, v);
                        return;
                    }
                }
                self.put_elem(o, i, v);
                if class == C_ARRAY && !self.colds[self.objs[o as usize].cold as usize].elems2.is_empty() {
                    self.clear_hole(o, i);
                }
                return;
            }
            if class == C_ARRAY && self.objs[o as usize].pos == 3 && self.len_readonly(o) {
                // past a read-only length: no new element
                if self.strict_now() {
                    self.throw_type(format!("Cannot add property {}, object length is read-only", i).as_str());
                }
                return;
            }
            if class == C_ARRAY && self.objs[o as usize].extensible && i < len + 50000000 {
                let mut k = len;
                while k < i {
                    self.push_elem(o, Val::Undef);
                    k += 1;
                }
                self.push_elem(o, v);
                if i > len {
                    let mut h = len;
                    while h < i {
                        self.mark_hole(o, h);
                        h += 1;
                    }
                }
                return;
            }
            if class == C_ARGUMENTS {
                // past the end: a property, length stays
                self.slow_args(o);
            }
        }
        let a = self.index_atom(i);
        self.set_obj(o, a, v);
    }

    pub fn set_elem(&mut self, target: &Val, k: &Val, v: Val) {
        if let Val::Obj(o) = target {
            if let Val::Num(n) = k {
                let i = *n as int;
                if (i as double) == *n && i >= 0 {
                    self.set_index(*o, i, v);
                    return;
                }
            }
            let (i, a) = self.to_key(k);
            if self.throwing {
                return;
            }
            if i >= 0 {
                self.set_index(*o, i, v);
            } else {
                self.set_obj(*o, a, v);
            }
            return;
        }
        if matches!(target, Val::Undef) || matches!(target, Val::Null) {
            let ks = self.to_string(k);
            let what = if matches!(target, Val::Null) { "null" } else { "undefined" };
            self.throw_type(format!("Cannot set properties of {} (setting '{}')", what, ks).as_str());
        }
    }

    /// Defines (or redefines) an own data property.
    /// The object holding private key `a` for `o`: `o` itself (a field, a
    /// static member) or, through `o`'s brand, its class's prototype (a
    /// method, an accessor); -1 when `o` has neither.
    pub fn priv_holder(&self, o: int, a: int, brand: int) -> int {
        if self.find(o, a) >= 0 {
            return o;
        }
        if brand >= 0 {
            let s = self.find(o, brand);
            if s >= 0 {
                let home = obj_of(&self.objs[o as usize].get_slot(s as usize));
                if home >= 0 && self.find(home, a) >= 0 {
                    return home;
                }
            }
        }
        -1
    }

    pub fn throw_private(&mut self, a: int, what: &str) {
        let n = self.atom_str(a);
        let mut shown = n.clone();
        let cut = jsstr::index_of(n.as_str(), "\u{2}", 0);
        if cut >= 0 {
            shown = jsstr::slice(n.as_str(), 0, cut);
        }
        if shown.starts_with('\u{1}') {
            shown = jsstr::slice(shown.as_str(), 1, jsstr::len(shown.as_str()));
        }
        self.throw_type(format!("Cannot {} private member {} from an object whose class did not declare it", what, shown).as_str());
    }

    /// obj.#x
    pub fn priv_get(&mut self, v: &Val, a: int, brand: int) -> Val {
        let o = obj_of(v);
        let h = if o >= 0 { self.priv_holder(o, a, brand) } else { -1 };
        if h < 0 {
            self.throw_private(a, "read");
            return Val::Undef;
        }
        let slot = self.find(h, a);
        if (self.attr_at(h, slot) & P_ACCESSOR) != 0 {
            let pair = obj_of(&self.objs[h as usize].get_slot(slot as usize));
            let g = self.evecs[self.objs[pair as usize].elems as usize].v[0].clone();
            if !self.is_callable(&g) {
                self.throw_private(a, "read (no getter)");
                return Val::Undef;
            }
            return self.call_fixed(g, v.clone(), 0, Val::Undef, Val::Undef, Val::Undef);
        }
        self.objs[h as usize].get_slot(slot as usize).clone()
    }

    /// obj.#x = val; kind 1: a method (not writable)
    pub fn priv_set(&mut self, v: &Val, a: int, brand: int, kind: int, val: Val) {
        let o = obj_of(v);
        let h = if o >= 0 { self.priv_holder(o, a, brand) } else { -1 };
        if h < 0 {
            self.throw_private(a, "write");
            return;
        }
        let slot = self.find(h, a);
        if (self.attr_at(h, slot) & P_ACCESSOR) != 0 {
            let pair = obj_of(&self.objs[h as usize].get_slot(slot as usize));
            let f = self.evecs[self.objs[pair as usize].elems as usize].v[1].clone();
            if !self.is_callable(&f) {
                self.throw_private(a, "write (no setter)");
                return;
            }
            self.call_fixed(f, v.clone(), 1, val, Val::Undef, Val::Undef);
            return;
        }
        if kind == 1 || h != o {
            self.throw_private(a, "assign to a private method");
            return;
        }
        self.put_slot(h, slot, val);
    }

    pub fn define(&mut self, o: int, atom: int, v: Val, attr: int) {
        let slot = self.find(o, atom);
        if slot >= 0 {
            self.put_slot(o, slot, v);
            self.set_attr(o, slot, attr);
        } else {
            self.add_own(o, atom, v, attr);
        }
        if (attr & P_READONLY) != 0 {
            self.any_setter = true;
            self.mark_setter(atom);
        }
    }

    pub fn define_elem(&mut self, o: int, k: &Val, v: Val, attr: int) {
        let (i, a) = self.to_key(k);
        if self.throwing {
            return;
        }
        if i >= 0 && self.objs[o as usize].class == C_ARRAY && attr == 0 {
            self.set_index(o, i, v);
            return;
        }
        let atom = if i >= 0 { self.index_atom(i) } else { a };
        self.define(o, atom, v, attr);
    }

    /// Defines a getter (`which` 0) or setter (1).
    pub fn define_accessor(&mut self, o: int, atom: int, f: Val, which: int, hidden: bool) {
        self.any_setter = true;
        self.mark_setter(atom);
        let slot = self.find(o, atom);
        let mut pair: int = -1;
        if slot >= 0 && (self.attr_at(o, slot) & P_ACCESSOR) != 0 {
            pair = obj_of(&self.objs[o as usize].get_slot(slot as usize));
        }
        if pair < 0 {
            pair = self.alloc(C_ACCESSOR, -1);
            self.put_elems(pair, vec![Val::Undef, Val::Undef]);
            let attr = P_ACCESSOR | if hidden { P_HIDDEN } else { 0 };
            if slot >= 0 {
                self.put_slot(o, slot, Val::Obj(pair));
                self.set_attr(o, slot, attr);
            } else {
                self.add_own(o, atom, Val::Obj(pair), attr);
            }
        }
        self.put_elem(pair, which, f);
    }

    pub fn has_property(&mut self, o: int, k: &Val) -> bool {
        let (i, a) = self.to_key(k);
        if i >= 0 {
            let class = self.objs[o as usize].class;
            if (class == C_ARRAY || class == C_ARGUMENTS) && i < self.evecs[self.objs[o as usize].elems as usize].v.len() as int && !self.is_hole(o, i) {
                return true;
            }
            if class == C_TYPED {
                return i < self.ta_length(o);
            }
            if class == C_STRING {
                if let Val::Str(s) = &self.colds[self.objs[o as usize].cold as usize].prim {
                    let s = self.sget(s.clone());
                    if i < jsstr::len(s.as_str()) {
                        return true;
                    }
                }
            }
            // an own index property, then the prototypes (their elements too)
            let at = self.index_atom(i);
            if class == C_PROXY {
                return self.has_atom(o, at);
            }
            if self.find(o, at) >= 0 {
                return true;
            }
            let p = self.objs[o as usize].proto as int;
            if p < 0 {
                return false;
            }
            return self.has_property(p, k);
        }
        self.has_atom(o, a)
    }

    pub fn has_atom(&mut self, o: int, a: int) -> bool {
        let mut cur = o;
        while cur >= 0 {
            if self.objs[cur as usize].class == C_PROXY {
                let k = self.key_val(a);
                return self.proxy_bool(cur, PH_HAS, vec![k]);
            }
            if self.find(cur, a) >= 0 {
                return true;
            }
            let class = self.objs[cur as usize].class;
            if a == A_LENGTH && (class == C_ARRAY || class == C_ARGUMENTS || class == C_STRING) {
                return true;
            }
            if class == C_TYPED && self.canon_numeric(a) {
                return false;
            }
            if (a == A_NAME || a == A_LENGTH) && (class == C_FUNCTION || class == C_BOUND) && !self.vgone(cur, a) {
                return true;
            }
            if a == A_PROTOTYPE && class == C_FUNCTION {
                let v = self.function_own(cur, a);
                if !matches!(v, Val::Undef) {
                    return true;
                }
            }
            cur = self.objs[cur as usize].proto as int;
        }
        false
    }

    pub fn has_own(&mut self, o: int, k: &Val) -> bool {
        let (i, a) = self.to_key(k);
        let class = self.objs[o as usize].class;
        if i >= 0 {
            if (class == C_ARRAY || class == C_ARGUMENTS) && i < self.evecs[self.objs[o as usize].elems as usize].v.len() as int && !self.is_hole(o, i) {
                return true;
            }
            if class == C_TYPED {
                return i < self.ta_length(o);
            }
            if class == C_STRING {
                if let Val::Str(s) = &self.colds[self.objs[o as usize].cold as usize].prim {
                    let s = self.sget(s.clone());
                    if i < jsstr::len(s.as_str()) {
                        return true;
                    }
                }
            }
            let at = self.index_atom(i);
            return self.find(o, at) >= 0;
        }
        if self.find(o, a) >= 0 {
            return true;
        }
        if a == A_LENGTH && (class == C_ARRAY || class == C_ARGUMENTS || class == C_STRING) {
            return true;
        }
        if (a == A_NAME || a == A_LENGTH) && (class == C_FUNCTION || class == C_BOUND) {
            return !self.vgone(o, a);
        }
        if a == A_PROTOTYPE && class == C_FUNCTION {
            let v = self.function_own(o, a);
            return !matches!(v, Val::Undef);
        }
        false
    }

    pub fn delete(&mut self, o: int, k: &Val) -> bool {
        let (i, a) = self.to_key(k);
        let class = self.objs[o as usize].class;
        if class == C_PROXY {
            let atom = if i >= 0 { self.index_atom(i) } else { a };
            let key = self.key_val(atom);
            let ok = self.proxy_bool(o, PH_DELETE, vec![key]);
            if !ok && !self.throwing && self.strict_now() {
                let n = self.atom_str(atom);
                self.throw_type(format!("'deleteProperty' on proxy: trap returned falsish for property '{}'", n).as_str());
            }
            return ok;
        }
        if class == C_TYPED && (i >= 0 || self.canon_numeric(a)) {
            // an element in range stays; any other numeric key names nothing
            if i >= 0 && i < self.ta_length(o) {
                if self.strict_now() {
                    self.throw_type(format!("Cannot delete property '{}' of [object TypedArray]", i).as_str());
                }
                return false;
            }
            return true;
        }
        if class == C_ARGUMENTS && (i >= 0 || a == A_LENGTH) {
            self.slow_args(o);
            return self.delete(o, k);
        }
        if i >= 0 && class == C_ARRAY {
            let len = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
            if class == C_ARRAY && i < len && self.objs[o as usize].pos == 2 && !self.is_hole(o, i) {
                // a frozen array's elements stay
                if self.strict_now() {
                    self.throw_type(format!("Cannot delete property '{}' of [object Array]", i).as_str());
                }
                return false;
            }
            let as_prop = class == C_ARRAY && i < len && self.objs[o as usize].pos == 3 && self.is_hole(o, i);
            if i < len && !as_prop {
                if i == len - 1 && class == C_ARGUMENTS {
                    self.pop_elem(o);
                } else {
                    self.put_elem(o, i, Val::Undef);
                    if class == C_ARRAY {
                        self.mark_hole(o, i);
                    }
                }
            }
            if !as_prop {
                return true;
            }
        }
        let atom = if i >= 0 { self.index_atom_if(i) } else { a };
        if atom < 0 {
            // an index no key has had: nothing to delete
            return true;
        }
        let slot = self.find(o, atom);
        if slot >= 0 {
            if (self.attr_at(o, slot) & P_FIXED) != 0 {
                if self.strict_now() {
                    let n = self.atom_str(atom);
                    self.throw_type(format!("Cannot delete property '{}'", n).as_str());
                }
                return false;
            }
            self.remove_own(o, slot);
        }
        if (atom == A_NAME || atom == A_LENGTH) && (class == C_FUNCTION || class == C_BOUND) && !self.vgone(o, atom) {
            let m = self.vgone_atom(atom);
            self.add_own(o, m, Val::Undef, P_HIDDEN);
        }
        true
    }

    /// Own enumerable string keys in the order JavaScript lists them:
    /// integer keys ascending, then the others as they were added.
    pub fn own_keys(&mut self, o: int, include_hidden: bool, symbols: bool) -> Vec<Val> {
        let mut out: Vec<Val> = Vec::new();
        let class = self.objs[o as usize].class;
        if class == C_PROXY {
            return self.proxy_keys(o, !include_hidden, symbols);
        }
        let mixed = class == C_ARRAY && self.objs[o as usize].pos == 3;
        let mut elem_ix: Vec<int> = Vec::new();
        if class == C_ARRAY || class == C_ARGUMENTS {
            let n = self.evecs[self.objs[o as usize].elems as usize].v.len();
            let mut i: usize = 0;
            while i < n {
                if !self.is_hole(o, i as int) {
                    if mixed {
                        elem_ix.push(i as int);
                    } else {
                        out.push(self.string_val(int_to_string(i as int)));
                    }
                }
                i += 1;
            }
        }
        if class == C_STRING {
            if let Val::Str(s) = &self.colds[self.objs[o as usize].cold as usize].prim {
                let s = self.sget(s.clone());
                let n = jsstr::len(s.as_str());
                let mut i: int = 0;
                while i < n {
                    out.push(self.string_val(int_to_string(i as int)));
                    i += 1;
                }
            }
        }
        if class == C_TYPED {
            let n = self.ta_length(o);
            let mut i: int = 0;
            while i < n {
                out.push(self.string_val(int_to_string(i as int)));
                i += 1;
            }
        }
        // the keys are read from the shape in place: atom_kind only reads
        let sh = self.objs[o as usize].shape as int;
        let nk = self.shapes[sh as usize].nkeys as usize;
        let hk = self.holder(sh);
        let mut ints: Vec<int> = Vec::new();
        let mut others: Vec<int> = Vec::with_capacity(nk);
        reserve_vals(&mut out, nk);
        let mut syms: Vec<int> = Vec::new();
        let mut k: usize = 0;
        while k < nk {
            let a = self.shapes[hk].keys[k];
            if (self.shapes[hk].attrs[k] & P_HIDDEN) != 0 && !include_hidden {
                k += 1;
                continue;
            }
            let kind = self.atom_kind(a);
            if kind == 4 {
                // a private name: never a key
            } else if kind == 3 {
                syms.push(a);
            } else if kind == 2 {
                ints.push(a);
            } else {
                others.push(a);
            }
            k += 1;
        }
        if ints.len() > 1 {
            // by numeric value: in order already (keys added ascending), else
            // a merge sort (n log n; insertion order can be anything)
            let mut keys: Vec<double> = Vec::with_capacity(ints.len());
            let mut sorted = true;
            let mut i: usize = 0;
            while i < ints.len() {
                let kv = index_value(self.atom_names[ints[i] as usize].as_str());
                if i > 0 && kv < keys[i - 1] {
                    sorted = false;
                }
                keys.push(kv);
                i += 1;
            }
            if !sorted {
                sort_ids_by_keys(&mut ints, &keys);
            }
        }
        if mixed {
            // elements and element properties (pos 3), in index order
            let mut j: usize = 0;
            for a in ints {
                let ix = index_value(self.atom_names[a as usize].as_str());
                while j < elem_ix.len() && (elem_ix[j] as double) < ix {
                    let ev = self.string_val(int_to_string(elem_ix[j]));
                    out.push(ev);
                    j += 1;
                }
                let v = self.atom_val(a);
                out.push(v);
            }
            while j < elem_ix.len() {
                let ev = self.string_val(int_to_string(elem_ix[j]));
                out.push(ev);
                j += 1;
            }
        } else {
            for a in ints {
                let v = self.atom_val(a);
                out.push(v);
            }
        }
        for a in others {
            let v = self.atom_val(a);
            out.push(v);
        }
        if symbols {
            for a in syms {
                out.push(self.key_val(a));
            }
        }
        out
    }

    /// own_keys(o, false, false) of an ordinary object (no elements, not a
    /// proxy) as atoms: integer keys ascending, then the rest in order.
    pub fn own_string_atoms(&mut self, o: int) -> Vec<int> {
        let sh = self.objs[o as usize].shape as int;
        let nk = self.shapes[sh as usize].nkeys as usize;
        let hk = self.holder(sh);
        let mut ints: Vec<int> = Vec::new();
        let mut others: Vec<int> = Vec::with_capacity(nk);
        let mut k: usize = 0;
        while k < nk {
            let a = self.shapes[hk].keys[k];
            if (self.shapes[hk].attrs[k] & P_HIDDEN) == 0 {
                let kind = self.atom_kind(a);
                if kind == 2 {
                    ints.push(a);
                } else if kind == 1 {
                    others.push(a);
                }
            }
            k += 1;
        }
        if ints.len() > 1 {
            let mut keys: Vec<double> = Vec::with_capacity(ints.len());
            let mut sorted = true;
            let mut i: usize = 0;
            while i < ints.len() {
                let kv = index_value(self.atom_names[ints[i] as usize].as_str());
                if i > 0 && kv < keys[i - 1] {
                    sorted = false;
                }
                keys.push(kv);
                i += 1;
            }
            if !sorted {
                sort_ids_by_keys(&mut ints, &keys);
            }
        }
        for a in others {
            ints.push(a);
        }
        ints
    }

    /// Keys a for-in visits: own and inherited enumerable string keys.
    /// Whether for-in iterator `it` checks each key is still there: not
    /// while no own property was removed since it began (ITER_KEYS).
    pub fn for_in_recheck(&self, it: int) -> bool {
        match &self.colds[self.objs[it as usize].cold as usize].prim {
            Val::Num(d) => (*d as int) != self.del_count,
            _ => true,
        }
    }

    pub fn for_in_keys(&mut self, o: int) -> Vec<Val> {
        // the common case: the prototypes add no enumerable keys (an object
        // literal over Object.prototype, a class instance), so the keys are
        // the object's own, with nothing to shadow
        if self.for_in_own_only(o) {
            return self.own_keys(o, false, false);
        }
        let mut out: Vec<Val> = Vec::new();
        let mut seen: HashMap<String, bool> = HashMap::new();
        let mut cur = o;
        let mut first = true;
        while cur >= 0 {
            let all = self.own_keys(cur, true, false);
            let visible = self.own_keys(cur, false, false);
            let mut vis: HashMap<String, bool> = HashMap::new();
            for v in visible.iter() {
                if let Val::Str(s) = v {
                    let s = self.sget(s.clone());
                    vis.insert(s.as_ref().clone(), true);
                }
            }
            for v in all {
                if let Val::Str(s) = &v {
                    let s = self.sget(s.clone());
                    let key = s.as_ref().clone();
                    if seen.contains_key(&key) {
                        continue;
                    }
                    seen.insert(key.clone(), true);
                    if vis.contains_key(&key) {
                        out.push(v.clone());
                    }
                }
            }
            let _ = first;
            first = false;
            cur = self.objs[cur as usize].proto as int;
        }
        out
    }

    /// A for-in over `o` can walk its shape in place: a plain object with
    /// no elements on a shared shape with no index-like keys, whose
    /// prototypes add no keys (for_in_own_only).
    pub fn for_in_fast(&mut self, o: int) -> bool {
        let ob = &self.objs[o as usize];
        if ob.class != C_OBJECT || (ob.elems != 0 && !self.evecs[ob.elems as usize].v.is_empty()) {
            return false;
        }
        let sh = ob.shape as int;
        if self.shapes[sh as usize].owned {
            return false;
        }
        let mut f = self.shapes[sh as usize].for_in;
        if f == 0 {
            // a shared shape never changes: whether it has an index-like
            // key (which for-in visits first, in order) is asked once
            f = 1;
            let nk = self.shapes[sh as usize].nkeys as usize;
            let hk = self.holder(sh);
            let mut k: usize = 0;
            while k < nk {
                let a = self.shapes[hk].keys[k];
                if self.atom_kind(a) == 2 {
                    f = 2;
                    break;
                }
                k += 1;
            }
            self.shapes[sh as usize].for_in = f;
        }
        f == 1 && self.for_in_own_only(o)
    }

    /// The iterator of a for-in that walks `o`'s shape: env the object,
    /// func the next key's index, pos 3 | shape << 2.
    pub fn for_in_iter(&mut self, o: int) -> int {
        let ip = self.iter_proto;
        let it = self.alloc(C_ITER, ip);
        self.put_env(it, o);
        let sh = self.objs[o as usize].shape;
        let ob = &mut self.objs[it as usize];
        ob.func = 0;
        ob.pos = 3 | (sh << 2);
        it
    }

    /// The next key of a shape-walking for-in (for_in_iter) from index `i`
    /// of shape `sh`: hidden keys, symbols and private names are skipped,
    /// and once the object's shape is another, keys it no longer has.
    fn for_in_next(&mut self, it: int, src: int, i: int, sh: int) -> Option<Val> {
        let nk = self.shapes[sh as usize].nkeys;
        let hk = self.holder(sh);
        let mut k = i;
        while k < nk {
            let a = self.shapes[hk].keys[k as usize];
            let attr = self.shapes[hk].attrs[k as usize];
            k += 1;
            if (attr & P_HIDDEN) != 0 {
                continue;
            }
            let kind = self.atom_kind(a);
            if kind == 3 || kind == 4 {
                continue;
            }
            let v = self.atom_val(a);
            if self.objs[src as usize].shape as int != sh && !self.has_property(src, &v) {
                // deleted since the loop began
                continue;
            }
            self.objs[it as usize].func = k as i32;
            return Some(v);
        }
        self.put_env(it, -1);
        None
    }

    /// No object on o's prototype chain (o itself aside) is a proxy or
    /// has an index or an enumerable string key.
    fn for_in_own_only(&self, o: int) -> bool {
        if self.objs[o as usize].class == C_PROXY {
            return false;
        }
        let mut cur = self.objs[o as usize].proto as int;
        while cur >= 0 {
            let c = self.objs[cur as usize].class;
            if c == C_PROXY || c == C_STRING || c == C_TYPED {
                return false;
            }
            if (c == C_ARRAY || c == C_ARGUMENTS) && !self.evecs[self.objs[cur as usize].elems as usize].v.is_empty() {
                return false;
            }
            let n = self.nkeys(cur) as usize;
            let mut k: usize = 0;
            while k < n {
                if (self.attr_at(cur, k as int) & P_HIDDEN) == 0 {
                    let a = self.key_at(cur, k as int);
                    if self.atom_names[a as usize].starts_with('\u{1}') {
                        // a private name: never a key
                    } else if self.symbol_atoms.contains_key(&a) {
                        // a symbol: for-in skips it
                    } else {
                        return false;
                    }
                }
                k += 1;
            }
            cur = self.objs[cur as usize].proto as int;
        }
        true
    }

    pub fn instance_of(&mut self, v: &Val, f: &Val) -> bool {
        if !is_obj(f) {
            self.throw_type("Right-hand side of 'instanceof' is not callable");
            return false;
        }
        let fo = obj_of(f);
        // a function straight on Function.prototype, no own @@hasInstance,
        // and Function.prototype and Object.prototype as they were when
        // neither had one (same shapes, same shape_epoch): no lookup
        let fp = self.function_proto;
        let op = self.object_proto;
        let direct = fp >= 0 && op >= 0 && self.objs[fo as usize].proto as int == fp && self.objs[fp as usize].proto as int == op && self.find(fo, A_HASINSTANCE) < 0;
        let plain = direct && self.hi_epoch == self.shape_epoch && self.hi_fp == self.objs[fp as usize].shape as int && self.hi_op == self.objs[op as usize].shape as int;
        let hi = if plain { Val::Undef } else { self.get_obj(fo, A_HASINSTANCE, f) };
        if direct && !plain && matches!(hi, Val::Undef) && self.find(fp, A_HASINSTANCE) < 0 && self.find(op, A_HASINSTANCE) < 0 {
            self.hi_epoch = self.shape_epoch;
            self.hi_fp = self.objs[fp as usize].shape as int;
            self.hi_op = self.objs[op as usize].shape as int;
        }
        if self.is_callable(&hi) && self.objs[obj_of(&hi) as usize].class != C_NATIVE {
            let r = self.call_fixed(hi, f.clone(), 1, v.clone(), Val::Undef, Val::Undef);
            return self.truthy_v(&r);
        }
        if !self.is_callable(f) {
            self.throw_type("Right-hand side of 'instanceof' is not callable");
            return false;
        }
        let mut target = fo;
        while self.objs[target as usize].class == C_BOUND {
            target = self.objs[target as usize].env as int;
        }
        let o = match v {
            Val::Obj(o) => *o,
            _ => return false,
        };
        // a script function's own data `prototype` read from its slot
        let ps = if self.objs[target as usize].class == C_FUNCTION { self.find(target, A_PROTOTYPE) } else { -1 };
        let p = if ps >= 0 && (self.attr_at(target, ps) & (P_ACCESSOR | P_MAPPED)) == 0 {
            self.objs[target as usize].get_slot(ps as usize)
        } else {
            self.get_obj(target, A_PROTOTYPE, &Val::Obj(target))
        };
        let proto = match p {
            Val::Obj(p) => p,
            _ => {
                self.throw_type("Function has non-object prototype in instanceof check");
                return false;
            }
        };
        let mut cur = self.proto_of(o);
        while cur >= 0 {
            if cur == proto {
                return true;
            }
            cur = self.proto_of(cur);
        }
        false
    }

    /// `v instanceof f` for a script function `f` straight on an unchanged
    /// Function.prototype (instance_of's `plain` case) whose `prototype` is
    /// a data property: 1 / 0, or -1 for instance_of to answer. A memo of
    /// the function's shape saves the two key lookups.
    #[inline(always)]
    pub fn instanceof_fast(&mut self, v: &Val, f: &Val) -> int {
        let fo = match f {
            Val::Obj(o) => *o,
            _ => return -1,
        };
        if self.objs[fo as usize].class != C_FUNCTION {
            return -1;
        }
        let fp = self.function_proto;
        let op = self.object_proto;
        if fp < 0 || op < 0 || self.objs[fo as usize].proto as int != fp || self.objs[fp as usize].proto as int != op || self.hi_epoch != self.shape_epoch || self.hi_fp != self.objs[fp as usize].shape as int || self.hi_op != self.objs[op as usize].shape as int {
            return -1;
        }
        let sh = self.objs[fo as usize].shape as int;
        if sh != self.io_shape {
            if self.shapes[sh as usize].owned || self.find(fo, A_HASINSTANCE) >= 0 {
                return -1;
            }
            let ps = self.find(fo, A_PROTOTYPE);
            self.io_shape = sh;
            self.io_slot = if ps >= 0 && (self.attr_at(fo, ps) & (P_ACCESSOR | P_MAPPED)) == 0 { ps } else { -1 };
        }
        if self.io_slot < 0 {
            return -1;
        }
        let o = match v {
            Val::Obj(o) => *o,
            _ => return 0,
        };
        let proto = match self.objs[fo as usize].get_slot(self.io_slot as usize) {
            Val::Obj(p) => p,
            _ => return -1,
        };
        let mut cur = o;
        loop {
            if self.objs[cur as usize].class == C_PROXY {
                return -1;
            }
            cur = self.objs[cur as usize].proto as int;
            if cur < 0 {
                return 0;
            }
            if cur == proto {
                return 1;
            }
        }
    }

    /// [[GetPrototypeOf]]: a proxy asks its handler.
    pub fn proto_of(&mut self, o: int) -> int {
        if self.objs[o as usize].class == C_PROXY {
            let p = self.proxy_call(o, PH_GETPROTO, Vec::new());
            return obj_of(&p);
        }
        self.objs[o as usize].proto as int
    }

    // ---- arithmetic helpers

    pub fn add_vals(&mut self, a: &Val, b: &Val) -> Val {
        match (a, b) {
            (Val::Num(x), Val::Num(y)) => return Val::Num(x + y),
            (Val::Str(x), Val::Str(y)) => {
                if *x == 0 {
                    return Val::Str(*y);
                }
                if *y == 0 {
                    return Val::Str(*x);
                }
                // a short string before a long one: into the room before it
                let yb = self.hbytes(*y);
                if yb >= 32 && self.hbytes(*x) * 4 <= yb {
                    let xt = self.sget(*x);
                    let r = self.str_prepend(xt.as_str(), *y);
                    if r >= 0 {
                        return Val::Str(r);
                    }
                }
                let n = self.sref(*x).as_bytes().len() + self.sref(*y).as_bytes().len();
                let h = self.take_handle(n as int, 0);
                let mut s = rc_take_buf(&mut self.strs[h as usize].s, n);
                s.push_str(self.sref(*x));
                s.push_str(self.sref(*y));
                rc_refill(&mut self.strs[h as usize].s, s);
                return Val::Str(h);
            }
            (Val::Str(x), Val::Num(y)) => {
                // "k" + i: a small whole number's digits straight onto the copy
                if *y >= 0.0 && *y < 2147483647.0 && *y == ((*y as int) as double) {
                    let n = self.sref(*x).as_bytes().len() + 10;
                    let h = self.take_handle(n as int, 0);
                    let mut s = rc_take_buf(&mut self.strs[h as usize].s, n);
                    s.push_str(self.sref(*x));
                    if push_small_int(&mut s, *y) >= 0 {
                        rc_refill(&mut self.strs[h as usize].s, s);
                        return Val::Str(h);
                    }
                    s.push_str(number_to_string(*y).as_str());
                    rc_refill(&mut self.strs[h as usize].s, s);
                    return Val::Str(h);
                }
                let t = number_to_string(*y);
                let n = self.sref(*x).as_bytes().len() + t.as_bytes().len();
                let h = self.take_handle(n as int, 0);
                let mut s = rc_take_buf(&mut self.strs[h as usize].s, n);
                s.push_str(self.sref(*x));
                s.push_str(t.as_str());
                rc_refill(&mut self.strs[h as usize].s, s);
                return Val::Str(h);
            }
            (Val::Num(x), Val::Str(y)) => {
                let t = number_to_string(*x);
                let n = t.as_bytes().len() + self.sref(*y).as_bytes().len();
                let h = self.take_handle(n as int, 0);
                let mut s = rc_take_buf(&mut self.strs[h as usize].s, n);
                s.push_str(t.as_str());
                s.push_str(self.sref(*y));
                rc_refill(&mut self.strs[h as usize].s, s);
                return Val::Str(h);
            }
            _ => {}
        }
        if self.class_of(a) == C_BIGINT && self.class_of(b) == C_BIGINT {
            return self.big_arith(OP_ADD, a, b);
        }
        let pa = self.to_primitive(a, "default");
        if self.throwing {
            return Val::Undef;
        }
        let pb = self.to_primitive(b, "default");
        if self.throwing {
            return Val::Undef;
        }
        if matches!(pa, Val::Str(_)) || matches!(pb, Val::Str(_)) {
            let x = self.to_str(&pa);
            let y = self.to_str(&pb);
            if self.throwing {
                return Val::Undef;
            }
            let mut s = String::new();
            s.push_str(x.as_str());
            s.push_str(y.as_str());
            return self.string_val(s);
        }
        if self.class_of(&pa) == C_BIGINT || self.class_of(&pb) == C_BIGINT {
            return self.big_arith(OP_ADD, &pa, &pb);
        }
        let x = self.to_number(&pa);
        let y = self.to_number(&pb);
        Val::Num(x + y)
    }

    /// `<`: 1 true, 0 false, -1 undefined (NaN)
    pub fn less_than(&mut self, a: &Val, b: &Val) -> int {
        if let (Val::Num(x), Val::Num(y)) = (a, b) {
            if is_nan(*x) || is_nan(*y) {
                return -1;
            }
            return if x < y { 1 } else { 0 };
        }
        let pa = self.to_primitive(a, "number");
        if self.throwing {
            return -1;
        }
        let pb = self.to_primitive(b, "number");
        if self.throwing {
            return -1;
        }
        if let (Val::Str(x), Val::Str(y)) = (&pa, &pb) {
            return if jsstr::compare(self.sref(*x), self.sref(*y)) < 0 { 1 } else { 0 };
        }
        if self.class_of(&pa) == C_BIGINT || self.class_of(&pb) == C_BIGINT {
            return self.big_less(&pa, &pb);
        }
        let x = self.to_number(&pa);
        let y = self.to_number(&pb);
        if is_nan(x) || is_nan(y) {
            return -1;
        }
        if x < y {
            1
        } else {
            0
        }
    }

    pub fn compare(&mut self, code: int, a: &Val, b: &Val) -> bool {
        if let (Val::Num(x), Val::Num(y)) = (a, b) {
            return match code {
                OP_LT => x < y,
                OP_GT => x > y,
                OP_LE => x <= y,
                OP_GE => x >= y,
                OP_SEQ | OP_EQ => x == y,
                _ => x != y,
            };
        }
        match code {
            OP_LT => self.less_than(a, b) == 1,
            OP_GT => self.less_than(b, a) == 1,
            OP_LE => self.less_than(b, a) == 0,
            OP_GE => self.less_than(a, b) == 0,
            OP_SEQ => self.strict_equals(a, b),
            OP_SNE => !self.strict_equals(a, b),
            OP_EQ => self.loose_equals(a, b),
            _ => !self.loose_equals(a, b),
        }
    }

    pub fn arith(&mut self, code: int, a: &Val, b: &Val) -> Val {
        if is_obj(a) || is_obj(b) {
            if self.class_of(a) == C_BIGINT && self.class_of(b) == C_BIGINT {
                return self.big_arith(code, a, b);
            }
            let pa = self.to_primitive(a, "number");
            if self.throwing {
                return Val::Undef;
            }
            let pb = self.to_primitive(b, "number");
            if self.throwing {
                return Val::Undef;
            }
            if self.class_of(&pa) == C_BIGINT || self.class_of(&pb) == C_BIGINT {
                return self.big_arith(code, &pa, &pb);
            }
            let x = self.to_number(&pa);
            let y = self.to_number(&pb);
            if self.throwing {
                return Val::Undef;
            }
            return Val::Num(arith_num(code, x, y));
        }
        if matches!(a, Val::Big(_)) || matches!(b, Val::Big(_)) {
            return self.big_arith(code, a, b);
        }
        let x = match a {
            Val::Num(n) => *n,
            _ => self.to_number(a),
        };
        if self.throwing {
            return Val::Undef;
        }
        let y = match b {
            Val::Num(n) => *n,
            _ => self.to_number(b),
        };
        if self.throwing {
            return Val::Undef;
        }
        Val::Num(arith_num(code, x, y))
    }

    // ---- calls

    /// Calls `f` from native code (or anywhere outside the interpreter
    /// loop) and answers its result; an exception is left in `throwing`.
    /// `f(a, b)` with `this` undefined, for a comparator: a script function
    /// is entered straight from the stack, with no argument vector.
    pub fn call2(&mut self, f: &Val, a: &Val, b: &Val) -> Val {
        let fo = obj_of(f);
        if fo >= 0 && self.objs[fo as usize].class == C_FUNCTION {
            let base = self.frames.len() as int;
            self.stack.push(f.clone());
            self.stack.push(Val::Undef);
            self.stack.push(a.clone());
            self.stack.push(b.clone());
            if !self.enter_function(fo, 2, false, Val::Undef) {
                return Val::Undef;
            }
            self.native_depth += 1;
            self.run(base);
            self.native_depth -= 1;
            if self.throwing {
                return Val::Undef;
            }
            return self.stack.pop().unwrap();
        }
        self.call_fixed(f.clone(), Val::Undef, 2, a.clone(), b.clone(), Val::Undef)
    }

    pub fn call_value(&mut self, f: Val, this: Val, args: Vec<Val>) -> Val {
        let fo = obj_of(&f);
        if fo < 0 || !self.is_callable(&f) {
            self.throw_type("value is not a function");
            return Val::Undef;
        }
        let class = self.objs[fo as usize].class;
        if class == C_NATIVE {
            let id = self.objs[fo as usize].func as int;
            return self.call_native(id, fo, this, &args, false, Val::Undef);
        }
        if class == C_BOUND {
            let target = self.objs[fo as usize].env as int;
            let bthis = self.colds[self.objs[fo as usize].cold as usize].prim.clone();
            let mut all = self.evecs[self.objs[fo as usize].elems as usize].v.clone();
            for a in args {
                all.push(a);
            }
            return self.call_value(Val::Obj(target), bthis, all);
        }
        if class == C_PROXY {
            let arr = self.new_array(args);
            return self.proxy_call(fo, PH_APPLY, vec![this, Val::Obj(arr)]);
        }
        let base = self.frames.len() as int;
        let argc = args.len() as int;
        self.stack.push(f);
        self.stack.push(this);
        for a in args {
            self.stack.push(a);
        }
        if !self.enter_function(fo, argc, false, Val::Undef) {
            return Val::Undef;
        }
        self.native_depth += 1;
        self.run(base);
        self.native_depth -= 1;
        if self.throwing || self.suspended {
            return Val::Undef;
        }
        self.stack.pop().unwrap()
    }

    /// call_value with up to three arguments (n of a, b, c) and no list
    /// made for them when f is a compiled function.
    pub fn call_fixed(&mut self, f: Val, this: Val, n: int, a: Val, b: Val, c: Val) -> Val {
        let fo = obj_of(&f);
        if fo < 0 || self.objs[fo as usize].class != C_FUNCTION {
            let mut args: Vec<Val> = Vec::new();
            if n > 0 {
                args.push(a);
            }
            if n > 1 {
                args.push(b);
            }
            if n > 2 {
                args.push(c);
            }
            return self.call_value(f, this, args);
        }
        let base = self.frames.len() as int;
        self.stack.push(f);
        self.stack.push(this);
        if n > 0 {
            self.stack.push(a);
        }
        if n > 1 {
            self.stack.push(b);
        }
        if n > 2 {
            self.stack.push(c);
        }
        if !self.enter_function(fo, n, false, Val::Undef) {
            return Val::Undef;
        }
        self.native_depth += 1;
        self.run(base);
        self.native_depth -= 1;
        if self.throwing || self.suspended {
            return Val::Undef;
        }
        self.stack.pop().unwrap()
    }

    /// Runs a script's entry function. Unlike `call_value` from a native,
    /// the script's own frames are safe points for the collector.
    pub fn run_program(&mut self, f: Val, this: Val) -> Val {
        self.native_depth -= 1;
        let r = self.call_fixed(f, this, 0, Val::Undef, Val::Undef, Val::Undef);
        self.native_depth += 1;
        r
    }

    /// A call from the host, with no native code under it (native_depth
    /// 0): the frames it runs are safe points, so what it leaves behind is
    /// reclaimed while it runs. Inside a native call it is call_value.
    pub fn call_top(&mut self, f: Val, this: Val, args: Vec<Val>) -> Val {
        if self.native_depth != 0 {
            return self.call_value(f, this, args);
        }
        self.native_depth -= 1;
        let r = self.call_value(f, this, args);
        self.native_depth += 1;
        r
    }

    /// IsConstructor(v): a function `new` may call.
    pub fn is_constructor(&mut self, v: &Val) -> bool {
        let mut o = obj_of(v);
        let mut hops = 0;
        while o >= 0 && hops < 10000 {
            let class = self.objs[o as usize].class;
            if class == C_FUNCTION {
                let pi = self.objs[o as usize].func as usize;
                return self.protos[pi].constructible();
            }
            if class == C_NATIVE {
                // the built-in constructors are the natives with a prototype
                return self.objs[o as usize].func as int == NF_PROXY || self.find(o, A_PROTOTYPE) >= 0;
            }
            if class == C_BOUND {
                o = self.objs[o as usize].env as int;
            } else if class == C_PROXY {
                if self.objs[o as usize].pos != 1 {
                    return false;
                }
                o = self.objs[o as usize].env as int;
            } else {
                return false;
            }
            hops += 1;
        }
        false
    }

    /// `new f(...args)` from native code.
    pub fn construct(&mut self, f: Val, args: Vec<Val>, new_target: Val) -> Val {
        let fo = obj_of(&f);
        if fo < 0 || !self.is_callable(&f) {
            self.throw_type("value is not a constructor");
            return Val::Undef;
        }
        let class = self.objs[fo as usize].class;
        if class == C_NATIVE && !self.is_constructor(&f) {
            self.throw_type("value is not a constructor");
            return Val::Undef;
        }
        if class == C_NATIVE {
            let id = self.objs[fo as usize].func as int;
            let this = Val::Undef;
            return self.call_native(id, fo, this, &args, true, new_target);
        }
        if class == C_BOUND {
            let target = self.objs[fo as usize].env as int;
            let mut all = self.evecs[self.objs[fo as usize].elems as usize].v.clone();
            for a in args {
                all.push(a);
            }
            let nt = if self.strict_equals(&new_target, &f) { Val::Obj(target) } else { new_target };
            return self.construct(Val::Obj(target), all, nt);
        }
        if class == C_PROXY {
            let arr = self.new_array(args);
            return self.proxy_call(fo, PH_CONSTRUCT, vec![Val::Obj(arr), new_target]);
        }
        let pi = self.objs[fo as usize].func as int;
        if !self.protos[pi as usize].constructible() {
            self.throw_type("value is not a constructor");
            return Val::Undef;
        }
        let this = self.make_this(&new_target);
        let base = self.frames.len() as int;
        let argc = args.len() as int;
        self.stack.push(f);
        self.stack.push(this.clone());
        for a in args {
            self.stack.push(a);
        }
        if !self.enter_function(fo, argc, true, new_target) {
            return Val::Undef;
        }
        self.native_depth += 1;
        self.run(base);
        self.native_depth -= 1;
        if self.throwing {
            return Val::Undef;
        }
        self.stack.pop().unwrap()
    }

    /// A new object whose prototype is `new_target.prototype`.
    pub fn make_this(&mut self, new_target: &Val) -> Val {
        let nt = obj_of(new_target);
        let mut proto = self.object_proto;
        if nt >= 0 {
            let p = self.get_obj(nt, A_PROTOTYPE, new_target);
            if let Val::Obj(po) = p {
                proto = po;
            }
        }
        let o = self.alloc(C_OBJECT, proto);
        Val::Obj(o)
    }

    /// The constructor `new f(…)` enters: f, or, when f is a class with the
    /// default derived constructor (Proto.fwd_atom -2) and no fields, its
    /// parent class, which that constructor would call with the same
    /// arguments, `this` and new.target (and so on up). The parent is read
    /// as super(…) reads it: f's prototype now.
    #[inline(never)]
    pub fn ctor_target(&self, fo: int) -> int {
        let mut c = fo;
        loop {
            if self.protos[self.objs[c as usize].func as usize].fwd_atom != -2 || !self.colds[self.objs[c as usize].cold as usize].elems2.is_empty() {
                return c;
            }
            let par = self.objs[c as usize].proto as int;
            if par < 0 || self.objs[par as usize].class != C_FUNCTION || !self.protos[self.objs[par as usize].func as usize].constructible() {
                return c;
            }
            c = par;
        }
    }

    /// `new f(…)` of a forwarder f (Proto.fwd_atom: its body is just
    /// `this.m.apply(this, arguments)`) whose new object is `this`: m when
    /// that call can be m's own frame (a construct frame of m: m returns
    /// nothing, so the object is the result either way), else -1. Only
    /// data properties are looked at (no getter runs here): m on this's
    /// prototypes, and apply on m's the built-in one.
    pub fn fwd_target(&mut self, fp: usize, this: &Val) -> int {
        let atom = self.protos[fp].fwd_atom;
        let o = obj_of(this);
        if atom < 0 || o < 0 {
            return -1;
        }
        let m = self.data_obj(self.objs[o as usize].proto as int, atom);
        if m < 0 || self.objs[m as usize].class != C_FUNCTION {
            return -1;
        }
        {
            let q = &self.protos[self.objs[m as usize].func as usize];
            if !q.void_ret || q.rest >= 0 || q.class_ctor || q.generator || q.is_async || q.arrow {
                return -1;
            }
        }
        if self.a_apply < 0 {
            self.a_apply = self.intern("apply");
        }
        let ap = self.data_obj(m, self.a_apply);
        if ap < 0 || self.objs[ap as usize].class != C_NATIVE || (self.objs[ap as usize].func as int) != crate::builtins::NF_APPLY {
            return -1;
        }
        m
    }

    /// The object in data property `atom` of o or of its prototypes, -1
    /// when it is not an object, is absent, or the walk meets an accessor
    /// or an object whose properties are not only its own slots.
    fn data_obj(&self, o: int, atom: int) -> int {
        let mut cur = o;
        let mut hops = 0;
        while cur >= 0 && hops < 64 {
            let class = self.objs[cur as usize].class;
            if class != C_OBJECT && class != C_FUNCTION && class != C_NATIVE {
                return -1;
            }
            let slot = self.find(cur, atom);
            if slot >= 0 {
                if (self.attr_at(cur, slot) & (P_ACCESSOR | P_MAPPED)) != 0 {
                    return -1;
                }
                return obj_of(&self.objs[cur as usize].get_slot(slot as usize));
            }
            if class == C_FUNCTION && (atom == A_LENGTH || atom == A_NAME || atom == A_PROTOTYPE) {
                return -1;
            }
            cur = self.objs[cur as usize].proto as int;
            hops += 1;
        }
        -1
    }

    /// The arguments object of a call of `fo` whose argc arguments lie
    /// from stack slot bp on.
    pub fn make_args(&mut self, fo: int, bp: int, argc: int, strict: bool) -> int {
        let mut items: Vec<Val> = Vec::new();
        reserve_vals(&mut items, argc as usize);
        let mut i = 0;
        while i < argc {
            items.push(self.stack[(bp + i) as usize].clone());
            i += 1;
        }
        let p = self.object_proto;
        let args_obj = self.alloc(C_ARGUMENTS, p);
        self.put_elems(args_obj, items);
        let si = if strict { 1 } else { 0 };
        let known = self.args_shapes[si];
        if known >= 0 {
            // the shape the first one got: no transitions looked up
            let callee = if strict { Val::Obj(self.callee_pair) } else { Val::Obj(fo) };
            let values = self.array_values_fn;
            self.objs[args_obj as usize].shape = known as i32;
            self.push_val(args_obj, callee);
            self.push_val(args_obj, Val::Obj(values));
        } else if strict {
            // a strict function's arguments.callee throws
            // non-configurable, so the shared pair never changes
            if self.callee_pair < 0 {
                let th = self.thrower;
                let pair = self.alloc(C_ACCESSOR, -1);
                self.put_elems(pair, vec![Val::Obj(th), Val::Obj(th)]);
                self.roots.push(pair);
                self.callee_pair = pair;
                self.any_setter = true;
                self.mark_setter(A_CALLEE);
            }
            let pair = self.callee_pair;
            self.add_own(args_obj, A_CALLEE, Val::Obj(pair), P_ACCESSOR | P_HIDDEN | P_FIXED);
        } else {
            self.add_own(args_obj, A_CALLEE, Val::Obj(fo), P_HIDDEN);
        }
        if known < 0 {
            let values = self.array_values_fn;
            self.add_own(args_obj, A_ITERATOR, Val::Obj(values), P_HIDDEN);
            let sh = self.objs[args_obj as usize].shape as int;
            if !self.shapes[sh as usize].owned {
                self.args_shapes[si] = sh;
            }
        }
        args_obj
    }

    /// Turns a fast arguments object (elements in elems, length from
    /// them) into C_SARGS: each element an own index property, then an
    /// own `length`, then the properties it had (callee, iterator).
    pub fn slow_args(&mut self, o: int) {
        if self.objs[o as usize].class != C_ARGUMENTS {
            return;
        }
        self.compact_dict(o);
        let keys = self.keys_of(o);
        let mut vals: Vec<Val> = Vec::new();
        let mut attrs: Vec<int> = Vec::new();
        let mut k: usize = 0;
        while k < keys.len() {
            let v = self.objs[o as usize].get_slot(k).clone();
            self.inc(&v);
            vals.push(v);
            attrs.push(self.attr_at(o, k as int));
            k += 1;
        }
        let mut r = (keys.len() as int) - 1;
        while r >= 0 {
            self.remove_own(o, r);
            r -= 1;
        }
        let n = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
        let mut items: Vec<Val> = Vec::new();
        let mut i: int = 0;
        while i < n {
            let v = self.evecs[self.objs[o as usize].elems as usize].v[i as usize].clone();
            self.inc(&v);
            items.push(v);
            i += 1;
        }
        i = 0;
        while i < n {
            self.pop_elem(o);
            i += 1;
        }
        self.objs[o as usize].class = C_SARGS;
        i = 0;
        while i < n {
            let a = self.index_atom(i);
            let v = items[i as usize].clone();
            self.add_own(o, a, v.clone(), 0);
            self.dec(&v);
            i += 1;
        }
        self.add_own(o, A_LENGTH, Val::Num(n as double), P_HIDDEN);
        k = 0;
        while k < keys.len() {
            let v = vals[k].clone();
            self.add_own(o, keys[k], v.clone(), attrs[k]);
            self.dec(&v);
            k += 1;
        }
    }

    /// Maps arguments element j to the parameter in env slot (b >> 8) of
    /// the env at depth (b & 255) (OP_ARGUMENTS a = 2 + j).
    pub fn map_arg(&mut self, o: int, j: int, b: int) {
        self.slow_args(o);
        let a = self.index_atom(j);
        let slot = self.find(o, a);
        if slot < 0 {
            return;
        }
        let env = self.get_env(b & 255);
        if self.colds[self.objs[o as usize].cold as usize].home != env {
            self.put_home(o, env);
        }
        let c = self.cold_id(o) as usize;
        self.colds[c].marks.push(a);
        self.colds[c].marks.push(b >> 8);
        let at = self.attr_at(o, slot);
        self.set_attr(o, slot, at | P_MAPPED);
    }

    /// The env slot a mapped arguments element atom reads, or -1.
    pub fn mapped_slot(&self, o: int, atom: int) -> int {
        let m = &self.colds[self.objs[o as usize].cold as usize].marks;
        let mut k: usize = 0;
        while k + 1 < m.len() {
            if m[k] == atom {
                return m[k + 1];
            }
            k += 2;
        }
        -1
    }

    /// A mapped element's value: the parameter's.
    pub fn mapped_get(&mut self, o: int, atom: int, slot: int) -> Val {
        let e = self.mapped_slot(o, atom);
        let home = self.colds[self.objs[o as usize].cold as usize].home;
        if e < 0 || home < 0 {
            return self.objs[o as usize].get_slot(slot as usize).clone();
        }
        self.evecs[self.objs[home as usize].elems as usize].v[e as usize].clone()
    }

    /// A write to a mapped element also sets the parameter.
    pub fn mapped_put(&mut self, o: int, atom: int, v: &Val) {
        let e = self.mapped_slot(o, atom);
        let home = self.colds[self.objs[o as usize].cold as usize].home;
        if e >= 0 && home >= 0 {
            self.put_elem(home, e, v.clone());
        }
    }

    /// Copies a mapped element's parameter value into its property (before
    /// a descriptor reads or redefines it). Returns whether it is mapped.
    pub fn mapped_sync(&mut self, o: int, atom: int) -> bool {
        if self.objs[o as usize].class != C_SARGS {
            return false;
        }
        let slot = self.find(o, atom);
        if slot < 0 || (self.attr_at(o, slot) & P_MAPPED) == 0 {
            return false;
        }
        let v = self.mapped_get(o, atom, slot);
        self.put_slot(o, slot, v);
        true
    }

    /// Sets up a frame for the JS function `fo`; the stack holds
    /// [f, this, args…]. For a constructor call `this` is the new object.
    pub fn enter_function(&mut self, fo: int, argc: int, construct: bool, new_target: Val) -> bool {
        let pi = self.objs[fo as usize].func as int;
        if !construct && !self.protos[pi as usize].uses_args && self.protos[pi as usize].rest < 0 && !self.protos[pi as usize].class_ctor {
            return self.enter_plain(fo, pi, argc);
        }
        let bp = (self.stack.len() as int) - argc;
        let (nparams, nslots, env_size, rest, uses_args, arrow, strict, class_ctor, derived) = {
            let p = &self.protos[pi as usize];
            (p.nparams, p.nslots, p.env_size, p.rest, p.uses_args, p.arrow, p.strict, p.class_ctor, p.derived)
        };
        if class_ctor && !construct {
            self.stack.truncate((bp - 2) as usize);
            self.throw_type("Class constructor cannot be invoked without 'new'");
            return false;
        }
        let mut args_obj: int = -1;
        if uses_args {
            args_obj = self.make_args(fo, bp, argc, strict);
        }
        if rest >= 0 {
            let mut items: Vec<Val> = Vec::new();
            if argc > rest {
                reserve_vals(&mut items, (argc - rest) as usize);
            }
            let mut i = rest;
            while i < argc {
                items.push(self.stack[(bp + i) as usize].clone());
                i += 1;
            }
            let arr = self.new_array(items);
            self.stack.truncate((bp + rest) as usize);
            // fewer arguments than the parameters before the rest one
            while (self.stack.len() as int) < bp + rest {
                self.stack.push(Val::Undef);
            }
            self.stack.push(Val::Obj(arr));
        } else if argc > nparams {
            self.stack.truncate((bp + nparams) as usize);
        }
        while (self.stack.len() as int) < bp + nslots {
            self.stack.push(Val::Undef);
        }
        let mut this_val = self.stack[(bp - 1) as usize].clone();
        let mut nt = new_target;
        if arrow {
            this_val = self.colds[self.objs[fo as usize].cold as usize].prim.clone();
            // new.target as the arrow found it
            nt = if self.colds[self.objs[fo as usize].cold as usize].elems2.is_empty() { Val::Undef } else { self.colds[self.objs[fo as usize].cold as usize].elems2[0].clone() };
        } else if !strict && !construct {
            if matches!(this_val, Val::Undef) || matches!(this_val, Val::Null) {
                this_val = Val::Obj(self.global);
            } else if !is_obj(&this_val) {
                // a sloppy function sees a primitive `this` boxed
                let bo = self.to_object(&this_val);
                this_val = Val::Obj(bo);
            }
        }
        let mut env = self.objs[fo as usize].env as int;
        if env_size > 0 {
            let e = self.alloc(C_ENV, env);
            let mut i = 0;
            while i < env_size {
                self.push_elem(e, Val::Undef);
                i += 1;
            }
            env = e;
        }
        unpack_code(&mut self.protos[pi as usize]);
        self.frames.push(Frame {
            proto: pi,
            fobj: fo,
            pc: 0,
            bp: bp,
            env: env,
            this_val: this_val.clone(),
            new_target: nt,
            args_obj: args_obj,
            construct: construct,
            gen: -1,
            ret: -1,
            mv_from: -1,
            mv_to: -1,
        });
        if construct && class_ctor && !derived {
            self.run_fields(fo, &this_val);
            if self.throwing {
                return false;
            }
        }
        true
    }

    /// enter_function for a call (not `new`) of a function without
    /// `arguments`, a rest parameter or a class constructor's checks.
    fn enter_plain(&mut self, fo: int, pi: int, argc: int) -> bool {
        let nparams = self.protos[pi as usize].nparams;
        let nslots = self.protos[pi as usize].nslots;
        let env_size = self.protos[pi as usize].env_size;
        let bp = (self.stack.len() as int) - argc;
        if argc > nparams {
            self.stack.truncate((bp + nparams) as usize);
        }
        while (self.stack.len() as int) < bp + nslots {
            self.stack.push(Val::Undef);
        }
        let arrow = self.protos[pi as usize].arrow;
        let mut nt = Val::Undef;
        if arrow && !self.colds[self.objs[fo as usize].cold as usize].elems2.is_empty() {
            nt = self.colds[self.objs[fo as usize].cold as usize].elems2[0].clone();
        }
        let mut this_val = if arrow { self.colds[self.objs[fo as usize].cold as usize].prim.clone() } else { self.stack[(bp - 1) as usize].clone() };
        if !arrow && !self.protos[pi as usize].strict {
            if matches!(this_val, Val::Undef) || matches!(this_val, Val::Null) {
                this_val = Val::Obj(self.global);
            } else if !is_obj(&this_val) {
                let bo = self.to_object(&this_val);
                this_val = Val::Obj(bo);
            }
        }
        let mut env = self.objs[fo as usize].env as int;
        if env_size > 0 {
            let e = self.alloc(C_ENV, env);
            let mut i = 0;
            while i < env_size {
                self.push_elem(e, Val::Undef);
                i += 1;
            }
            env = e;
        }
        unpack_code(&mut self.protos[pi as usize]);
        self.frames.push(Frame {
            proto: pi,
            fobj: fo,
            pc: 0,
            bp: bp,
            env: env,
            this_val: this_val,
            new_target: nt,
            args_obj: -1,
            construct: false,
            gen: -1,
            ret: -1,
            mv_from: -1,
            mv_to: -1,
        });
        true
    }

    /// The class's instance fields, defined on `this`.
    pub fn run_fields(&mut self, ctor: int, this: &Val) {
        if self.colds[self.objs[ctor as usize].cold as usize].elems2.is_empty() {
            return;
        }
        let f = self.colds[self.objs[ctor as usize].cold as usize].elems2[0].clone();
        self.call_fixed(f, this.clone(), 0, Val::Undef, Val::Undef, Val::Undef);
    }

    /// Unwinds to the innermost handler of this run; false when there is
    /// none and the exception leaves the run.
    pub fn unwind(&mut self, base: int) -> bool {
        loop {
            if self.handlers.is_empty() {
                break;
            }
            let h = self.handlers.len() - 1;
            let hf = self.handlers[h].frame;
            if hf < base {
                break;
            }
            let (catch_pc, sp, env) = (self.handlers[h].catch_pc, self.handlers[h].sp, self.handlers[h].env);
            self.handlers.pop();
            self.frames.truncate((hf + 1) as usize);
            self.stack.truncate(sp as usize);
            let fi = self.frames.len() - 1;
            self.frames[fi].pc = catch_pc;
            self.frames[fi].env = env;
            let e = self.exc.clone();
            self.stack.push(e);
            self.exc = Val::Undef;
            self.throwing = false;
            return true;
        }
        // no handler here: drop this run's frames
        if (self.frames.len() as int) > base {
            let bp = self.frames[base as usize].bp;
            self.frames.truncate(base as usize);
            self.stack.truncate((bp - 2) as usize);
        }
        false
    }

    /// The function whose `this` / new.target frame `fi` shares: its own,
    /// or for an arrow the one it was made in.
    pub fn ctor_of_frame(&self, fi: usize) -> int {
        let fo = self.frames[fi].fobj;
        if fo >= 0 && self.objs[fo as usize].class == C_FUNCTION && self.colds[self.objs[fo as usize].cold as usize].elems2.len() == 2 {
            let pi = self.objs[fo as usize].func as int;
            if self.protos[pi as usize].arrow {
                return obj_of(&self.colds[self.objs[fo as usize].cold as usize].elems2[1]);
            }
        }
        fo
    }

    pub fn closure(&mut self, pi: int) -> int {
        let fp = self.closure_proto(pi);
        let f = self.alloc(C_FUNCTION, fp);
        let fi = self.frames.len() - 1;
        let env = self.frames[fi].env;
        self.objs[f as usize].func = pi as i32;
        self.put_env(f, env);
        if self.protos[pi as usize].arrow {
            let t = self.frames[fi].this_val.clone();
            self.put_prim(f, t);
            let home = self.colds[self.objs[self.frames[fi].fobj as usize].cold as usize].home;
            self.put_home(f, home);
            if self.protos[pi as usize].lexical_ctor {
                let nt = self.frames[fi].new_target.clone();
                let owner = self.ctor_of_frame(fi);
                self.push_elem2(f, nt);
                self.push_elem2(f, Val::Obj(owner));
            }
        }
        f
    }

    pub fn iter_values(&mut self, v: &Val) -> Val {
        // arrays, strings, and anything with a Symbol.iterator method
        if let Val::Obj(o) = v {
            let class = self.objs[*o as usize].class;
            if class == C_ARRAY || class == C_ARGUMENTS {
                let plain = class == C_ARGUMENTS || self.array_iter_plain(*o) || obj_of(&self.get_obj(*o, A_ITERATOR, v)) == self.array_values_fn;
                if plain {
                    let ip = self.iter_proto;
                    let it = self.alloc(C_ITER, ip);
                    self.put_env(it, *o);
                    self.objs[it as usize].func = 0;
                    return Val::Obj(it);
                }
            }
        }
        if let Val::Str(_) = v {
            let ip = self.iter_proto;
            return self.string_iter(v.clone(), ip);
        }
        let f = self.get(v, A_ITERATOR);
        if self.throwing {
            return Val::Undef;
        }
        if !self.is_callable(&f) {
            self.throw_type("object is not iterable");
            return Val::Undef;
        }
        let it = self.call_fixed(f, v.clone(), 0, Val::Undef, Val::Undef, Val::Undef);
        if self.throwing {
            return Val::Undef;
        }
        if !is_obj(&it) {
            self.throw_type("Result of the Symbol.iterator method is not an object");
            return Val::Undef;
        }
        it
    }

    /// An array iterator's step over an object not an array: its length
    /// read again (a typed array out of bounds throws), then element i.
    fn array_like_next(&mut self, o: int, src: int, i: int, kind: int) -> Option<Val> {
        let len = if self.objs[src as usize].class == C_TYPED {
            if self.ta_out_of_bounds(src) {
                self.throw_type("Cannot iterate a typed array out of bounds");
                return None;
            }
            self.ta_length(src)
        } else {
            let srcv = Val::Obj(src);
            let l = self.get(&srcv, A_LENGTH);
            if self.throwing {
                return None;
            }
            let n = to_integer(self.to_number(&l));
            if n > 0.0 { n as int } else { 0 }
        };
        if self.throwing {
            return None;
        }
        if i >= len {
            self.put_env(o, -1);
            return None;
        }
        self.objs[o as usize].func = (i + 1) as i32;
        if kind == 1 {
            return Some(Val::Num(i as double));
        }
        let v = self.get_index(&Val::Obj(src), i);
        if self.throwing {
            return None;
        }
        if kind == 2 {
            let pair = self.new_array(vec![Val::Num(i as double), v]);
            return Some(Val::Obj(pair));
        }
        Some(v)
    }

    /// An iterator over the code points of string `s`, one step at a time
    /// (iter_next, kind 16): its source is a one-element array holding the
    /// string, its position a jsstr::cp_at position.
    pub fn string_iter(&mut self, s: Val, proto: int) -> Val {
        let arr = self.new_array(vec![s]);
        let it = self.alloc(C_ITER, proto);
        self.put_env(it, arr);
        self.objs[it as usize].func = 0;
        self.objs[it as usize].pos = 16;
        Val::Obj(it)
    }

    fn string_iter_next(&mut self, o: int, src: int, i: int) -> Option<Val> {
        let sv = self.evecs[self.objs[src as usize].elems as usize].v[0].clone();
        let s = self.to_str(&sv);
        if i >= jsstr::cp_end(s.as_str()) {
            self.put_env(o, -1);
            return None;
        }
        let cp = jsstr::cp_at(s.as_str(), i);
        if cp < 0 {
            self.put_env(o, -1);
            return None;
        }
        self.objs[o as usize].func = (i + jsstr::cp_width(cp)) as i32;
        let mut t = String::new();
        jsstr::push_cp(&mut t, cp);
        Some(self.string_val(t))
    }

    /// The next value of an iterator made by `iter_values` / ITER_KEYS;
    /// None when done.
    pub fn iter_next(&mut self, it: &Val) -> Option<Val> {
        let o = obj_of(it);
        if o >= 0 && self.objs[o as usize].class == C_ITER {
            let src = self.objs[o as usize].env as int;
            let i = self.objs[o as usize].func as int;
            let kind = self.objs[o as usize].pos as int;
            if src < 0 {
                return None;
            }
            if kind == 16 {
                return self.string_iter_next(o, src, i);
            }
            if (kind & 3) == 3 {
                return self.for_in_next(o, src, i, kind >> 2);
            }
            if (kind & 32) != 0 {
                return self.coll_next(o, src, i, kind);
            }
            if (kind & 8) != 0 {
                return self.array_like_next(o, src, i, kind & 3);
            }
            let len = self.evecs[self.objs[src as usize].elems as usize].v.len() as int;
            if i >= len {
                self.put_env(o, -1);
                return None;
            }
            self.objs[o as usize].func = (i + 1) as i32;
            let v = if self.objs[src as usize].pos == 3 && self.is_hole(src, i) {
                self.get_index(&Val::Obj(src), i)
            } else {
                self.evecs[self.objs[src as usize].elems as usize].v[i as usize].clone()
            };
            if kind == 1 {
                return Some(Val::Num(i as double));
            }
            if kind == 2 {
                let pair = self.new_array(vec![Val::Num(i as double), v]);
                return Some(Val::Obj(pair));
            }
            return Some(v);
        }
        let plain = o >= 0 && self.objs[o as usize].class == C_GENERATOR && self.gen_plain_next(o);
        let next = if plain { Val::Undef } else { self.get(it, A_NEXT) };
        if self.throwing {
            return None;
        }
        // a generator with its own next: resumed here, no result object
        let nf = obj_of(&next);
        if plain || (o >= 0 && nf >= 0 && self.objs[nf as usize].class == C_NATIVE && (self.objs[nf as usize].func as int) == crate::coroutine::NF_GEN_NEXT && self.objs[o as usize].class == C_GENERATOR && !self.protos[self.objs[o as usize].func as usize].is_async) {
            let v = self.gen_resume(o, crate::coroutine::GM_NEXT, Val::Undef);
            if self.throwing || self.gen_done {
                return None;
            }
            if !self.gen_raw {
                return Some(v);
            }
            // yield* handed on its inner result as it is
            if !is_obj(&v) {
                self.throw_type("Iterator result is not an object");
                return None;
            }
            let done = self.get(&v, A_DONE);
            if self.throwing || self.truthy_v(&done) {
                return None;
            }
            return Some(self.get(&v, A_VALUE));
        }
        let r = self.call_fixed(next, it.clone(), 0, Val::Undef, Val::Undef, Val::Undef);
        if self.throwing {
            return None;
        }
        if !is_obj(&r) {
            self.throw_type("Iterator result is not an object");
            return None;
        }
        let done = self.get(&r, A_DONE);
        if self.truthy_v(&done) {
            return None;
        }
        Some(self.get(&r, A_VALUE))
    }

    pub fn iterable_to_vec(&mut self, v: &Val) -> Vec<Val> {
        if let Val::Obj(o) = v {
            if self.array_iter_plain(*o) {
                return self.evecs[self.objs[*o as usize].elems as usize].v.clone();
            }
        }
        let it = self.iter_values(v);
        let mut out: Vec<Val> = Vec::new();
        if self.throwing {
            return out;
        }
        self.temp_roots.push(it.clone());
        loop {
            match self.iter_next(&it) {
                Some(x) => out.push(x),
                None => break,
            }
            if self.throwing {
                break;
            }
        }
        self.temp_roots.pop();
        out
    }

    /// Whether generator `o` (not async) takes `next` from
    /// %GeneratorPrototype% as made: nothing named next on it or on its
    /// function's prototype. The lookup is kept (gn_memo) for o's shape
    /// while both prototypes' shapes and the slot's value stay.
    pub fn gen_plain_next(&mut self, o: int) -> bool {
        if self.protos[self.objs[o as usize].func as usize].is_async {
            return false;
        }
        let p1 = self.objs[o as usize].proto;
        if p1 < 0 {
            return false;
        }
        let p2 = self.objs[p1 as usize].proto;
        if p2 < 0 {
            return false;
        }
        if self.gn_memo.len() == 7 && self.gn_memo[0] == self.objs[o as usize].shape && self.gn_memo[1] == (self.shape_epoch as i32) && self.gn_memo[2] == p1 && self.gn_memo[3] == self.objs[p1 as usize].shape && self.gn_memo[4] == p2 && self.gn_memo[5] == self.objs[p2 as usize].shape {
            let f = obj_of(&self.objs[p2 as usize].get_slot(self.gn_memo[6] as usize));
            return f >= 0 && self.objs[f as usize].class == C_NATIVE && (self.objs[f as usize].func as int) == crate::coroutine::NF_GEN_NEXT;
        }
        if self.find(o, A_NEXT) >= 0 || self.find(p1 as int, A_NEXT) >= 0 {
            return false;
        }
        let slot = self.find(p2 as int, A_NEXT);
        if slot < 0 || (self.attr_at(p2 as int, slot) & P_ACCESSOR) != 0 {
            return false;
        }
        let f = obj_of(&self.objs[p2 as usize].get_slot(slot as usize));
        if f < 0 || self.objs[f as usize].class != C_NATIVE || (self.objs[f as usize].func as int) != crate::coroutine::NF_GEN_NEXT {
            return false;
        }
        while self.gn_memo.len() < 7 {
            self.gn_memo.push(-1);
        }
        self.gn_memo[0] = self.objs[o as usize].shape;
        self.gn_memo[1] = self.shape_epoch as i32;
        self.gn_memo[2] = p1;
        self.gn_memo[3] = self.objs[p1 as usize].shape;
        self.gn_memo[4] = p2;
        self.gn_memo[5] = self.objs[p2 as usize].shape;
        self.gn_memo[6] = slot as i32;
        true
    }

    /// Whether array `o` iterates as its elements: no holes seen through
    /// prototypes and its Symbol.iterator Array.prototype.values. The
    /// lookup's answer is kept (ai_memo) for o's shape while its
    /// prototype's shape and slot stay.
    pub fn array_iter_plain(&mut self, o: int) -> bool {
        if self.objs[o as usize].class != C_ARRAY || !(self.objs[o as usize].pos != 3 && !self.idx_protos || self.colds[self.objs[o as usize].cold as usize].elems2.is_empty()) {
            return false;
        }
        let p = self.objs[o as usize].proto;
        if self.ai_memo.len() == 5 && p >= 0 && self.ai_memo[0] == self.objs[o as usize].shape && self.ai_memo[1] == (self.shape_epoch as i32) && self.ai_memo[2] == p && self.ai_memo[3] == self.objs[p as usize].shape {
            let f = self.objs[p as usize].get_slot(self.ai_memo[4] as usize);
            if obj_of(&f) == self.array_values_fn {
                return true;
            }
        }
        let v = Val::Obj(o);
        let f = self.get_obj(o, A_ITERATOR, &v);
        if obj_of(&f) != self.array_values_fn {
            return false;
        }
        if p >= 0 && self.find(o, A_ITERATOR) < 0 {
            let slot = self.find(p as int, A_ITERATOR);
            if slot >= 0 && (self.attr_at(p as int, slot) & P_ACCESSOR) == 0 {
                while self.ai_memo.len() < 5 {
                    self.ai_memo.push(-1);
                }
                self.ai_memo[0] = self.objs[o as usize].shape;
                self.ai_memo[1] = self.shape_epoch as i32;
                self.ai_memo[2] = p;
                self.ai_memo[3] = self.objs[p as usize].shape;
                self.ai_memo[4] = slot as i32;
            }
        }
        true
    }

    /// The first `n` values of an iterable; an iterator not done by then is
    /// closed (array destructuring without a rest element).
    pub fn iterable_take(&mut self, v: &Val, n: int) -> Vec<Val> {
        if let Val::Obj(o) = v {
            if self.array_iter_plain(*o) {
                return self.evecs[self.objs[*o as usize].elems as usize].v.clone();
            }
        }
        let it = self.iter_values(v);
        let mut out: Vec<Val> = Vec::new();
        if self.throwing {
            return out;
        }
        self.temp_roots.push(it.clone());
        let mut done = false;
        while (out.len() as int) < n {
            match self.iter_next(&it) {
                Some(x) => out.push(x),
                None => {
                    done = true;
                    break;
                }
            }
            if self.throwing {
                break;
            }
        }
        if !done && !self.throwing {
            self.iter_close(&it);
        }
        self.temp_roots.pop();
        out
    }

    pub fn get_env(&self, depth: int) -> int {
        let fi = self.frames.len() - 1;
        let mut e = self.frames[fi].env;
        let mut d = depth;
        while d > 0 {
            e = self.objs[e as usize].proto as int;
            d -= 1;
        }
        e
    }

    pub fn pop(&mut self) -> Val {
        self.stack.pop().unwrap()
    }

    fn top(&self) -> &Val {
        &self.stack[self.stack.len() - 1]
    }

    // ---- the interpreter

    /// Runs frames until the frame count falls back to `base`.
    pub fn run(&mut self, base: int) {
        if self.regs {
            self.rrun(base);
            if self.frames.is_empty() && self.jit_on {
                crate::rvm::jit_flush(self);
            }
            return;
        }
        'frames: loop {
            if self.throwing {
                if !self.unwind(base) {
                    return;
                }
            }
            if (self.frames.len() as int) <= base {
                return;
            }
            if self.slicing && self.frame_brk() {
                return;
            }
            let fi = self.frames.len() - 1;
            let pi = self.frames[fi].proto as usize;
            let bp = self.frames[fi].bp;
            let mut pc = self.frames[fi].pc;
            loop {
                let op = self.protos[pi].code[pc as usize];
                pc += 1;
                match (op.code as int) {
                    OP_GET_LOCAL => {
                        let v = self.stack[(bp + (op.a as int)) as usize].clone();
                        self.stack.push(v);
                    }
                    OP_SET_LOCAL => {
                        let v = self.stack[self.stack.len() - 1].clone();
                        self.stack[(bp + (op.a as int)) as usize] = v;
                    }
                    OP_INT => self.stack.push(Val::Num(op.a as double)),
                    OP_CONST => {
                        let v = self.protos[pi].consts[op.a as usize].clone();
                        self.stack.push(v);
                    }
                    OP_POP => {
                        self.stack.pop();
                    }
                    OP_ADD_LOCAL_POP => {
                        self.frames[fi].pc = pc;
                        self.add_local_pop(op, bp);
                    }
                    // cheap ops, here rather than through step()
                    OP_UNDEF => self.stack.push(Val::Undef),
                    OP_NULL => self.stack.push(Val::Null),
                    OP_TRUE => self.stack.push(Val::Bool(true)),
                    OP_FALSE => self.stack.push(Val::Bool(false)),
                    OP_DUP => {
                        let v = self.stack[self.stack.len() - 1].clone();
                        self.stack.push(v);
                    }
                    OP_GET_ENV => {
                        let e = self.get_env((op.a as int));
                        let v = self.evecs[self.objs[e as usize].elems as usize].v[op.b as usize].clone();
                        self.stack.push(v);
                    }
                    OP_SET_ENV => {
                        let e = self.get_env((op.a as int));
                        let v = self.stack[self.stack.len() - 1].clone();
                        self.put_elem(e, (op.b as int), v);
                    }
                    OP_NOT => {
                        let v = self.pop();
                        self.stack.push(Val::Bool(!self.truthy_v(&v)));
                    }
                    OP_NEW_OBJECT => {
                        let o = self.new_object();
                        self.stack.push(Val::Obj(o));
                    }
                    OP_GET_GLOBAL | OP_INIT_PROP | OP_TOSTR | OP_TONUM | OP_INC | OP_DEC => {
                        if !self.quick_op(op, pi, pc) {
                            self.frames[fi].pc = pc;
                            let changed = self.step(op, pi, bp, fi, &mut pc);
                            if self.throwing {
                                if !self.unwind(base) {
                                    return;
                                }
                                continue 'frames;
                            }
                            if changed {
                                continue 'frames;
                            }
                        }
                    }
                    OP_ADD_ENV_POP => {
                        self.frames[fi].pc = pc;
                        self.add_env_pop(op);
                    }
                    OP_ADD_PROP_POP => {
                        self.frames[fi].pc = pc;
                        self.add_prop_pop(op);
                    }
                    OP_INC_LOCAL => {
                        let i = (bp + (op.a as int)) as usize;
                        if let Val::Num(n) = self.stack[i] {
                            self.stack[i] = Val::Num(n + (op.b as double));
                        } else {
                            let v = self.stack[i].clone();
                            let (_, nv) = self.numeric_step(&v, (op.b as int));
                            self.stack[i] = nv;
                        }
                    }
                    OP_CMP_JF => {
                        let b = self.pop();
                        let a = self.pop();
                        let r = if let (Val::Num(x), Val::Num(y)) = (&a, &b) {
                            match (op.b as int) {
                                OP_LT => x < y,
                                OP_GT => x > y,
                                OP_LE => x <= y,
                                OP_GE => x >= y,
                                OP_SEQ | OP_EQ => x == y,
                                _ => x != y,
                            }
                        } else {
                            let strict = (op.b as int) == OP_SEQ || (op.b as int) == OP_SNE;
                            let q = if strict || (op.b as int) == OP_EQ || (op.b as int) == OP_NE { quick_eq(&a, &b, strict) } else { -1 };
                            if q >= 0 {
                                (q == 1) == ((op.b as int) == OP_SEQ || (op.b as int) == OP_EQ)
                            } else {
                                self.frames[fi].pc = pc;
                                self.compare((op.b as int), &a, &b)
                            }
                        };
                        if !r {
                            if (op.a as int) < pc {
                                self.frames[fi].pc = (op.a as int);
                                if self.gc_due() {
                                    self.gc();
                                }
                            }
                            if self.slicing && self.brk(fi, (op.a as int)) {
                                return;
                            }
                            pc = (op.a as int);
                        }
                    }
                    OP_GET_LOCAL2 => {
                        let v = self.stack[(bp + (op.a as int)) as usize].clone();
                        let w = self.stack[(bp + (op.b as int)) as usize].clone();
                        self.stack.push(v);
                        self.stack.push(w);
                    }
                    OP_GET_ENV_UNDEF => {
                        let e = self.get_env((op.a as int));
                        let v = self.evecs[self.objs[e as usize].elems as usize].v[op.b as usize].clone();
                        self.stack.push(v);
                        self.stack.push(Val::Undef);
                    }
                    OP_CASE_INT_JT => {
                        if let Val::Num(x) = self.stack[self.stack.len() - 1] {
                            if x == (op.b as double) {
                                if self.slicing && self.brk(fi, (op.a as int)) {
                                    return;
                                }
                                pc = (op.a as int);
                            }
                        }
                    }
                    OP_LOCAL_NULL_JF => {
                        let cmp = op.b % 256;
                        let r = match self.stack[(bp + ((op.b / 256) as int)) as usize] {
                            Val::Null => (cmp as int) == OP_SEQ || (cmp as int) == OP_EQ,
                            Val::Undef => (cmp as int) == OP_SNE || (cmp as int) == OP_EQ,
                            _ => (cmp as int) == OP_SNE || (cmp as int) == OP_NE,
                        };
                        if !r {
                            if (op.a as int) < pc {
                                self.frames[fi].pc = (op.a as int);
                                if self.gc_due() {
                                    self.gc();
                                }
                            }
                            if self.slicing && self.brk(fi, (op.a as int)) {
                                return;
                            }
                            pc = (op.a as int);
                        }
                    }
                    OP_CASE_JT => {
                        let b = self.pop();
                        let n = self.stack.len() - 1;
                        let hit = if let (Val::Num(x), Val::Num(y)) = (&self.stack[n], &b) {
                            x == y
                        } else {
                            let q = quick_eq(&self.stack[n], &b, true);
                            if q >= 0 {
                                q == 1
                            } else {
                                let d = self.stack[n].clone();
                                self.compare(OP_SEQ, &d, &b)
                            }
                        };
                        if hit {
                            if self.slicing && self.brk(fi, (op.a as int)) {
                                return;
                            }
                            pc = (op.a as int);
                        }
                    }
                    OP_JUMP => {
                        if (op.a as int) < pc {
                            if self.gc_due() {
                                self.frames[fi].pc = (op.a as int);
                                self.gc();
                            }
                        }
                        if self.slicing && self.brk(fi, (op.a as int)) {
                            return;
                        }
                        pc = (op.a as int);
                    }
                    OP_JF => {
                        let v = self.pop();
                        if !self.truthy_v(&v) {
                            if self.slicing && self.brk(fi, (op.a as int)) {
                                return;
                            }
                            pc = (op.a as int);
                        }
                    }
                    OP_JT => {
                        let v = self.pop();
                        if self.truthy_v(&v) {
                            if self.slicing && self.brk(fi, (op.a as int)) {
                                return;
                            }
                            pc = (op.a as int);
                        }
                    }
                    OP_ADD => {
                        let b = self.pop();
                        let a = self.pop();
                        if let (Val::Num(x), Val::Num(y)) = (&a, &b) {
                            self.stack.push(Val::Num(x + y));
                        } else {
                            self.frames[fi].pc = pc;
                            let r = self.add_vals(&a, &b);
                            self.stack.push(r);
                        }
                    }
                    OP_DIV | OP_MOD | OP_EXP | OP_BAND | OP_BOR | OP_BXOR | OP_SHL | OP_SHR | OP_USHR => {
                        let b = self.pop();
                        let a = self.pop();
                        if let (Val::Num(x), Val::Num(y)) = (&a, &b) {
                            self.stack.push(Val::Num(arith_num((op.code as int), *x, *y)));
                        } else {
                            self.frames[fi].pc = pc;
                            let r = self.arith((op.code as int), &a, &b);
                            self.stack.push(r);
                        }
                    }
                    OP_LT | OP_GT | OP_LE | OP_GE | OP_EQ | OP_NE | OP_SEQ | OP_SNE => {
                        let b = self.pop();
                        let a = self.pop();
                        let strict = (op.code as int) == OP_SEQ || (op.code as int) == OP_SNE;
                        let q = if strict || (op.code as int) == OP_EQ || (op.code as int) == OP_NE { quick_eq(&a, &b, strict) } else { -1 };
                        if q >= 0 {
                            self.stack.push(Val::Bool((q == 1) == ((op.code as int) == OP_SEQ || (op.code as int) == OP_EQ)));
                            continue;
                        }
                        self.frames[fi].pc = pc;
                        let r = self.compare((op.code as int), &a, &b);
                        self.stack.push(Val::Bool(r));
                    }
                    OP_THIS => {
                        let t = self.frames[fi].this_val.clone();
                        self.stack.push(t);
                    }
                    OP_SET_LOCAL_POP => {
                        let v = self.pop();
                        self.stack[(bp + (op.a as int)) as usize] = v;
                        pc += 1;
                    }
                    OP_POSTINC_LOCAL | OP_PREINC_LOCAL => {
                        let i = (bp + (op.a as int)) as usize;
                        if let Val::Num(old) = self.stack[i] {
                            let nv = old + (op.b as double);
                            self.stack[i] = Val::Num(nv);
                            self.stack.push(Val::Num(if (op.code as int) == OP_PREINC_LOCAL { nv } else { old }));
                        } else {
                            self.frames[fi].pc = pc;
                            let v = self.stack[i].clone();
                            let (old, nv) = self.numeric_step(&v, (op.b as int));
                            self.stack[i] = nv.clone();
                            self.stack.push(if (op.code as int) == OP_PREINC_LOCAL { nv } else { old });
                        }
                    }
                    OP_SUB => {
                        let b = self.pop();
                        let a = self.pop();
                        if let (Val::Num(x), Val::Num(y)) = (&a, &b) {
                            self.stack.push(Val::Num(x - y));
                        } else {
                            self.frames[fi].pc = pc;
                            let r = self.arith((op.code as int), &a, &b);
                            self.stack.push(r);
                        }
                    }
                    OP_MUL => {
                        let b = self.pop();
                        let a = self.pop();
                        if let (Val::Num(x), Val::Num(y)) = (&a, &b) {
                            self.stack.push(Val::Num(x * y));
                        } else {
                            self.frames[fi].pc = pc;
                            let r = self.arith((op.code as int), &a, &b);
                            self.stack.push(r);
                        }
                    }
                    OP_GET_THIS_PROP => {
                        let v = self.frames[fi].this_val.clone();
                        let atom = op.a;
                        let mut done = false;
                        if let Val::Obj(o) = &v {
                            let o = *o;
                            let slot = self.ic_get(o, (atom as int), (op.c as int), pi, pc);
                            if slot >= 0 {
                                let r = self.objs[o as usize].get_slot(slot as usize).clone();
                                self.stack.push(r);
                                done = true;
                            } else if (atom as int) == A_LENGTH && self.objs[o as usize].class == C_ARRAY && self.find(o, (atom as int)) < 0 {
                                // an array's length is its element count
                                let r = Val::Num(self.evecs[self.objs[o as usize].elems as usize].v.len() as double);
                                self.stack.push(r);
                                done = true;
                            }
                        }
                        if !done {
                            self.frames[fi].pc = pc;
                            let r = self.get(&v, (atom as int));
                            self.stack.push(r);
                        }
                    }
                    OP_GET_LOCAL_PROP => {
                        let v = self.stack[(bp + (op.a as int)) as usize].clone();
                        let atom = op.b;
                        let mut done = false;
                        if let Val::Obj(o) = &v {
                            let o = *o;
                            let slot = self.ic_get(o, (atom as int), (op.c as int), pi, pc);
                            if slot >= 0 {
                                let r = self.objs[o as usize].get_slot(slot as usize).clone();
                                self.stack.push(r);
                                done = true;
                            } else if (atom as int) == A_LENGTH && self.objs[o as usize].class == C_ARRAY && self.find(o, (atom as int)) < 0 {
                                // an array's length is its element count
                                let r = Val::Num(self.evecs[self.objs[o as usize].elems as usize].v.len() as double);
                                self.stack.push(r);
                                done = true;
                            }
                        }
                        if !done {
                            self.frames[fi].pc = pc;
                            let r = self.get(&v, (atom as int));
                            self.stack.push(r);
                        }
                    }
                    OP_GET_PROP => {
                        let v = self.pop();
                        let atom = op.a;
                        let mut done = false;
                        if let Val::Obj(o) = &v {
                            let o = *o;
                            let slot = self.ic_get(o, (atom as int), (op.c as int), pi, pc);
                            if slot >= 0 {
                                let r = self.objs[o as usize].get_slot(slot as usize).clone();
                                self.stack.push(r);
                                done = true;
                            } else if (atom as int) == A_LENGTH && self.objs[o as usize].class == C_ARRAY && self.find(o, (atom as int)) < 0 {
                                // an array's length is its element count
                                let r = Val::Num(self.evecs[self.objs[o as usize].elems as usize].v.len() as double);
                                self.stack.push(r);
                                done = true;
                            }
                        }
                        if !done {
                            self.frames[fi].pc = pc;
                            let r = self.get(&v, (atom as int));
                            self.stack.push(r);
                        }
                    }
                    OP_SET_PROP | OP_SET_PROP_POP => {
                        let val = self.pop();
                        let target = self.pop();
                        let atom = op.a;
                        let mut done = false;
                        if let Val::Obj(o) = &target {
                            let o = *o;
                            let slot = self.ic_set(o, (atom as int), (op.c as int), pi, pc);
                            if slot >= 0 {
                                self.put_slot(o, slot, val.clone());
                                done = true;
                            }
                        }
                        if !done {
                            self.frames[fi].pc = pc;
                            self.set(&target, (atom as int), val.clone());
                        }
                        if (op.code as int) == OP_SET_PROP_POP {
                            pc += 1;
                        } else {
                            self.stack.push(val);
                        }
                    }
                    OP_GET_ELEM => {
                        let k = self.pop();
                        let v = self.pop();
                        let mut done = false;
                        if let (Val::Obj(o), Val::Num(n)) = (&v, &k) {
                            let ob = &self.objs[*o as usize];
                            if ob.class == C_ARRAY {
                                let i = *n as int;
                                if i >= 0 && (i as usize) < self.evecs[ob.elems as usize].v.len() && (i as double) == *n && ob.pos != 3 && !self.idx_protos {
                                    let r = self.evecs[ob.elems as usize].v[i as usize].clone();
                                    self.stack.push(r);
                                    done = true;
                                }
                            }
                        }
                        if !done {
                            self.frames[fi].pc = pc;
                            let r = self.get_elem(&v, &k);
                            self.stack.push(r);
                        }
                    }
                    OP_SET_ELEM | OP_SET_ELEM_POP => {
                        let val = self.pop();
                        let k = self.pop();
                        let target = self.pop();
                        let mut done = false;
                        if let (Val::Obj(o), Val::Num(n)) = (&target, &k) {
                            self.inc(&val);
                            let ob = &mut self.objs[*o as usize];
                            let mut old: int = -1;
                            let mut old_s: int = 0;
                            if ob.class == C_ARRAY {
                                let i = *n as int;
                                if i >= 0 && (i as usize) < self.evecs[ob.elems as usize].v.len() && (i as double) == *n && ob.pos < 2 {
                                    old = obj_of(&self.evecs[ob.elems as usize].v[i as usize]);
                                    old_s = str_of(&self.evecs[ob.elems as usize].v[i as usize]);
                                    elem_put(&mut self.evecs[ob.elems as usize], i as usize, val.clone());
                                    let oc = ob.cold as usize;
                                    if oc > 0 && (i as usize) < self.colds[oc].elems2.len() {
                                        // the element is no longer a hole
                                        self.colds[oc].elems2[i as usize] = Val::Bool(false);
                                    }
                                    done = true;
                                } else if i >= 0 && (i as usize) == self.evecs[ob.elems as usize].v.len() && (i as double) == *n && ob.extensible && ob.pos != 3 {
                                    if ob.elems == 0 {
                                        if !self.efree.is_empty() {
                                            ob.elems = self.efree.pop().unwrap();
                                        } else {
                                            grow_evecs(&mut self.evecs);
                                            self.evecs.push(elems_of(Vec::new()));
                                            ob.elems = (self.evecs.len() - 1) as i32;
                                        }
                                    }
                                    self.evecs[ob.elems as usize].v.push(val.clone());
                                    done = true;
                                }
                            }
                            if done {
                                self.dec_o(old);
                                self.sdec(old_s);
                            } else {
                                self.dec(&val);
                            }
                        }
                        if !done {
                            self.frames[fi].pc = pc;
                            self.set_elem(&target, &k, val.clone());
                        }
                        if (op.code as int) == OP_SET_ELEM_POP {
                            pc += 1;
                        } else {
                            self.stack.push(val);
                        }
                    }
                    OP_GET_METHOD => {
                        let v = self.pop();
                        self.frames[fi].pc = pc;
                        let atom = op.a;
                        let mut f = Val::Undef;
                        let mut done = false;
                        if let Val::Obj(o) = &v {
                            // own slot, else the prototype's slot, cached
                            let o = *o;
                            let slot = self.find(o, (atom as int));
                            if slot >= 0 && (self.attr_at(o, slot) & P_ACCESSOR) == 0 {
                                f = self.objs[o as usize].get_slot(slot as usize).clone();
                                done = true;
                            }
                        }
                        if !done {
                            f = self.get(&v, (atom as int));
                        }
                        if !self.throwing && op.b == 0 && !self.is_callable(&f) {
                            let name = self.atom_str((atom as int));
                            self.throw_type(format!("{} is not a function", name).as_str());
                        }
                        self.stack.push(f);
                        self.stack.push(v);
                    }
                    OP_GET_METHOD_ELEM => {
                        let k = self.pop();
                        let v = self.pop();
                        self.frames[fi].pc = pc;
                        let f = self.get_elem(&v, &k);
                        self.stack.push(f);
                        self.stack.push(v);
                    }
                    OP_CALL => {
                        self.frames[fi].pc = pc;
                        let argc = op.a;
                        if self.gc_due() {
                            self.gc();
                        }
                        // a function of this program: straight in
                        let fo = obj_of(&self.stack[self.stack.len() - (argc as usize) - 2]);
                        if fo >= 0 && self.objs[fo as usize].class == C_FUNCTION {
                            self.enter_function(fo, (argc as int), false, Val::Undef);
                            continue 'frames;
                        }
                        if self.call_op((argc as int)) {
                            continue 'frames;
                        }
                    }
                    OP_RETURN | OP_RETURN_UNDEF => {
                        let mut v = if (op.code as int) == OP_RETURN { self.pop() } else { Val::Undef };
                        let fr = self.frames.pop().unwrap();
                        if fr.construct && !is_obj(&v) {
                            if !matches!(v, Val::Undef) && self.protos[fr.proto as usize].derived {
                                self.frames.push(fr);
                                self.throw_type("Derived constructors may only return object or undefined");
                                if !self.unwind(base) {
                                    return;
                                }
                                continue 'frames;
                            }
                            v = fr.this_val.clone();
                        }
                        // pop handlers the frame left (a return inside try)
                        let depth = self.frames.len() as int;
                        while !self.handlers.is_empty() && self.handlers[self.handlers.len() - 1].frame >= depth {
                            self.handlers.pop();
                        }
                        self.stack.truncate((fr.bp - 2) as usize);
                        self.stack.push(v);
                        continue 'frames;
                    }
                    _ => {
                        self.frames[fi].pc = pc;
                        let pc0 = pc;
                        let changed = self.step(op, pi, bp, fi, &mut pc);
                        if self.throwing {
                            if !self.unwind(base) {
                                return;
                            }
                            continue 'frames;
                        }
                        if changed {
                            continue 'frames;
                        }
                        // a branch in step (JF_KEEP, JNULLISH, ITER_NEXT...)
                        if pc != pc0 && self.slicing && self.brk(fi, pc) {
                            return;
                        }
                    }
                }
                if self.throwing {
                    self.frames[fi].pc = pc;
                    if !self.unwind(base) {
                        return;
                    }
                    continue 'frames;
                }
            }
        }
    }

    /// An anonymous function stored under a computed key takes the key as
    /// its name (a symbol's as "[description]").
    pub fn name_by_key(&mut self, f: &Val, k: &Val) {
        self.name_by_key_pre(f, k, "");
    }

    /// The function named after the key, with `pre` ("get " / "set ") first.
    pub fn name_by_key_pre(&mut self, f: &Val, k: &Val, pre: &str) {
        let fo = obj_of(f);
        if fo < 0 || self.objs[fo as usize].class != C_FUNCTION || self.find(fo, A_NAME) >= 0 {
            return;
        }
        let pi = self.objs[fo as usize].func as int;
        if !self.protos[pi as usize].name.is_empty() || self.protos[pi as usize].class_ctor {
            return;
        }
        let name = if self.class_of(k) == C_SYMBOL {
            let d = self.colds[self.objs[obj_of(k) as usize].cold as usize].prim.clone();
            if matches!(d, Val::Undef) {
                String::new()
            } else {
                format!("[{}]", self.to_string(&d))
            }
        } else {
            self.to_string(k)
        };
        let full = if pre.is_empty() { name } else { format!("{}{}", pre, name) };
        let sv17 = self.string_val(full);
        self.add_prop(fo, A_NAME, sv17, P_HIDDEN | P_READONLY);
    }

    /// ToNumeric(v) and it plus `delta`: numbers or BigInts.
    pub fn numeric_step(&mut self, v: &Val, delta: int) -> (Val, Val) {
        let p = if is_obj(v) { self.to_primitive(v, "number") } else { v.clone() };
        if let Val::Big(n) = p {
            let b = self.big_step(n, delta as double);
            return (p, b);
        }
        if self.class_of(&p) == C_BIGINT {
            let t = self.big_text(&p);
            let r = crate::bigint::add(t.as_str(), format!("{}", delta).as_str());
            let b = self.bigint_val(r.as_str());
            return (p, b);
        }
        let n = self.to_number(&p);
        (Val::Num(n), Val::Num(n + (delta as double)))
    }

    /// An array's missing element. An array keeps holes (elisions,
    /// writes past the end, deleted elements, a longer length) as markers in
    /// `elems2` beside `undefined` in `elems`; an array without holes has
    /// none.
    pub fn is_hole(&self, o: int, i: int) -> bool {
        let ob = &self.objs[o as usize];
        let e2 = &self.colds[ob.cold as usize].elems2;
        if ob.class != C_ARRAY || i < 0 || (i as usize) >= e2.len() {
            return false;
        }
        // a binding, not matches!(.., Val::Bool(true)): rgrc's JS matches the
        // variant only and would count every marker as a hole
        match &e2[i as usize] {
            Val::Bool(b) => *b,
            _ => false,
        }
    }

    pub fn mark_hole(&mut self, o: int, i: int) {
        let n = self.evecs[self.objs[o as usize].elems as usize].v.len();
        while self.colds[self.objs[o as usize].cold as usize].elems2.len() < n {
            self.push_elem2(o, Val::Bool(false));
        }
        if (i as usize) < n {
            self.put_elem2(o, i, Val::Bool(true));
        }
    }

    /// Turns array element i into an own property (index atom) so it can
    /// carry attributes or be an accessor: the element becomes a hole, the
    /// array is extended to cover i, and pos 3 marks the array so element
    /// reads and writes look the property up. Returns the element's value
    /// and whether it was present.
    pub fn elem_to_prop(&mut self, o: int, i: int) -> (Val, bool) {
        let len = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
        let mut v = Val::Undef;
        let mut present = false;
        if i < len {
            if !self.is_hole(o, i) {
                v = self.evecs[self.objs[o as usize].elems as usize].v[i as usize].clone();
                present = true;
                self.put_elem(o, i, Val::Undef);
            }
        } else {
            let mut k = len;
            while k <= i {
                self.push_elem(o, Val::Undef);
                k += 1;
            }
            let mut h = len;
            while h < i {
                self.mark_hole(o, h);
                h += 1;
            }
        }
        self.mark_hole(o, i);
        self.objs[o as usize].pos = 3;
        (v, present)
    }

    /// Whether an array has an element (not a hole) in its elems.
    pub fn has_plain_elem(&self, o: int) -> bool {
        let n = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
        let mut i: int = 0;
        while i < n {
            if !self.is_hole(o, i) {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn clear_elem_hole(&mut self, o: int, i: int) {
        self.clear_hole(o, i);
    }

    fn clear_hole(&mut self, o: int, i: int) {
        if (i as usize) < self.colds[self.objs[o as usize].cold as usize].elems2.len() {
            self.put_elem2(o, i, Val::Bool(false));
        }
    }

    /// HasProperty(o, i) for an index, holes counted as missing.
    pub fn has_index(&mut self, o: int, i: int) -> bool {
        let class = self.objs[o as usize].class;
        if class == C_ARRAY && (i as usize) < self.evecs[self.objs[o as usize].elems as usize].v.len() && !self.is_hole(o, i) {
            return true;
        }
        let k = Val::Num(i as double);
        self.has_property(o, &k)
    }

    pub fn freeze_obj(&mut self, o: int) {
        self.call_native(crate::builtins::NF_O_FREEZE, -1, Val::Undef, &vec![Val::Obj(o)], false, Val::Undef);
    }

    /// At a branch, call or return with slices on: whether this slice is
    /// over and the script can stop here, with no native code under it
    /// (native_depth 0). The clock is read once in SLICE_TICKS branches.
    pub fn slice_over(&mut self) -> bool {
        if self.prof_on {
            self.prof_tick();
            return false;
        }
        self.ticks -= 1;
        if self.ticks > 0 || self.native_depth != 0 {
            return false;
        }
        self.ticks = SLICE_TICKS;
        let now = crate::builtins::now_ms();
        now - self.slice_t0 >= self.slice_ms
    }

    /// The profiler's tick (prof_on): the time since the last tick to the
    /// running function and to each function on the stack once.
    #[inline(never)]
    pub fn prof_tick(&mut self) {
        let now = crate::builtins::now_ms();
        let dt = now - self.prof_t;
        self.prof_t = now;
        if self.frames.is_empty() || dt <= 0.0 || dt > 1000.0 {
            return;
        }
        let n = self.protos.len();
        while self.prof_self.len() < n {
            self.prof_self.push(0.0);
            self.prof_incl.push(0.0);
            self.prof_mark.push(0);
        }
        self.prof_n += 1;
        let top = self.frames[self.frames.len() - 1].proto;
        if top >= 0 {
            self.prof_self[top as usize] += dt;
        }
        let mut k = self.frames.len();
        while k > 0 {
            k -= 1;
            let p = self.frames[k].proto;
            if p >= 0 && self.prof_mark[p as usize] != self.prof_n {
                self.prof_mark[p as usize] = self.prof_n;
                self.prof_incl[p as usize] += dt;
            }
        }
    }

    /// A taken branch to `to` with slices on: when the slice is over the
    /// frame stops there (its pc is `to`) and the run returns.
    pub fn brk(&mut self, fi: usize, to: int) -> bool {
        if !self.slice_over() {
            return false;
        }
        self.frames[fi].pc = to;
        self.suspended = true;
        true
    }

    /// Between frames (a call, a return, a catch) with slices on: when the
    /// slice is over the run stops; every frame's pc is already stored.
    pub fn frame_brk(&mut self) -> bool {
        if !self.slice_over() {
            return false;
        }
        self.suspended = true;
        true
    }

    pub fn gc_due(&self) -> bool {
        self.gc_flag && self.native_depth == 0
    }

    /// gc_flag from the counts (Vm::gc_due): after a collection, and where
    /// a limit changes.
    pub fn gc_recheck(&mut self) {
        self.gc_flag = self.alloc_count >= self.gc_threshold || self.made >= self.made_limit || (self.zct.len() as int) >= self.zct_limit || (self.szct.len() as int) >= self.szct_limit || self.snew >= self.slimit || self.sbytes >= self.sbytes_limit;
    }

    /// The hottest natives (Math of numbers, String of a string or number,
    /// hasOwnProperty, push, Map/Set get/has/set/add) straight off the
    /// stack: [f, this, args...] at fpos, no argument vector, same checks
    /// as their builtins. True when it ran: the result is in slot fpos (or
    /// it threw); the stack keeps its length.
    pub fn native_fast(&mut self, id: int, fpos: int, argc: int) -> bool {
        if !native_fast_id(id) {
            return false;
        }
        let bp = fpos + 2;
        // a prelude method with a native body for its plain cases
        if id == crate::typed::NF_FAST_WRAP {
            let f = obj_of(&self.stack[fpos as usize]);
            return f >= 0 && self.fast_wrap(f, fpos, argc);
        }
        // DataView get<Kind> / set<Kind> in bounds
        if id == crate::typed::NF_DVM_GET || id == crate::typed::NF_DVM_SET {
            let f = obj_of(&self.stack[fpos as usize]);
            return f >= 0 && self.dvm_fast(id, f, fpos, argc);
        }
        // Math's functions of numbers, straight off the stack
        let which = id - crate::builtins::NF_MATH;
        if which >= 0 && which <= 30 && argc >= 1 {
            let x = match &self.stack[bp as usize] {
                Val::Num(n) => *n,
                _ => nan(),
            };
            let y = if which >= 28 && argc >= 2 {
                match &self.stack[(bp + 1) as usize] {
                    Val::Num(n) => *n,
                    _ => nan(),
                }
            } else {
                0.0
            };
            let numbers = matches!(self.stack[bp as usize], Val::Num(_)) && (which < 28 || (argc >= 2 && matches!(self.stack[(bp + 1) as usize], Val::Num(_))));
            if numbers {
                let r = match which {
                    28 => arith_num(crate::ops::OP_EXP, x, y),
                    29 => x.atan2(y),
                    30 => imul(to_int32(x), to_int32(y)) as double,
                    _ => crate::builtins::math1(which, x),
                };
                self.stack[fpos as usize] = Val::Num(r);
                return true;
            }
        }
        // Number.isInteger / isSafeInteger / isFinite / isNaN: no coercion,
        // anything but a number is false
        if id >= crate::builtins::NF_N_ISINTEGER && id <= crate::builtins::NF_N_ISNAN {
            let r = if argc >= 1 {
                if let Val::Num(n) = self.stack[bp as usize] {
                    if id == crate::builtins::NF_N_ISINTEGER {
                        is_finite(n) && n == to_integer(n)
                    } else if id == crate::builtins::NF_N_ISSAFEINTEGER {
                        is_finite(n) && n == to_integer(n) && n.abs() <= 9007199254740991.0
                    } else if id == crate::builtins::NF_N_ISFINITE {
                        is_finite(n)
                    } else {
                        is_nan(n)
                    }
                } else {
                    false
                }
            } else {
                false
            };
            self.stack[fpos as usize] = Val::Bool(r);
            return true;
        }
        // isNaN / isFinite of a number (others coerce: the full call)
        if (id == crate::builtins::NF_ISNAN || id == crate::builtins::NF_ISFINITE) && argc >= 1 {
            if let Val::Num(n) = self.stack[bp as usize] {
                let r = if id == crate::builtins::NF_ISNAN { is_nan(n) } else { is_finite(n) };
                self.stack[fpos as usize] = Val::Bool(r);
                return true;
            }
        }
        // Math.min / Math.max of numbers
        if (which == 31 || which == 32) && argc <= 8 {
            let mut r = if which == 31 { infinity() } else { -infinity() };
            let mut nan_seen = false;
            let mut numbers = true;
            let mut i: int = 0;
            while i < argc {
                if let Val::Num(n) = self.stack[(bp + i) as usize] {
                    if is_nan(n) {
                        nan_seen = true;
                    } else if which == 31 {
                        if n < r || (n == 0.0 && r == 0.0 && 1.0 / n < 0.0) {
                            r = n;
                        }
                    } else if n > r || (n == 0.0 && r == 0.0 && 1.0 / n > 0.0) {
                        r = n;
                    }
                } else {
                    numbers = false;
                    break;
                }
                i += 1;
            }
            if numbers {
                self.stack[fpos as usize] = Val::Num(if nan_seen { nan() } else { r });
                return true;
            }
        }
        // String(x) of a string or a number
        if id == crate::builtins::NF_STRING && argc >= 1 {
            let a = self.stack[bp as usize].clone();
            let mut r = Val::Undef;
            if let Val::Str(_) = a {
                r = a;
            } else if let Val::Num(n) = a {
                r = self.string_val(number_to_string(n));
            }
            if !matches!(r, Val::Undef) {
                self.stack[fpos as usize] = r;
                return true;
            }
        }
        // "s".slice(a, b) / substring(a, b) with whole-number (or no) bounds
        if (id == crate::builtins::NF_SP_SLICE || id == crate::builtins::NF_SP_SUBSTRING) && argc <= 2 {
            if let Val::Str(h) = self.stack[(fpos + 1) as usize] {
                let a0 = if argc >= 1 { self.stack[bp as usize] } else { Val::Undef };
                let a1 = if argc >= 2 { self.stack[(bp + 1) as usize] } else { Val::Undef };
                if slice_bound_ok(&a0) && slice_bound_ok(&a1) {
                    let s = self.sget(h);
                    let len = self.str_len(&s);
                    let slice = id == crate::builtins::NF_SP_SLICE;
                    let mut a = slice_at(&a0, 0, len, slice);
                    let mut b = slice_at(&a1, len, len, slice);
                    if !slice && a > b {
                        let c = a;
                        a = b;
                        b = c;
                    }
                    let r = if a <= 0 && b >= len {
                        Val::Str(h)
                    } else if a >= b {
                        Val::Str(0)
                    } else {
                        let tw = self.str_slice(&s, a, b);
                        self.string_val(tw)
                    };
                    self.stack[fpos as usize] = r;
                    return true;
                }
            }
        }
        // "s".charCodeAt(i) and n.toString(): no argument vector
        if id == crate::builtins::NF_SP_CHARCODEAT && argc >= 1 {
            let t = self.stack[(fpos + 1) as usize];
            let a = self.stack[bp as usize];
            let r = self.char_code_fast(&t, &a);
            if !matches!(r, Val::Undef) {
                self.stack[fpos as usize] = r;
                return true;
            }
        }
        if id == crate::builtins::NF_NP_TOSTRING && argc <= 1 {
            let radix = if argc == 0 { Val::Num(10.0) } else { self.stack[bp as usize] };
            if let (Val::Num(n), Val::Num(rx)) = (self.stack[(fpos + 1) as usize], radix) {
                if rx != 2.0 && rx != 8.0 && rx != 10.0 && rx != 16.0 && rx != 36.0 {
                    return false;
                }
                let r = self.string_val(crate::num::radix_string(n, rx as int));
                self.stack[fpos as usize] = r;
                return true;
            }
        }
        // The hottest natives, straight off the stack: no argument
        // vector, same checks as their builtins.
        if argc >= 1 {
            let tobj = obj_of(&self.stack[(fpos + 1) as usize]);
            if tobj >= 0 {
                let tclass = self.objs[tobj as usize].class;
                let weak = self.objs[tobj as usize].func == 1;
                let mut r = Val::Undef;
                let mut hit = false;
                if id == crate::builtins::NF_OP_HASOWN && tclass != C_PROXY {
                    let k = self.stack[bp as usize].clone();
                    r = Val::Bool(self.has_own(tobj, &k));
                    if self.throwing {
                        return true;
                    }
                    hit = true;
                } else if id == crate::builtins::NF_AP_PUSH && argc == 1 && tclass == C_ARRAY && self.objs[tobj as usize].extensible && self.objs[tobj as usize].pos != 3 && self.colds[self.objs[tobj as usize].cold as usize].elems2.is_empty() {
                    let a = self.stack[bp as usize].clone();
                    self.push_elem(tobj, a);
                    r = Val::Num(self.evecs[self.objs[tobj as usize].elems as usize].v.len() as double);
                    hit = true;
                } else if (tclass == C_MAP || tclass == C_SET) && !weak {
                    if (tclass == C_MAP && (id == crate::builtins::NF_MP_GET || id == crate::builtins::NF_MP_HAS)) || (tclass == C_SET && id == crate::builtins::NF_SP_HAS) {
                        let k = self.stack[bp as usize].clone();
                        let i = self.map_find(tobj, &k);
                        r = if id != crate::builtins::NF_MP_GET {
                            Val::Bool(i >= 0)
                        } else if i >= 0 {
                            self.colds[self.objs[tobj as usize].cold as usize].elems2[i as usize].clone()
                        } else {
                            Val::Undef
                        };
                        hit = true;
                    } else if tclass == C_MAP && id == crate::builtins::NF_MP_SET && argc >= 2 {
                        let k = self.stack[bp as usize].clone();
                        let v = self.stack[(bp + 1) as usize].clone();
                        self.map_set(tobj, k, v);
                        r = Val::Obj(tobj);
                        hit = true;
                    } else if tclass == C_SET && id == crate::builtins::NF_SETP_ADD {
                        let k = self.stack[bp as usize].clone();
                        self.map_set(tobj, k.clone(), k);
                        r = Val::Obj(tobj);
                        hit = true;
                    }
                } else if weak && ((tclass == C_MAP && (id == crate::builtins::NF_WM_GET || id == crate::builtins::NF_WM_HAS)) || (tclass == C_SET && id == crate::builtins::NF_WS_HAS)) {
                    // WeakMap get/has, WeakSet has: a key that cannot be
                    // held weakly is simply not found
                    let k = self.stack[bp as usize].clone();
                    let i = if obj_of(&k) >= 0 { self.map_find(tobj, &k) } else { -1 };
                    r = if id != crate::builtins::NF_WM_GET {
                        Val::Bool(i >= 0)
                    } else if i >= 0 {
                        self.colds[self.objs[tobj as usize].cold as usize].elems2[i as usize].clone()
                    } else {
                        Val::Undef
                    };
                    hit = true;
                } else if weak && tclass == C_MAP && id == crate::builtins::NF_WM_SET && argc >= 2 && obj_of(&self.stack[bp as usize]) >= 0 && self.objs[obj_of(&self.stack[bp as usize]) as usize].class != C_SYMBOL {
                    let k = self.stack[bp as usize].clone();
                    let v = self.stack[(bp + 1) as usize].clone();
                    self.map_set(tobj, k, v);
                    r = Val::Obj(tobj);
                    hit = true;
                }
                if hit {
                    self.stack[fpos as usize] = r;
                    return true;
                }
            }
        }
        false
    }

    /// `t.charCodeAt(a)` for a string `t` and a whole number `a`, as
    /// NF_SP_CHARCODEAT answers it; undefined when it is not that case.
    pub fn char_code_fast(&mut self, t: &Val, a: &Val) -> Val {
        if let (Val::Str(h), Val::Num(x)) = (t, a) {
            let i = *x as int;
            if (i as double) != *x {
                return Val::Undef;
            }
            let c0 = if i < 0 { -1 } else { self.hunit(*h, i) };
            if c0 != -2 {
                return Val::Num(if c0 < 0 { nan() } else { c0 as double });
            }
            let s = self.sget(*h);
            let c = if i < 0 { -1 } else { self.str_at(&s, i) };
            return Val::Num(if c < 0 { nan() } else { c as double });
        }
        Val::Undef
    }

    /// The calling part of OP_CALL; true when a frame was pushed or the
    /// stack changed frames.
    pub fn call_op(&mut self, argc_in: int) -> bool {
        let mut argc = argc_in;
        loop {
            let fpos = (self.stack.len() as int) - argc - 2;
            let f = self.stack[fpos as usize].clone();
            let fo = obj_of(&f);
            if fo < 0 {
                self.throw_type("value is not a function");
                return false;
            }
            let class = self.objs[fo as usize].class;
            if class == C_FUNCTION {
                return self.enter_function(fo, argc, false, Val::Undef) || true;
            }
            if class == C_NATIVE {
                let id = self.objs[fo as usize].func as int;
                // f.call(this, …) and f.apply(this, args) run in this loop
                if id == crate::builtins::NF_CALL {
                    // [call, f, this, a…] → [f, this, a…]
                    self.stack.remove(fpos as usize);
                    if argc == 0 {
                        self.stack.push(Val::Undef);
                        argc = 1;
                    }
                    argc -= 1;
                    continue;
                }
                if id == crate::builtins::NF_APPLY {
                    // [apply, f, this, arr] → [f, this, a…]
                    let arr = if argc >= 2 { self.stack[(fpos + 3) as usize].clone() } else { Val::Undef };
                    let this = if argc >= 1 { self.stack[(fpos + 2) as usize].clone() } else { Val::Undef };
                    let items = self.array_like_to_vec(&arr);
                    if self.throwing {
                        return false;
                    }
                    self.stack.truncate((fpos + 2) as usize);
                    self.stack.remove(fpos as usize);
                    self.stack.push(this);
                    argc = items.len() as int;
                    for x in items {
                        self.stack.push(x);
                    }
                    continue;
                }
                let bp = fpos + 2;
                if self.native_fast(id, fpos, argc) {
                    if !self.throwing {
                        self.stack.truncate((fpos + 1) as usize);
                    }
                    return false;
                }
                // the arguments in a vector from the pool: no allocation per call
                let mut args: Vec<Val> = take_args(&mut self.arg_pool);
                let mut i = 0;
                while i < argc {
                    args.push(self.stack[(bp + i) as usize]);
                    i += 1;
                }
                let this = self.stack[(fpos + 1) as usize].clone();
                self.stack.truncate(fpos as usize);
                self.native_depth += 1;
                let r = self.call_native(id, fo, this, &args, false, Val::Undef);
                self.native_depth -= 1;
                give_args(&mut self.arg_pool, args);
                self.stack.push(r);
                return false;
            }
            if class == C_PROXY && (self.objs[fo as usize].pos & 1) != 0 {
                let mut args: Vec<Val> = Vec::new();
                let mut i = 0;
                while i < argc {
                    args.push(self.stack[(fpos + 2 + i) as usize].clone());
                    i += 1;
                }
                let this = self.stack[(fpos + 1) as usize].clone();
                self.stack.truncate(fpos as usize);
                self.native_depth += 1;
                let r = self.call_value(f, this, args);
                self.native_depth -= 1;
                self.stack.push(r);
                return false;
            }
            if class == C_BOUND {
                let target = self.objs[fo as usize].env as int;
                let bthis = self.colds[self.objs[fo as usize].cold as usize].prim.clone();
                let bargs = self.evecs[self.objs[fo as usize].elems as usize].v.clone();
                self.stack[fpos as usize] = Val::Obj(target);
                self.stack[(fpos + 1) as usize] = bthis;
                let mut k: usize = 0;
                for a in bargs {
                    self.stack.insert((fpos + 2) as usize + k, a);
                    k += 1;
                }
                argc += k as int;
                continue;
            }
            self.throw_type("value is not a function");
            return false;
        }
    }

    pub fn array_like_to_vec(&mut self, v: &Val) -> Vec<Val> {
        let mut out: Vec<Val> = Vec::new();
        match v {
            Val::Undef | Val::Null => return out,
            Val::Obj(o) => {
                let class = self.objs[*o as usize].class;
                if (class == C_ARRAY && self.objs[*o as usize].pos != 3 && !(self.idx_protos && !self.colds[self.objs[*o as usize].cold as usize].elems2.is_empty())) || class == C_ARGUMENTS {
                    return self.evecs[self.objs[*o as usize].elems as usize].v.clone();
                }
                let l = self.get(v, A_LENGTH);
                let n = to_integer(self.to_number(&l)) as int;
                let mut i = 0;
                while i < n {
                    let x = self.get_index(v, i);
                    out.push(x);
                    i += 1;
                }
                out
            }
            _ => {
                self.throw_type("CreateListFromArrayLike called on non-object");
                out
            }
        }
    }

    /// `new` in the loop: [f, args…]
    pub fn new_op(&mut self, argc: int) -> bool {
        let fpos = (self.stack.len() as int) - argc - 1;
        let f = self.stack[fpos as usize].clone();
        let fo = obj_of(&f);
        if fo < 0 || !self.is_callable(&f) {
            self.throw_type("value is not a constructor");
            return false;
        }
        let class = self.objs[fo as usize].class;
        if class == C_FUNCTION {
            let pi = self.objs[fo as usize].func as int;
            if !self.protos[pi as usize].constructible() {
                self.throw_type("value is not a constructor");
                return false;
            }
            let this = self.make_this(&f);
            self.stack.insert((fpos + 1) as usize, this);
            let t = self.ctor_target(fo);
            if t != fo {
                self.stack[fpos as usize] = Val::Obj(t);
            }
            self.enter_function(t, argc, true, f.clone());
            return true;
        }
        let mut args: Vec<Val> = Vec::new();
        let mut i = 0;
        while i < argc {
            args.push(self.stack[(fpos + 1 + i) as usize].clone());
            i += 1;
        }
        self.stack.truncate(fpos as usize);
        self.native_depth += 1;
        let r = self.construct(f.clone(), args, f.clone());
        self.native_depth -= 1;
        self.stack.push(r);
        false
    }

    // the bodies of run()'s string-append ops; run() stays small enough for
    // Go's inliner to keep its per-op copies on the stack
    /// `cur + b` appended in cur's own buffer when both are strings and
    /// nothing else holds it (Vm::str_append): the new string's handle, or
    /// -1 for the ordinary way.
    #[inline]
    pub fn append_val(&mut self, cur: Val, b: Val) -> int {
        if let (Val::Str(h), Val::Str(hb)) = (cur, b) {
            if hb == 0 {
                return h;
            }
            let bs = self.sget(hb);
            return self.str_append(h, bs.as_str());
        }
        if let (Val::Str(h), Val::Num(y)) = (cur, b) {
            // a number's text (a template's `${s}${i}`, `s += i`)
            let t = number_to_string(y);
            return self.str_append(h, t.as_str());
        }
        -1
    }

    #[inline]
    pub fn add_local_pop(&mut self, op: Op, bp: int) {
        let b = self.pop();
        let i = (bp + (op.a as int)) as usize;
        let h = self.append_val(self.stack[i], b);
        if h >= 0 {
            self.stack[i] = Val::Str(h);
        } else {
            let a = self.stack[i];
            let r = if let (Val::Num(x), Val::Num(y)) = (&a, &b) { Val::Num(x + y) } else { self.add_vals(&a, &b) };
            self.stack[i] = r;
        }
    }

    #[inline]
    pub fn add_env_pop(&mut self, op: Op) {
        let b = self.pop();
        let e = self.get_env((op.a as int));
        let i = op.b as usize;
        let mut done = false;
        if let Val::Str(_) = b {
            let n = self.stack.len() - 1;
            let cur = self.evecs[self.objs[e as usize].elems as usize].v[i];
            if let (Val::Str(x), Val::Str(y)) = (self.stack[n], cur) {
                if x == y {
                    let h = self.append_val(cur, b);
                    if h >= 0 {
                        self.stack.pop();
                        elem_put(&mut self.evecs[self.objs[e as usize].elems as usize], i as usize, Val::Str(h));
                        // the slot holds the new handle, no longer the old
                        // (now its prefix)
                        self.sinc(h);
                        self.sdec(x);
                        done = true;
                    }
                }
            }
        }
        if !done {
            let a = self.pop();
            let r = if let (Val::Num(x), Val::Num(y)) = (&a, &b) { Val::Num(x + y) } else { self.add_vals(&a, &b) };
            if !self.throwing {
                self.put_elem(e, (i) as int, r);
            }
        }
    }

    /// Env slot `i` (`depth` envs out) becomes a + b, a its value before b
    /// was worked out; a string grows in place when nothing else holds it.
    pub fn add_env(&mut self, depth: int, i: int, a: Val, b: Val) {
        let e = self.get_env(depth);
        if let (Val::Str(x), Val::Str(_)) = (a, b) {
            let cur = self.evecs[self.objs[e as usize].elems as usize].v[i as usize];
            if let Val::Str(y) = cur {
                if x == y {
                    let h = self.append_val(cur, b);
                    if h >= 0 {
                        elem_put(&mut self.evecs[self.objs[e as usize].elems as usize], i as usize, Val::Str(h));
                        self.sinc(h);
                        self.sdec(x);
                        return;
                    }
                }
            }
        }
        let r = if let (Val::Num(x), Val::Num(y)) = (&a, &b) { Val::Num(x + y) } else { self.add_vals(&a, &b) };
        if !self.throwing {
            self.put_elem(e, i, r);
        }
    }

    #[inline]
    pub fn add_prop_pop(&mut self, op: Op) {
        let b = self.pop();
        let atom = op.a;
        let mut done = false;
        if let Val::Str(_) = b {
            let n = self.stack.len();
            let o = obj_of(&self.stack[n - 2]);
            if o >= 0 {
                let slot = self.find(o, (atom as int));
                if slot >= 0 && self.attr_at(o, slot) == 0 {
                    let cur = self.objs[o as usize].get_slot(slot as usize);
                    if let (Val::Str(x), Val::Str(y)) = (self.stack[n - 1], cur) {
                        if x == y {
                            let h = self.append_val(cur, b);
                            if h >= 0 {
                                self.stack.pop();
                                self.stack.pop();
                                self.objs[o as usize].set_slot(slot as usize, Val::Str(h));
                                self.sinc(h);
                                self.sdec(x);
                                done = true;
                            }
                        }
                    }
                }
            }
        }
        if !done {
            let a = self.pop();
            let target = self.pop();
            let r = if let (Val::Num(x), Val::Num(y)) = (&a, &b) { Val::Num(x + y) } else { self.add_vals(&a, &b) };
            if !self.throwing {
                self.set(&target, (atom as int), r);
            }
        }
    }

    // the common case of a few ops run() would otherwise send to step()
    #[inline]
    pub fn quick_op(&mut self, op: Op, pi: usize, pc: int) -> bool {
        let mut done = false;
        let n = self.stack.len();
        if (op.code as int) == OP_GET_GLOBAL {
            let g = self.global;
            let slot = self.ic_slot(g, (op.a as int), (op.c as int), false);
            if slot >= 0 {
                let v = self.objs[g as usize].get_slot(slot as usize).clone();
                self.stack.push(v);
                done = true;
            }
        } else if (op.code as int) == OP_INIT_PROP {
            // `key: value` of a literal (b 1 is `__proto__: v`)
            if op.b != 1 {
                let v = self.pop();
                let o = obj_of(&self.stack[n - 2]);
                // c caches the transition: shape before * 65536 + after
                let sh = self.objs[o as usize].shape as int;
                let c = op.c;
                if c >= 0 && ((c >> 16) as int) == sh {
                    self.objs[o as usize].shape = c & 65535;
                    self.push_val(o, v);
                } else {
                    self.define(o, (op.a as int), v, 0);
                    let sh2 = self.objs[o as usize].shape as int;
                    if sh2 != sh && sh < 32768 && sh2 < 65536 && !self.shapes[sh as usize].owned && !self.shapes[sh2 as usize].owned {
                        self.protos[pi].code[(pc - 1) as usize].c = (sh * 65536 + sh2) as i32;
                    }
                }
                done = true;
            }
        } else if (op.code as int) == OP_TOSTR {
            // a template's substitution: an ADD onto a string always comes
            // next, and for a primitive (no symbol: symbols are objects) it
            // gives what ToString would: the value stays as it is
            if !matches!(self.stack[n - 1], Val::Obj(_)) {
                done = true;
            }
        } else if let Val::Num(x) = self.stack[n - 1] {
            if (op.code as int) == OP_TONUM {
                done = true;
            } else {
                self.stack[n - 1] = Val::Num(if (op.code as int) == OP_INC { x + 1.0 } else { x - 1.0 });
                done = true;
            }
        }
        done
    }

    /// The rarer operations; true when the current frame changed.
    pub fn step(&mut self, op: Op, pi: usize, bp: int, fi: usize, pc: &mut int) -> bool {
        match (op.code as int) {
            OP_TDZ => {
                self.stack.push(Val::Obj(self.tdz_obj));
            }
            OP_CHECK_TDZ => {
                let t = self.top().clone();
                if let Val::Obj(o) = t {
                    if o == self.tdz_obj {
                        let n = self.protos[pi].consts[op.a as usize].clone();
                        let ns = self.to_string(&n);
                        self.throw_ref(format!("Cannot access '{}' before initialization", ns).as_str());
                    }
                }
            }
            OP_CHECK_UNINIT => {
                let bound = match self.top() {
                    Val::Obj(o) => *o != self.tdz_obj,
                    _ => true,
                };
                if bound {
                    self.throw_ref("Super constructor may only be called once");
                }
            }
            OP_BIGINT => {
                let v = self.pop();
                // a literal's text: small ones skip the copy
                let n = if let Val::Str(h) = v { crate::bigint::small_big_of(self.sref(h)) } else { crate::bigint::small_big_limit() };
                if n < crate::bigint::small_big_limit() {
                    let b = self.bigint_small(n);
                    self.stack.push(b);
                } else {
                    let s = self.to_string(&v);
                    let b = self.bigint_val(s.as_str());
                    self.stack.push(b);
                }
            }
            OP_ITER_CLOSE => {
                let it = self.pop();
                if op.b == 1 {
                    let e = self.exc.clone();
                    let was = self.throwing;
                    self.iter_close(&it);
                    self.throwing = was;
                    self.exc = e;
                } else {
                    self.iter_close(&it);
                }
            }
            OP_ITER_CLOSE_AT => {
                let n = self.stack.len();
                let it = self.stack[n - 1 - (op.a as usize)].clone();
                self.iter_close(&it);
            }
            OP_GEN_START => {
                self.gen_start(fi, *pc);
                return true;
            }
            OP_YIELD => {
                let v = self.pop();
                self.gen_yield(fi, *pc, v, (op.a as int));
                return true;
            }
            OP_ASYNC_ITER => {
                // GetIterator(v, async): @@asyncIterator, else the sync
                // iterator wrapped
                let v = self.pop();
                let a = self.intern("@@asyncIterator");
                let m = self.get(&v, a);
                if self.throwing {
                    return false;
                }
                if !matches!(m, Val::Undef) && !matches!(m, Val::Null) {
                    let it = self.call_fixed(m, v, 0, Val::Undef, Val::Undef, Val::Undef);
                    if !self.throwing && !is_obj(&it) {
                        self.throw_type("Result of the Symbol.asyncIterator method is not an object");
                    }
                    self.stack.push(it);
                    return false;
                }
                let sync = self.iter_values(&v);
                if self.throwing {
                    return false;
                }
                let wrap = self.async_from_sync;
                let it = self.call_fixed(Val::Obj(wrap), Val::Undef, 1, sync, Val::Undef, Val::Undef);
                self.stack.push(it);
            }
            OP_GEN_RESUME => {
                let mode = self.pop();
                if let Val::Num(m) = mode {
                    if m == 1.0 {
                        *pc = (op.a as int);
                    } else if m == 2.0 {
                        let e = self.pop();
                        self.throw_val(e);
                    }
                }
            }
            OP_ASTAR_CALL => {
                if self.astar_call() {
                    *pc = (op.b as int);
                }
            }
            OP_ASTAR_DONE => {
                let r = self.astar_done();
                if r == 1 {
                    *pc = (op.a as int);
                } else if r == 2 {
                    *pc = (op.b as int);
                }
            }
            OP_YIELD_STAR => {
                let r = self.yield_star_step();
                if r == 1 {
                    *pc = (op.a as int);
                } else if r == 2 {
                    *pc = (op.b as int);
                }
            }
            OP_UNDEF => self.stack.push(Val::Undef),
            OP_NULL => self.stack.push(Val::Null),
            OP_TRUE => self.stack.push(Val::Bool(true)),
            OP_FALSE => self.stack.push(Val::Bool(false)),
            OP_DUP => {
                let v = self.top().clone();
                self.stack.push(v);
            }
            OP_DUP2 => {
                let n = self.stack.len();
                let a = self.stack[n - 2].clone();
                let b = self.stack[n - 1].clone();
                self.stack.push(a);
                self.stack.push(b);
            }
            OP_SWAP => {
                let n = self.stack.len();
                self.stack.swap(n - 1, n - 2);
            }
            OP_OVER => {
                let n = self.stack.len();
                let v = self.stack[n - 2].clone();
                self.stack.push(v);
            }
            OP_ROT3 => {
                let z = self.pop();
                let n = self.stack.len();
                self.stack.insert(n - 2, z);
            }
            OP_ROT4 => {
                let z = self.pop();
                let n = self.stack.len();
                self.stack.insert(n - 3, z);
            }
            OP_GET_ENV => {
                let e = self.get_env((op.a as int));
                let v = self.evecs[self.objs[e as usize].elems as usize].v[op.b as usize].clone();
                self.stack.push(v);
            }
            OP_SET_ENV => {
                let e = self.get_env((op.a as int));
                let v = self.top().clone();
                self.put_elem(e, (op.b as int), v);
            }
            OP_GET_GLOBAL => {
                let g = self.global;
                let slot = self.ic_get(g, (op.a as int), (op.c as int), pi, *pc);
                if slot >= 0 {
                    let v = self.objs[g as usize].get_slot(slot as usize).clone();
                    self.stack.push(v);
                } else if self.has_atom(g, (op.a as int)) {
                    let v = self.get_obj(g, (op.a as int), &Val::Obj(g));
                    self.stack.push(v);
                } else {
                    let n = self.atom_str((op.a as int));
                    self.throw_ref(format!("{} is not defined", n).as_str());
                }
            }
            OP_SET_GLOBAL => {
                let v = self.top().clone();
                self.set_global((op.a as int), v, (op.b as int), pi, *pc);
            }
            OP_DECL_GLOBAL => {
                let g = self.global;
                if self.find(g, (op.a as int)) < 0 {
                    self.add_own(g, (op.a as int), Val::Undef, if op.b == 1 { 0 } else { P_FIXED });
                }
            }
            OP_TYPEOF_GLOBAL => {
                let g = self.global;
                if self.has_atom(g, (op.a as int)) {
                    let v = self.get_obj(g, (op.a as int), &Val::Obj(g));
                    let t = self.type_of(&v);
                    let sv13 = self.string_val(t);
                    self.stack.push(sv13);
                } else {
                    let sv12 = self.str_val("undefined");
                    self.stack.push(sv12);
                }
            }
            OP_DEL_PROP => {
                let v = self.pop();
                match v {
                    Val::Obj(o) => {
                        let k = self.key_val((op.a as int));
                        let r = self.delete(o, &k);
                        self.stack.push(Val::Bool(r));
                    }
                    Val::Undef | Val::Null => self.throw_type("Cannot convert undefined or null to object"),
                    _ => self.stack.push(Val::Bool(true)),
                }
            }
            OP_DEL_ELEM => {
                let k = self.pop();
                let v = self.pop();
                match v {
                    Val::Obj(o) => {
                        let r = self.delete(o, &k);
                        self.stack.push(Val::Bool(r));
                    }
                    Val::Undef | Val::Null => self.throw_type("Cannot convert undefined or null to object"),
                    _ => self.stack.push(Val::Bool(true)),
                }
            }
            OP_INSTANCEOF => {
                let b = self.pop();
                let a = self.pop();
                let r = self.instance_of(&a, &b);
                self.stack.push(Val::Bool(r));
            }
            OP_GET_PRIVATE => {
                let v = self.pop();
                if op.b == -2 {
                    // the AST compiler's private names: a plain lookup
                    let has = match &v {
                        Val::Obj(o) => {
                            let k = self.key_val((op.a as int));
                            self.has_property(*o, &k)
                        }
                        _ => false,
                    };
                    if !has {
                        self.throw_private(op.a as int, "read");
                    } else {
                        let r = self.get(&v, (op.a as int));
                        self.stack.push(r);
                    }
                } else {
                    let r = self.priv_get(&v, op.a as int, op.b as int);
                    if !self.throwing {
                        self.stack.push(r);
                    }
                }
            }
            OP_SET_PRIVATE => {
                let val = self.pop();
                let v = self.pop();
                self.priv_set(&v, op.a as int, op.b as int, op.c as int, val.clone());
                if !self.throwing {
                    self.stack.push(val);
                }
            }
            OP_CALL_EVAL => {
                let argc = op.a as usize;
                let fo = obj_of(&self.stack[self.stack.len() - argc - 2]);
                let direct = fo >= 0 && self.objs[fo as usize].class == C_NATIVE && (self.objs[fo as usize].func as int) == crate::dynamic::NF_EVAL;
                if !direct {
                    // any other callee: as OP_CALL
                    return self.call_op(argc as int);
                }
                let at = self.stack.len() - argc;
                let mut args: Vec<Val> = Vec::new();
                let mut ai = at;
                while ai < self.stack.len() {
                    args.push(self.stack[ai].clone());
                    ai += 1;
                }
                self.stack.truncate(at);
                self.pop();
                self.pop();
                let r = {
                    if args.is_empty() {
                        Val::Undef
                    } else if let Val::Str(s) = &args[0] {
                        let src = self.sget(s.clone()).as_ref().clone();
                        let dv = self.protos[pi].consts[op.b as usize].clone();
                        let desc = match &dv {
                            Val::Str(d) => self.sget(d.clone()).as_ref().clone(),
                            _ => String::new(),
                        };
                        self.direct_eval(src.as_str(), desc.as_str())
                    } else {
                        args[0].clone()
                    }
                };
                if !self.throwing {
                    self.stack.push(r);
                }
            }
            OP_HAS_PRIVATE => {
                let v = self.pop();
                self.pop();
                let o = obj_of(&v);
                if o < 0 {
                    self.throw_type("Cannot use 'in' operator to search for a private field in a non-object");
                } else {
                    let h = self.priv_holder(o, op.a as int, op.b as int);
                    self.stack.push(Val::Bool(h >= 0));
                }
            }
            OP_IN => {
                let b = self.pop();
                let a = self.pop();
                match b {
                    Val::Obj(o) => {
                        let r = self.has_property(o, &a);
                        self.stack.push(Val::Bool(r));
                    }
                    _ => self.throw_type("Cannot use 'in' operator to search for a key in a primitive"),
                }
            }
            OP_NEG => {
                let v = self.pop();
                let p = if is_obj(&v) { self.to_primitive(&v, "number") } else { v };
                if self.class_of(&p) == C_BIGINT {
                    let t = self.big_text(&p);
                    let r = crate::bigint::negate(t.as_str());
                    let b = self.bigint_val(r.as_str());
                    self.stack.push(b);
                } else {
                    let n = self.to_number(&p);
                    self.stack.push(Val::Num(-n));
                }
            }
            OP_TONUM => {
                let v = self.pop();
                if let Val::Num(_) = v {
                    self.stack.push(v);
                } else if self.class_of(&v) == C_BIGINT {
                    // ToNumeric keeps a BigInt; unary + (a = 1) is ToNumber
                    if op.a == 1 {
                        self.throw_type("Cannot convert a BigInt value to a number");
                    } else {
                        self.stack.push(v);
                    }
                } else {
                    let n = self.to_number(&v);
                    self.stack.push(Val::Num(n));
                }
            }
            OP_NOT => {
                let v = self.pop();
                self.stack.push(Val::Bool(!self.truthy_v(&v)));
            }
            OP_BNOT => {
                let v = self.pop();
                let p = if is_obj(&v) { self.to_primitive(&v, "number") } else { v };
                if self.throwing {
                    return false;
                }
                if let Val::Big(x) = p {
                    // ~x is -x - 1
                    let b = self.big_step(-x, -1.0);
                    self.stack.push(b);
                } else if self.class_of(&p) == C_BIGINT {
                    let t = self.big_text(&p);
                    let r = crate::bigint::sub(crate::bigint::negate(t.as_str()).as_str(), "1");
                    let b = self.bigint_val(r.as_str());
                    self.stack.push(b);
                } else {
                    let n = self.to_number(&p);
                    self.stack.push(Val::Num((-to_int32(n) - 1) as double));
                }
            }
            OP_TYPEOF => {
                let v = self.pop();
                let t = self.type_val(&v);
                self.stack.push(t);
            }
            OP_INC | OP_DEC => {
                let v = self.pop();
                if let Val::Big(n) = v {
                    let b = self.big_step(n, if (op.code as int) == OP_INC { 1.0 } else { -1.0 });
                    self.stack.push(b);
                    return false;
                }
                if self.class_of(&v) == C_BIGINT {
                    let t = self.big_text(&v);
                    let r = if (op.code as int) == OP_INC { crate::bigint::add(t.as_str(), "1") } else { crate::bigint::sub(t.as_str(), "1") };
                    let b = self.bigint_val(r.as_str());
                    self.stack.push(b);
                    return false;
                }
                let n = match v {
                    Val::Num(x) => x,
                    _ => self.to_number(&v),
                };
                self.stack.push(Val::Num(if (op.code as int) == OP_INC { n + 1.0 } else { n - 1.0 }));
            }
            OP_TO_KEY => {
                let v = self.pop();
                let p = if is_obj(&v) && self.class_of(&v) != C_SYMBOL { self.to_primitive(&v, "string") } else { v };
                if self.throwing {
                    return false;
                }
                if let Val::Str(_) = p {
                    self.stack.push(p);
                } else if is_obj(&p) && self.class_of(&p) == C_SYMBOL {
                    self.stack.push(p);
                } else {
                    let s = self.to_str(&p);
                    let sv = self.rc_val(s);
                    self.stack.push(sv);
                }
            }
            OP_TOSTR => {
                let v = self.pop();
                if let Val::Str(_) = v {
                    self.stack.push(v);
                } else {
                    let s = self.to_str(&v);
                    let sv11 = self.rc_val(s);
                    self.stack.push(sv11);
                }
            }
            OP_JF_KEEP => {
                if !self.truthy_v(self.top()) {
                    *pc = (op.a as int);
                } else {
                    self.stack.pop();
                }
            }
            OP_JT_KEEP => {
                if self.truthy_v(self.top()) {
                    *pc = (op.a as int);
                } else {
                    self.stack.pop();
                }
            }
            OP_JNN_KEEP => {
                let t = self.top();
                if !(matches!(t, Val::Undef) || matches!(t, Val::Null)) {
                    *pc = (op.a as int);
                } else {
                    self.stack.pop();
                }
            }
            OP_JNOT_UNDEF => {
                let v = self.pop();
                if !matches!(v, Val::Undef) {
                    *pc = (op.a as int);
                }
            }
            OP_JNULLISH => {
                if op.b == 1 {
                    let t = self.top();
                    if matches!(t, Val::Undef) || matches!(t, Val::Null) {
                        self.stack.pop();
                        self.stack.push(Val::Undef);
                        *pc = (op.a as int);
                    }
                } else {
                    let v = self.pop();
                    if matches!(v, Val::Undef) || matches!(v, Val::Null) {
                        let mut k = 1;
                        while k < op.b {
                            self.stack.pop();
                            k += 1;
                        }
                        self.stack.push(Val::Undef);
                        *pc = (op.a as int);
                    }
                }
            }
            OP_NEW => {
                if self.gc_due() {
                    self.gc();
                }
                return self.new_op((op.a as int));
            }
            OP_CALL_SPREAD => {
                let arr = self.pop();
                let items = self.evecs[self.objs[obj_of(&arr) as usize].elems as usize].v.clone();
                let argc = items.len() as int;
                for x in items {
                    self.stack.push(x);
                }
                return self.call_op(argc);
            }
            OP_NEW_SPREAD => {
                let arr = self.pop();
                let items = self.evecs[self.objs[obj_of(&arr) as usize].elems as usize].v.clone();
                let argc = items.len() as int;
                for x in items {
                    self.stack.push(x);
                }
                return self.new_op(argc);
            }
            OP_SUPER_CALL | OP_SUPER_CALL_SPREAD => {
                let mut args: Vec<Val> = Vec::new();
                if (op.code as int) == OP_SUPER_CALL_SPREAD {
                    let arr = self.pop();
                    args = self.evecs[self.objs[obj_of(&arr) as usize].elems as usize].v.clone();
                } else {
                    let n = self.stack.len() as int;
                    let mut i = n - (op.a as int);
                    while i < n {
                        args.push(self.stack[i as usize].clone());
                        i += 1;
                    }
                    self.stack.truncate((n - (op.a as int)) as usize);
                }
                // the class whose constructor is running (arrows: the one
                // they were made in)
                let fobj = self.ctor_of_frame(fi);
                let parent = self.objs[fobj as usize].proto as int;
                let nt = self.frames[fi].new_target.clone();
                let this = self.frames[fi].this_val.clone();
                let pclass = if parent >= 0 { self.objs[parent as usize].class } else { -1 };
                let result: Val;
                if pclass == C_FUNCTION {
                    // run the parent constructor on the same `this`
                    let base = self.frames.len() as int;
                    let argc = args.len() as int;
                    self.stack.push(Val::Obj(parent));
                    self.stack.push(this.clone());
                    for a in args {
                        self.stack.push(a);
                    }
                    if self.enter_function(parent, argc, true, nt) {
                        self.native_depth += 1;
                        self.run(base);
                        self.native_depth -= 1;
                    }
                    if self.throwing {
                        return false;
                    }
                    result = self.pop();
                } else if pclass == C_NATIVE || pclass == C_BOUND {
                    let r = self.construct(Val::Obj(parent), args, nt);
                    if self.throwing {
                        return false;
                    }
                    // the native made its own object: give it this one's fields
                    if let (Val::Obj(t), Val::Obj(n)) = (&this, &r) {
                        self.compact_dict(*t);
                        let keys = self.keys_of(*t);
                        let vals = self.objs[*t as usize].slots();
                        let mut i: usize = 0;
                        while i < keys.len() {
                            self.add_own(*n, keys[i], vals[i].clone(), 0);
                            i += 1;
                        }
                    }
                    result = r;
                } else {
                    self.throw_type("Super constructor is not a constructor");
                    return false;
                }
                self.frames[fi].this_val = result.clone();
                self.run_fields(fobj, &result);
                if self.throwing {
                    return false;
                }
                self.stack.push(result);
            }
            OP_CLOSURE => {
                let f = self.closure((op.a as int));
                self.stack.push(Val::Obj(f));
            }
            OP_CALLEE => {
                let f = self.frames[fi].fobj;
                self.stack.push(Val::Obj(f));
            }
            OP_THIS => {
                let t = self.frames[fi].this_val.clone();
                self.stack.push(t);
            }
            OP_NEW_TARGET => {
                let t = self.frames[fi].new_target.clone();
                self.stack.push(t);
            }
            OP_ARGUMENTS => {
                let a = self.frames[fi].args_obj;
                if op.a >= 2 && a >= 0 {
                    self.map_arg(a, (op.a as int) - 2, op.b as int);
                }
                self.stack.push(if a >= 0 { Val::Obj(a) } else { Val::Undef });
            }
            OP_NEW_OBJECT => {
                let o = self.new_object();
                self.stack.push(Val::Obj(o));
            }
            OP_INIT_PROP => {
                let v = self.pop();
                let o = obj_of(self.top());
                if op.b == 1 {
                    match v {
                        Val::Obj(p) => self.put_proto(o, p),
                        Val::Null => self.put_proto(o, -1),
                        _ => {}
                    }
                } else {
                    self.define(o, (op.a as int), v, 0);
                }
            }
            OP_INIT_ELEM => {
                let v = self.pop();
                let k = self.pop();
                let o = obj_of(self.top());
                self.name_by_key(&v, &k);
                self.define_elem(o, &k, v, 0);
            }
            OP_DEFINE_FIELD => {
                let v = self.pop();
                let o = obj_of(self.top());
                if o >= 0 {
                    let slot = self.find(o, op.a as int);
                    if op.b == 1 {
                        // a private field or brand (PrivateFieldAdd)
                        if slot >= 0 {
                            self.throw_private(op.a as int, "initialize twice");
                        } else if !self.objs[o as usize].extensible {
                            self.throw_private(op.a as int, "add to a non-extensible object");
                        } else {
                            self.add_own(o, (op.a as int), v, 0);
                        }
                    } else if (slot >= 0 && (self.attr_at(o, slot) & P_FIXED) != 0) || (slot < 0 && !self.objs[o as usize].extensible && self.objs[o as usize].class == C_OBJECT) {
                        // CreateDataPropertyOrThrow
                        let n = self.atom_str(op.a as int);
                        self.throw_type(format!("Cannot define property {}, object is not extensible", n).as_str());
                    } else {
                        self.define(o, (op.a as int), v, 0);
                    }
                }
            }
            OP_DEFINE_FIELD_ELEM => {
                let v = self.pop();
                let k = self.pop();
                let o = obj_of(self.top());
                if o >= 0 {
                    self.define_elem(o, &k, v, 0);
                }
            }
            OP_INIT_GETTER | OP_INIT_SETTER => {
                let f = self.pop();
                let o = obj_of(self.top());
                self.put_home(obj_of(&f), o);
                self.define_accessor(o, (op.a as int), f, if (op.code as int) == OP_INIT_GETTER { 0 } else { 1 }, op.b == 1);
            }
            OP_INIT_GETTER_ELEM | OP_INIT_SETTER_ELEM => {
                let f = self.pop();
                let k = self.pop();
                let o = obj_of(self.top());
                let (i, a) = self.to_key(&k);
                let atom = if i >= 0 { self.index_atom(i) } else { a };
                self.name_by_key_pre(&f, &k, if (op.code as int) == OP_INIT_GETTER_ELEM { "get " } else { "set " });
                self.put_home(obj_of(&f), o);
                self.define_accessor(o, atom, f, if (op.code as int) == OP_INIT_GETTER_ELEM { 0 } else { 1 }, op.b == 1);
            }
            OP_INIT_SPREAD => {
                let src = self.pop();
                let o = obj_of(self.top());
                if let Val::Obj(s) = &src {
                    let keys = self.own_keys(*s, false, true);
                    for k in keys {
                        let v = self.get_elem(&src, &k);
                        if self.throwing {
                            return false;
                        }
                        self.define_elem(o, &k, v, 0);
                    }
                } else if let Val::Str(s) = &src {
                    let s = self.sget(s.clone());
                    let mut i: int = 0;
                    for c in s.chars() {
                        let mut t = String::new();
                        t.push(c);
                        let sv10 = self.string_val(t);
                        self.set_index(o, i, sv10);
                        i += 1;
                    }
                }
            }
            OP_DEFINE_METHOD => {
                let f = self.pop();
                let o = obj_of(self.top());
                self.put_home(obj_of(&f), o);
                let hidden = self.is_class_proto(o) || self.is_class_ctor(o);
                self.define(o, (op.a as int), f, if hidden { P_HIDDEN } else { 0 });
            }
            OP_DEFINE_METHOD_ELEM => {
                let f = self.pop();
                let k = self.pop();
                let o = obj_of(self.top());
                self.name_by_key(&f, &k);
                self.put_home(obj_of(&f), o);
                let hidden = self.is_class_proto(o) || self.is_class_ctor(o);
                self.define_elem(o, &k, f, if hidden { P_HIDDEN } else { 0 });
            }
            OP_NEW_ARRAY => {
                // the items go straight into the slot's elements (a free
                // slot keeps its small buffer), no vector in between
                let n = self.stack.len() as int;
                let p = self.array_proto;
                let a = self.alloc(C_ARRAY, p);
                if op.a > 0 {
                    let e = self.elems_id(a);
                    reserve_vals(&mut self.evecs[e].v, op.a as usize);
                }
                let mut i = n - (op.a as int);
                while i < n {
                    let v = self.stack[i as usize];
                    self.push_elem(a, v);
                    i += 1;
                }
                self.stack.truncate((n - (op.a as int)) as usize);
                self.stack.push(Val::Obj(a));
            }
            OP_ARRAY_PUSH => {
                let v = self.pop();
                let a = obj_of(self.top());
                self.push_elem(a, v);
            }
            OP_ARRAY_HOLE => {
                let a = obj_of(self.top());
                self.push_elem(a, Val::Undef);
                let i = (self.evecs[self.objs[a as usize].elems as usize].v.len() as int) - 1;
                self.mark_hole(a, i);
            }
            OP_ARRAY_SPREAD => {
                let src = self.pop();
                let a = obj_of(self.top());
                if let Val::Obj(so) = &src {
                    if a >= 0 && self.array_iter_plain(*so) {
                        // a plain array: its elements in one copy, as
                        // iterable_to_vec would give them
                        self.inc_elems(*so);
                        let de = self.elems_id(a);
                        let se = self.objs[*so as usize].elems as usize;
                        evecs_append(&mut self.evecs, de, se);
                        return false;
                    }
                }
                let items = self.iterable_to_vec(&src);
                if self.throwing {
                    return false;
                }
                let a = obj_of(self.top());
                for x in items {
                    self.push_elem(a, x);
                }
            }
            OP_TEMPLATE_OBJ => {
                // b >= 0: the site's cached object, if made, then a jump
                // past the strings; b < 0: make it from [cooked…, raw…]
                if op.b >= 0 {
                    let c = self.protos[pi].consts[op.b as usize].clone();
                    if is_obj(&c) {
                        self.stack.push(c);
                        *pc = (op.a as int);
                    }
                    return false;
                }
                let slot = (-1 - op.b) as usize;
                let n = self.stack.len() as int;
                let mut cooked: Vec<Val> = Vec::new();
                let mut raws: Vec<Val> = Vec::new();
                let mut i = n - (2 * op.a as int);
                while i < n {
                    if i < n - (op.a as int) {
                        cooked.push(self.stack[i as usize].clone());
                    } else {
                        raws.push(self.stack[i as usize].clone());
                    }
                    i += 1;
                }
                self.stack.truncate((n - (2 * op.a as int)) as usize);
                let raw = self.new_array(raws);
                let a = self.new_array(cooked);
                self.define(a, A_RAW, Val::Obj(raw), P_HIDDEN | P_READONLY | P_FIXED);
                self.freeze_obj(raw);
                self.freeze_obj(a);
                self.roots.push(a);
                self.protos[pi].consts[slot] = Val::Obj(a);
                self.stack.push(Val::Obj(a));
            }
            OP_THROW => {
                let v = self.pop();
                self.throw_val(v);
            }
            OP_TRY => {
                let sp = self.stack.len() as int;
                let env = self.frames[fi].env;
                self.handlers.push(Handler { frame: fi as int, catch_pc: (op.a as int), sp: sp, env: env });
            }
            OP_END_TRY => {
                self.handlers.pop();
            }
            OP_ITER_KEYS => {
                let v = self.pop();
                if let Val::Obj(o) = v {
                    if self.for_in_fast(o) {
                        // no key list: the iterator walks the object's shape
                        let it = self.for_in_iter(o);
                        self.stack.push(Val::Obj(it));
                        return false;
                    }
                }
                let keys = match &v {
                    Val::Obj(o) => self.for_in_keys(*o),
                    Val::Str(s) => {
                        let s = self.sget(s.clone());
                        let mut ks: Vec<Val> = Vec::new();
                        let n = jsstr::len(s.as_str());
                        let mut i = 0;
                        while i < n {
                            ks.push(self.string_val(format!("{}", i)));
                            i += 1;
                        }
                        ks
                    }
                    _ => Vec::new(),
                };
                let arr = self.new_array(keys);
                let ip = self.iter_proto;
                let it = self.alloc(C_ITER, ip);
                self.put_env(it, arr);
                self.objs[it as usize].func = 0;
                // for-in skips keys deleted during the loop
                self.put_home(it, obj_of(&v));
                if let Val::Obj(so) = &v {
                    // keys of a plain object's own only: none can go
                    // without a remove_own, so the deletes so far tell
                    if self.objs[*so as usize].class == C_OBJECT && self.for_in_own_only(*so) {
                        let c = self.objs[it as usize].cold as usize;
                        self.colds[c].prim = Val::Num(self.del_count as double);
                    }
                }
                self.stack.push(Val::Obj(it));
            }
            OP_ITER_VALUES => {
                let v = self.pop();
                let it = self.iter_values(&v);
                if self.throwing {
                    return false;
                }
                self.stack.push(it);
            }
            OP_ITER_NEXT => {
                let it = self.top().clone();
                let o = obj_of(&it);
                let src_obj = if o >= 0 { self.colds[self.objs[o as usize].cold as usize].home } else { -1 };
                loop {
                    match self.iter_next(&it) {
                        Some(v) => {
                            if src_obj >= 0 && self.objs[o as usize].class == C_ITER && self.for_in_recheck(o) {
                                // a key deleted since the loop began is skipped
                                if !self.has_property(src_obj, &v) {
                                    continue;
                                }
                            }
                            self.stack.push(v);
                        }
                        None => {
                            if !self.throwing {
                                *pc = (op.a as int);
                            }
                        }
                    }
                    break;
                }
            }
            OP_PUSH_ENV => {
                let parent = self.frames[fi].env;
                let e = self.alloc(C_ENV, parent);
                let mut i = 0;
                while i < op.a {
                    self.push_elem(e, Val::Undef);
                    i += 1;
                }
                self.frames[fi].env = e;
            }
            OP_POP_ENV => {
                let e = self.frames[fi].env;
                self.frames[fi].env = self.objs[e as usize].proto as int;
            }
            OP_COPY_ENV => {
                let e = self.frames[fi].env;
                let parent = self.objs[e as usize].proto as int;
                let n = self.alloc(C_ENV, parent);
                let mut i = 0;
                while i < self.evecs[self.objs[e as usize].elems as usize].v.len() {
                    let v = self.evecs[self.objs[e as usize].elems as usize].v[i].clone();
                    self.push_elem(n, v);
                    i += 1;
                }
                self.frames[fi].env = n;
            }
            OP_REGEX => {
                // c caches the compiled regex
                let src = self.protos[pi].consts[op.a as usize].clone();
                let mut idx = op.c;
                if idx < 0 {
                    let flags = self.protos[pi].consts[op.b as usize].clone();
                    let s = self.to_string(&src);
                    let f = if matches!(flags, Val::Undef) { String::new() } else { self.to_string(&flags) };
                    idx = (self.regexp_index(s.as_str(), f.as_str()) as i32);
                    if idx < 0 {
                        return false;
                    }
                    self.protos[pi].code[(*pc - 1) as usize].c = idx;
                }
                let r = self.make_regexp((idx as int), src);
                self.stack.push(r);
            }
            OP_CLASS => {
                let mut sup = Val::Undef;
                if op.a == 1 {
                    sup = self.pop();
                }
                let ctor = obj_of(self.top());
                let mut proto_parent = self.object_proto;
                if op.a == 1 {
                    match &sup {
                        Val::Null => {
                            proto_parent = -1;
                        }
                        Val::Obj(s) => {
                            if !self.is_callable(&sup) {
                                self.throw_type("Class extends value is not a constructor or null");
                                return false;
                            }
                            let sp = self.get_obj(*s, A_PROTOTYPE, &sup);
                            match sp {
                                Val::Obj(p) => proto_parent = p,
                                Val::Null => proto_parent = -1,
                                _ => {
                                    self.throw_type("Class extends value does not have valid prototype property");
                                    return false;
                                }
                            }
                            self.put_proto(ctor, *s);
                        }
                        _ => {
                            self.throw_type("Class extends value is not a constructor or null");
                            return false;
                        }
                    }
                }
                let p = self.alloc(C_OBJECT, proto_parent);
                self.add_own(p, A_CONSTRUCTOR, Val::Obj(ctor), P_HIDDEN);
                self.objs[p as usize].pos = 1; // a class prototype: methods hidden
                self.define(ctor, A_PROTOTYPE, Val::Obj(p), P_HIDDEN | P_READONLY | P_FIXED);
                self.put_home(ctor, p);
                self.stack.push(Val::Obj(p));
            }
            OP_SET_FIELDS => {
                let f = self.pop();
                let ctor = obj_of(self.top());
                if op.a == 1 {
                    // the static initializer: [ctor, f] with ctor its home
                    self.put_home(obj_of(&f), ctor);
                    let n = self.stack.len();
                    self.stack[n - 1] = f;
                } else {
                    self.put_home(obj_of(&f), self.colds[self.objs[ctor as usize].cold as usize].home);
                    self.put_elems2(ctor, vec![f]);
                }
            }
            OP_SET_SUPER | OP_SET_SUPER_ELEM => {
                let v = self.pop();
                let key = if (op.code as int) == OP_SET_SUPER { self.key_val((op.a as int)) } else { self.pop() };
                let fobj = self.frames[fi].fobj;
                let home = self.colds[self.objs[fobj as usize].cold as usize].home;
                let this = self.frames[fi].this_val.clone();
                if home < 0 {
                    self.throw_syntax("'super' keyword unexpected here");
                    return false;
                }
                let (i, a) = self.to_key(&key);
                if self.throwing {
                    return false;
                }
                let atom = if i >= 0 { self.index_atom(i) } else { a };
                self.super_set(home, atom, i >= 0, v.clone(), this);
                if self.throwing {
                    return false;
                }
                self.stack.push(v);
            }
            OP_GET_SUPER | OP_GET_SUPER_ELEM => {
                let key = if (op.code as int) == OP_GET_SUPER { self.key_val((op.a as int)) } else { self.pop() };
                let fobj = self.frames[fi].fobj;
                let home = self.colds[self.objs[fobj as usize].cold as usize].home;
                let this = self.frames[fi].this_val.clone();
                if home < 0 {
                    self.throw_syntax("'super' keyword unexpected here");
                    return false;
                }
                let start = self.objs[home as usize].proto as int;
                if start < 0 {
                    self.stack.push(Val::Undef);
                } else {
                    let (i, a) = self.to_key(&key);
                    let atom = if i >= 0 { self.index_atom(i) } else { a };
                    let v = self.get_obj(start, atom, &this);
                    self.stack.push(v);
                }
            }
            OP_REQUIRE_OBJ => {
                let t = self.top();
                if matches!(t, Val::Undef) || matches!(t, Val::Null) {
                    self.throw_type("Cannot destructure 'undefined' or 'null'");
                }
            }
            OP_OBJ_REST => {
                let n = self.stack.len() as int;
                // the keys taken already: strings, or symbols (by identity);
                // a names on top, b computed keys below the source
                let mut skip: Vec<String> = Vec::new();
                let mut skip_syms: Vec<int> = Vec::new();
                let below = n - (op.a as int) - 1 - (op.b as int);
                let mut i = below;
                while i < n {
                    if i != n - (op.a as int) - 1 {
                        let k = self.stack[i as usize].clone();
                        if self.class_of(&k) == C_SYMBOL {
                            skip_syms.push(obj_of(&k));
                        } else {
                            let s = self.to_string(&k);
                            skip.push(s);
                        }
                    }
                    i += 1;
                }
                let src = self.stack[(n - (op.a as int) - 1) as usize].clone();
                self.stack.truncate(below as usize);
                self.stack.push(src.clone());
                let r = self.new_object();
                let so = if is_obj(&src) { obj_of(&src) } else if matches!(src, Val::Undef) || matches!(src, Val::Null) { -1 } else { self.to_object(&src) };
                if so >= 0 {
                    let srcv = Val::Obj(so);
                    let keys = self.own_keys(so, false, true);
                    for k in keys {
                        let is_sym = self.class_of(&k) == C_SYMBOL;
                        let ks = if let Val::Str(x) = &k { self.sref(*x).to_string() } else if is_sym { String::new() } else { self.to_string(&k) };
                        let mut skipped = is_sym && skip_syms.contains(&obj_of(&k));
                        if !is_sym {
                            for x in skip.iter() {
                                if x.as_str() == ks.as_str() {
                                    skipped = true;
                                }
                            }
                        }
                        if skipped {
                            continue;
                        }
                        let v = self.get_elem(&srcv, &k);
                        if self.throwing {
                            return false;
                        }
                        self.define_elem(r, &k, v, 0);
                    }
                }
                self.stack.push(Val::Obj(r));
            }
            OP_TO_ARRAY => {
                let v = self.pop();
                if matches!(v, Val::Undef) || matches!(v, Val::Null) {
                    self.throw_type("value is not iterable");
                    return false;
                }
                if op.a > 0 && obj_of(&v) >= 0 && self.array_iter_plain(obj_of(&v)) {
                    // destructuring a plain array: its elements read from it
                    // in place (as its iterator would read them), no copy
                    self.stack.push(v);
                    return false;
                }
                let items = if op.a > 0 { self.iterable_take(&v, ((op.a - 1) as int)) } else { self.iterable_to_vec(&v) };
                if self.throwing {
                    return false;
                }
                let a = self.new_array(items);
                self.stack.push(Val::Obj(a));
            }
            OP_ARRAY_REST => {
                let a = obj_of(self.top());
                let n = self.evecs[self.objs[a as usize].elems as usize].v.len() as int;
                let mut items: Vec<Val> = Vec::new();
                let mut i = op.a;
                while (i as int) < n {
                    items.push(self.evecs[self.objs[a as usize].elems as usize].v[i as usize].clone());
                    i += 1;
                }
                let r = self.new_array(items);
                self.stack.push(Val::Obj(r));
            }
            OP_SET_NAME => {
                let f = obj_of(self.top());
                if f >= 0 {
                    let class = self.objs[f as usize].class;
                    let name = self.protos[pi].consts[op.a as usize].clone();
                    if class == C_FUNCTION {
                        let fp = self.objs[f as usize].func as int;
                        if self.protos[fp as usize].name.is_empty() {
                            if let Val::Str(s) = &name {
                                let s = self.sget(s.clone());
                                self.protos[fp as usize].name = s.as_ref().clone();
                            }
                        }
                    }
                }
            }
            OP_CONST_ERROR => {
                if op.b == 1 {
                    // a strict write to a `with` binding that went away
                    let n = self.protos[pi].consts[op.a as usize].clone();
                    let ns = self.to_string(&n);
                    self.throw_ref(format!("{} is not defined", ns).as_str());
                } else {
                    self.throw_type("Assignment to constant variable.");
                }
            }
            OP_NOP => {}
            _ => {
                let _ = bp;
                self.throw_type(format!("internal: unknown op {}", op.code).as_str());
            }
        }
        false
    }

    fn is_class_ctor(&self, o: int) -> bool {
        let ob = &self.objs[o as usize];
        ob.class == C_FUNCTION && self.protos[ob.func as usize].class_ctor
    }

    fn is_class_proto(&self, o: int) -> bool {
        self.objs[o as usize].pos == 1 && self.objs[o as usize].class == C_OBJECT
    }

    // ---- collection
    //
    // Reference counting frees most garbage when it becomes garbage; a
    // mark-and-sweep collection frees the rest (cycles) and corrects the
    // counts.
    //
    // A count (JsObj.rc) is the number of references to the object from
    // other objects: properties, elements, a closure's scope, a prototype.
    // References from the stack, the frames, the handlers and the machine's
    // own fields (the roots) are not counted, so pushing, popping and
    // moving values costs nothing (deferred reference counting). An object
    // whose count falls to 0 goes to `zct` (and a new object starts there);
    // when the table is full, at a safe point, `reclaim` frees the objects
    // in it that no root holds, and what only they held.
    //
    // A missing increment would free a live object; the counts are
    // therefore written where the heap changes (inc / dec, set_slot, ...),
    // and `rc_check` (tests) checks every reclaim against a full mark.

    #[inline]
    pub fn inc(&mut self, v: &Val) {
        match v {
            Val::Obj(o) => {
                self.objs[*o as usize].rc += 1;
            }
            Val::Str(h) => {
                self.sinc(*h);
            }
            _ => {}
        }
    }

    #[inline]
    pub fn inc_o(&mut self, o: int) {
        if o >= 0 {
            self.objs[o as usize].rc += 1;
        }
    }

    #[inline]
    pub fn dec(&mut self, v: &Val) {
        match v {
            Val::Obj(o) => {
                self.dec_o(*o);
            }
            Val::Str(h) => {
                self.sdec(*h);
            }
            _ => {}
        }
    }

    #[inline]
    pub fn dec_o(&mut self, o: int) {
        if o >= 0 {
            let ob = &mut self.objs[o as usize];
            ob.rc -= 1;
            if ob.rc <= 0 && !ob.zct && !ob.free {
                ob.zct = true;
                self.zct.push(o);
                if (self.zct.len() as int) >= self.zct_limit {
                    self.gc_flag = true;
                }
            }
        }
    }

    /// Counts each object of `vs` (values put in the heap at once).
    pub fn inc_all(&mut self, vs: &Vec<Val>) {
        let n = vs.len();
        let mut i: usize = 0;
        while i < n {
            let v = vs[i];
            self.inc(&v);
            i += 1;
        }
    }

    pub fn dec_all(&mut self, vs: &Vec<Val>) {
        let n = vs.len();
        let mut i: usize = 0;
        while i < n {
            let v = vs[i];
            self.dec(&v);
            i += 1;
        }
    }

    /// Property slot `slot` of `o` becomes `v`.
    #[inline]
    pub fn put_slot(&mut self, o: int, slot: int, v: Val) {
        self.inc(&v);
        let old = self.objs[o as usize].get_slot(slot as usize);
        self.objs[o as usize].set_slot(slot as usize, v);
        self.dec(&old);
    }

    /// Element `i` of `o` becomes `v`.
    #[inline]
    pub fn put_elem(&mut self, o: int, i: int, v: Val) {
        self.inc(&v);
        let old = self.evecs[self.objs[o as usize].elems as usize].v[i as usize];
        elem_put(&mut self.evecs[self.objs[o as usize].elems as usize], i as usize, v);
        self.dec(&old);
    }

    #[inline]
    pub fn push_val(&mut self, o: int, v: Val) {
        self.inc(&v);
        self.objs[o as usize].push_slot(v);
    }

    pub fn remove_val(&mut self, o: int, slot: int) {
        let v = self.objs[o as usize].remove_slot(slot as usize);
        self.dec(&v);
    }

    /// `o`'s elements' index in `evecs`, given one on its first element.
    #[inline(always)]
    pub fn elems_id(&mut self, o: int) -> usize {
        let e = self.objs[o as usize].elems;
        if e != 0 {
            return e as usize;
        }
        let n = self.new_evec();
        self.objs[o as usize].elems = n as i32;
        n
    }

    /// An empty vector of elements in `evecs`, a free one when there is.
    #[inline(never)]
    pub fn new_evec(&mut self) -> usize {
        if !self.efree.is_empty() {
            return self.efree.pop().unwrap() as usize;
        }
        grow_evecs(&mut self.evecs);
        self.evecs.push(elems_of(Vec::new()));
        self.evecs.len() - 1
    }

    #[inline(always)]
    /// A reference more to each of `o`'s elements.
    pub fn inc_elems(&mut self, o: int) {
        let e = self.objs[o as usize].elems as usize;
        inc_refs(&mut self.objs, &mut self.strs, &self.evecs[e]);
    }

    #[inline(always)]
    pub fn push_elem(&mut self, o: int, v: Val) {
        self.inc(&v);
        let e = self.elems_id(o);
        self.evecs[e].v.push(v);
    }

    pub fn put_elem2(&mut self, o: int, i: int, v: Val) {
        self.inc(&v);
        let c = self.cold_id(o) as usize;
        let old = self.colds[c].elems2[i as usize];
        self.colds[c].elems2[i as usize] = v;
        self.dec(&old);
    }

    pub fn push_elem2(&mut self, o: int, v: Val) {
        self.inc(&v);
        let c = self.cold_id(o) as usize;
        self.colds[c].elems2.push(v);
    }

    /// `o`'s elements become `vs`.
    pub fn put_elems(&mut self, o: int, vs: Vec<Val>) {
        self.inc_all(&vs);
        let n = self.evecs[self.objs[o as usize].elems as usize].v.len();
        let mut i: usize = 0;
        while i < n {
            let x = self.evecs[self.objs[o as usize].elems as usize].v[i];
            self.dec(&x);
            i += 1;
        }
        let e = self.elems_id(o);
        elems_set(&mut self.evecs[e], vs);
    }

    pub fn put_elems2(&mut self, o: int, vs: Vec<Val>) {
        if vs.is_empty() && self.objs[o as usize].cold == 0 {
            return;
        }
        self.inc_all(&vs);
        let n = self.colds[self.objs[o as usize].cold as usize].elems2.len();
        let mut i: usize = 0;
        while i < n {
            let x = self.colds[self.objs[o as usize].cold as usize].elems2[i];
            self.dec(&x);
            i += 1;
        }
        let c = self.cold_id(o) as usize;
        self.colds[c].elems2 = vs;
    }

    /// `o`'s elements past `n` go.
    pub fn cut_elems(&mut self, o: int, n: int) {
        let len = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
        if n >= len {
            return;
        }
        let mut i = n;
        while i < len {
            let x = self.evecs[self.objs[o as usize].elems as usize].v[i as usize];
            self.dec(&x);
            i += 1;
        }
        self.evecs[self.objs[o as usize].elems as usize].v.truncate(n as usize);
    }

    pub fn cut_elems2(&mut self, o: int, n: int) {
        let len = self.colds[self.objs[o as usize].cold as usize].elems2.len() as int;
        if n >= len {
            return;
        }
        let mut i = n;
        while i < len {
            let x = self.colds[self.objs[o as usize].cold as usize].elems2[i as usize];
            self.dec(&x);
            i += 1;
        }
        let c = self.cold_id(o) as usize;
        self.colds[c].elems2.truncate(n as usize);
    }

    /// The last element of `o`, taken out (the caller owns its count).
    pub fn pop_elem_val(&mut self, o: int) -> Val {
        match self.evecs[self.objs[o as usize].elems as usize].v.pop() {
            Some(v) => v,
            None => Val::Undef,
        }
    }

    pub fn pop_elem(&mut self, o: int) {
        let v = self.pop_elem_val(o);
        self.dec(&v);
    }

    pub fn insert_elem(&mut self, o: int, i: int, v: Val) {
        self.inc(&v);
        let e = self.elems_id(o);
        self.evecs[e].v.insert(i as usize, v);
    }

    pub fn remove_elem(&mut self, o: int, i: int) {
        let v = self.evecs[self.objs[o as usize].elems as usize].v.remove(i as usize);
        self.dec(&v);
    }

    pub fn put_prim(&mut self, o: int, v: Val) {
        if self.objs[o as usize].cold == 0 && matches!(v, Val::Undef) {
            return;
        }
        self.inc(&v);
        let c = self.cold_id(o) as usize;
        let old = self.colds[c].prim;
        self.colds[c].prim = v;
        self.dec(&old);
    }

    pub fn put_proto(&mut self, o: int, p: int) {
        self.inc_o(p);
        if p >= 0 {
            self.objs[p as usize].is_proto = true;
        }
        let old = self.objs[o as usize].proto as int;
        self.objs[o as usize].proto = p as i32;
        self.dec_o(old);
    }

    pub fn put_env(&mut self, o: int, e: int) {
        self.inc_o(e);
        let old = self.objs[o as usize].env as int;
        self.objs[o as usize].env = e as i32;
        self.dec_o(old);
    }

    pub fn put_home(&mut self, o: int, h: int) {
        if self.objs[o as usize].cold == 0 && h < 0 {
            return;
        }
        self.inc_o(h);
        let c = self.cold_id(o) as usize;
        let old = self.colds[c].home;
        self.colds[c].home = h;
        self.dec_o(old);
    }

    /// `o`'s Cold, made on first use.
    pub fn cold_id(&mut self, o: int) -> int {
        let c = self.objs[o as usize].cold as int;
        if c > 0 {
            return c;
        }
        let id = if self.free_colds.is_empty() {
            self.colds.push(Cold::new());
            (self.colds.len() as int) - 1
        } else {
            self.free_colds.pop().unwrap()
        };
        self.objs[o as usize].cold = id as i32;
        id
    }

    pub fn free_cold(&mut self, o: int) {
        let c = self.objs[o as usize].cold as int;
        if c > 0 {
            reset_cold(&mut self.colds, c as usize);
            self.free_colds.push(c);
            self.objs[o as usize].cold = 0;
        }
    }

    fn mark_obj(&mut self, o: int, work: &mut Vec<int>) {
        if o >= 0 && !self.objs[o as usize].marked {
            self.objs[o as usize].marked = true;
            work.push(o);
        }
    }

    /// Marks what the roots hold (not what that holds): the global object,
    /// the host's roots, the stack, the frames, the handlers, the pending
    /// exception and jobs, and the objects the machine keeps in its fields.
    fn mark_roots(&mut self, work: &mut Vec<int>) {
        let g = self.global;
        self.mark_obj(g, work);
        let nr = self.roots.len();
        let mut r: usize = 0;
        while r < nr {
            let x = self.roots[r];
            self.mark_obj(x, work);
            r += 1;
        }
        let ns = self.symbols.len();
        r = 0;
        while r < ns {
            let x = self.symbols[r];
            self.mark_obj(x, work);
            r += 1;
        }
        let fields = vec![
            self.object_proto,
            self.function_proto,
            self.array_proto,
            self.string_proto,
            self.number_proto,
            self.boolean_proto,
            self.symbol_proto,
            self.error_proto,
            self.type_error_proto,
            self.range_error_proto,
            self.reference_error_proto,
            self.syntax_error_proto,
            self.regexp_proto,
            self.date_proto,
            self.map_proto,
            self.set_proto,
            self.iter_proto,
            self.promise_proto,
            self.iterator_proto,
            self.map_iter_proto,
            self.set_iter_proto,
            self.string_iter_proto,
            self.generator_proto,
            self.generator_function_proto,
            self.async_function_proto,
            self.async_generator_proto,
            self.async_generator_function_proto,
            self.async_from_sync,
            self.array_generic,
            self.proxy_hooks,
            self.thrower,
            self.callee_pair,
            self.tdz_obj,
            self.bigint_proto,
            self.array_values_fn,
        ];
        for x in fields {
            self.mark_obj(x, work);
        }
        let mut held: Vec<int> = Vec::new();
        for (_, v) in self.symbol_registry.iter() {
            held.push(*v);
        }
        for x in held {
            self.mark_obj(x, work);
        }
        let n = self.stack.len();
        let mut i: usize = 0;
        while i < n {
            if let Val::Obj(x) = self.stack[i] {
                self.mark_obj(x, work);
            }
            i += 1;
        }
        let nf = self.frames.len();
        let mut k: usize = 0;
        while k < nf {
            let (fo, env, t, nt, ao, gen) = {
                let f = &self.frames[k];
                (f.fobj, f.env, obj_of(&f.this_val), obj_of(&f.new_target), f.args_obj, f.gen)
            };
            self.mark_obj(gen, work);
            self.mark_obj(fo, work);
            self.mark_obj(env, work);
            self.mark_obj(t, work);
            self.mark_obj(nt, work);
            self.mark_obj(ao, work);
            k += 1;
        }
        let nh = self.handlers.len();
        let mut h: usize = 0;
        while h < nh {
            let e = self.handlers[h].env;
            self.mark_obj(e, work);
            h += 1;
        }
        let exc = obj_of(&self.exc);
        self.mark_obj(exc, work);
        let nt = self.temp_roots.len();
        i = 0;
        while i < nt {
            if let Val::Obj(x) = self.temp_roots[i] {
                self.mark_obj(x, work);
            }
            i += 1;
        }
        let nj = self.jobs.len();
        i = 0;
        while i < nj {
            if let Val::Obj(x) = self.jobs[i] {
                self.mark_obj(x, work);
            }
            i += 1;
        }
    }

    /// Marks everything reachable from `work` (marked already); with
    /// `count`, every reference followed is counted into JsObj.rc.
    fn trace_ref(&mut self, x: int, count: bool, work: &mut Vec<int>) {
        if x >= 0 {
            if count {
                self.objs[x as usize].rc += 1;
            }
            self.mark_obj(x, work);
        }
    }

    fn trace(&mut self, work: &mut Vec<int>, count: bool) {
        while !work.is_empty() {
            let o = work.pop().unwrap();
            let (proto, env, home) = {
                let ob = &self.objs[o as usize];
                // an ArrayBuffer's home is what its numbers hold (typed.rs)
                (ob.proto, ob.env, if ob.class == C_ARRAYBUFFER { -1 } else { self.colds[ob.cold as usize].home })
            };
            let prim = obj_of(&self.colds[self.objs[o as usize].cold as usize].prim);
            // no vector per object: four calls
            self.trace_ref(proto as int, count, work);
            self.trace_ref(env as int, count, work);
            self.trace_ref(home, count, work);
            self.trace_ref(prim, count, work);
            let nv = self.objs[o as usize].nv as usize;
            let mut j: usize = 0;
            while j < nv {
                if let Val::Obj(x) = self.objs[o as usize].get_slot(j) {
                    if count {
                        self.objs[x as usize].rc += 1;
                    }
                    self.mark_obj(x, work);
                }
                j += 1;
            }
            let ne = self.evecs[self.objs[o as usize].elems as usize].v.len();
            j = 0;
            while j < ne {
                if let Val::Obj(x) = self.evecs[self.objs[o as usize].elems as usize].v[j] {
                    if count {
                        self.objs[x as usize].rc += 1;
                    }
                    self.mark_obj(x, work);
                }
                j += 1;
            }
            let ne2 = self.colds[self.objs[o as usize].cold as usize].elems2.len();
            j = 0;
            while j < ne2 {
                if let Val::Obj(x) = self.colds[self.objs[o as usize].cold as usize].elems2[j] {
                    if count {
                        self.objs[x as usize].rc += 1;
                    }
                    self.mark_obj(x, work);
                }
                j += 1;
            }
        }
    }

    /// A collection when one is due, else a reclaim (the safe points call
    /// this when gc_due).
    pub fn gc(&mut self) {
        if self.rc_on && self.alloc_count < self.gc_threshold && self.made < self.made_limit && self.snew < self.slimit && self.sbytes < self.sbytes_limit {
            self.reclaim();
            return;
        }
        self.collect();
    }

    /// The mark-and-sweep collection: frees everything unreachable (cycles
    /// too) and counts every object's references again.
    pub fn collect(&mut self) {
        self.gc_runs += 1;
        let total = self.objs.len();
        let mut old: Vec<int> = Vec::new();
        let mut i2: usize = 0;
        while i2 < total {
            if self.rc_check {
                old.push(self.objs[i2].rc as int);
            }
            self.objs[i2].rc = 0;
            i2 += 1;
        }
        let mut work: Vec<int> = Vec::new();
        self.mark_roots(&mut work);
        self.trace(&mut work, true);
        // BigInts no one holds leave the (weak) table while their text is
        // still there
        if !self.bigints.is_empty() {
            let mut dead: Vec<String> = Vec::new();
            for (k, v) in self.bigints.iter() {
                if !self.objs[*v as usize].marked {
                    dead.push(k.clone());
                }
            }
            for k in dead {
                self.bigints.remove(k.as_str());
            }
        }
        if !self.strs.is_empty() {
            self.mark_strs();
        }
        let mut live: int = 0;
        self.zct.clear();
        i2 = 0;
        while i2 < total {
            if self.objs[i2].marked {
                self.objs[i2].marked = false;
                live += 1;
                if self.rc_check && old[i2] < self.objs[i2].rc as int {
                    // a reference was not counted
                    self.rc_errors += 1;
                    if self.rc_errors <= 20 {
                        let c = self.objs[i2].class;
                        let nr = self.objs[i2].rc as int;
                        self.rc_log.push(format!("rc: low count {} < {} (class {}, #{})", old[i2], nr, c, i2));
                    }
                }
                if self.rc_on && self.objs[i2].rc == 0 {
                    // held only by roots
                    self.objs[i2].zct = true;
                    self.zct.push(i2 as int);
                } else {
                    self.objs[i2].zct = false;
                }
                // a survivor's buffers to their size (it lives on)
                let e = self.objs[i2].elems as usize;
                trim_obj(&mut self.objs[i2], &mut self.evecs[e].v);
            } else if !self.objs[i2].free {
                self.release(i2 as int);
                self.free_list.push(i2 as int);
                let e = self.objs[i2].elems as usize;
                drop_bufs(&mut self.objs[i2], &mut self.evecs[e].v);
            } else {
                let e = self.objs[i2].elems as usize;
                drop_bufs(&mut self.objs[i2], &mut self.evecs[e].v);
            }
            i2 += 1;
        }
        // the free element vectors' buffers go too
        let ne = self.efree.len();
        let mut q: usize = 0;
        while q < ne {
            let e = self.efree[q] as usize;
            elems_set(&mut self.evecs[e], Vec::new());
            q += 1;
        }
        let nf = self.freed.len();
        let mut k: usize = 0;
        while k < nf {
            let x = self.freed[k];
            self.free_list.push(x);
            k += 1;
        }
        self.freed.clear();
        self.live = live;
        self.alloc_count = 0;
        self.gc_threshold = if live * 2 > 200000 { live * 2 } else { 200000 };
        self.made = 0;
        self.made_limit = if live * MADE_PER_LIVE > MADE_MIN { live * MADE_PER_LIVE } else { MADE_MIN };
        self.zct_limit = self.next_zct_limit();
        self.gc_recheck();
    }

    /// Tests: every safe point reclaims and checks the counts (rc_errors,
    /// rc_log).
    pub fn rc_stress(&mut self) {
        self.rc_check = true;
        self.zct_min = 1;
        self.zct_limit = 1;
        // and the string table is swept after every few strings
        self.slimit_min = 16;
        self.slimit = 16;
        self.sbytes_limit = 16;
        self.gc_recheck();
    }

    fn next_zct_limit(&self) -> int {
        let n = (self.zct.len() as int) * 2;
        if n > self.zct_min {
            n
        } else {
            self.zct_min
        }
    }

    /// Empties slot `o` (garbage): its shape and Cold go back.
    #[inline]
    fn release(&mut self, o: int) {
        // shape 0 (arrays, scopes, most functions) is never owned
        if self.objs[o as usize].shape != 0 {
            self.free_shape(o);
        }
        self.free_cold(o);
        let e = self.objs[o as usize].elems as usize;
        if e > 0 {
            clear_elems(&mut self.evecs[e].v);
            self.efree.push(e as i32);
            self.objs[o as usize].elems = 0;
        }
        let ob = &mut self.objs[o as usize];
        ob.clear();
        ob.free = true;
        ob.zct = false;
        ob.marked = false;
    }

    /// release() for reclaim's loop: an object with no owned shape and no
    /// Cold (most) is emptied here, the rest by release().
    #[inline(always)]
    fn release_fast(&mut self, o: int) {
        let (sh, c, e) = {
            let ob = crate::rvm::obj_at(&self.objs, o as usize);
            (ob.shape, ob.cold, ob.elems)
        };
        if c > 0 || (sh != 0 && self.shapes[sh as usize].owned) {
            self.release(o);
            return;
        }
        if e > 0 {
            clear_elems(&mut crate::rvm::evec_mut(&mut self.evecs, e as usize).v);
            self.efree.push(e);
        }
        let ob = crate::rvm::obj_mut(&mut self.objs, o as usize);
        ob.elems = 0;
        ob.clear();
        ob.free = true;
        ob.zct = false;
        ob.marked = false;
    }

    /// Frees the objects of `zct` that no root holds, and what only they
    /// held. The slots freed are reused after the next reclaim: an id of a
    /// dead object may still be compared somewhere (the host's ids).
    pub fn reclaim(&mut self) {
        self.reclaims += 1;
        let nf = self.freed.len();
        let mut k: usize = 0;
        while k < nf {
            let x = self.freed[k];
            self.free_list.push(x);
            k += 1;
        }
        self.freed.clear();
        let mut rooted: Vec<int> = Vec::new();
        self.mark_roots(&mut rooted);
        let mut reach: Vec<bool> = Vec::new();
        if self.rc_check {
            reach = self.reachable();
        }
        let list = take_ints(&mut self.zct, &mut self.zct_spare);
        let n = list.len();
        let mut i: usize = 0;
        let mut work: Vec<int> = Vec::new();
        while i < n {
            let o = list[i];
            i += 1;
            let ob = crate::rvm::obj_mut(&mut self.objs, o as usize);
            if !ob.zct {
                continue;
            }
            ob.zct = false;
            if ob.free || ob.rc > 0 {
                continue;
            }
            if ob.marked {
                ob.zct = true;
                self.zct.push(o);
                continue;
            }
            ob.free = true;
            work.push(o);
            while !work.is_empty() {
                let x = work.pop().unwrap();
                if self.rc_check && reach[x as usize] {
                    self.rc_errors += 1;
                    if self.rc_errors <= 20 {
                        let c = self.objs[x as usize].class;
                        self.rc_log.push(format!("rc: freed a reachable object (class {}, #{})", c, x));
                    }
                }
                self.drop_refs(x, &mut work);
                self.release_fast(x);
                self.freed.push(x);
                self.live -= 1;
                self.alloc_count -= 1;
                self.rc_freed += 1;
            }
        }
        let nr = rooted.len();
        i = 0;
        while i < nr {
            let x = rooted[i] as usize;
            self.objs[x].marked = false;
            i += 1;
        }
        give_ints(&mut self.zct_spare, list);
        self.reclaim_strs();
        self.zct_limit = self.next_zct_limit();
        self.gc_recheck();
    }

    /// The references `x` (being freed) holds are let go; what only it
    /// held joins `work`.
    fn drop_refs(&mut self, x: int, work: &mut Vec<int>) {
        let (proto, env, cold, nv, xe) = {
            let ob = crate::rvm::obj_at(&self.objs, x as usize);
            (ob.proto, ob.env, ob.cold, ob.nv as usize, ob.elems as usize)
        };
        if cold != 0 {
            // few objects have a Cold
            self.drop_refs_cold(x, work);
        }
        self.drop_ref(proto as int, work);
        self.drop_ref(env as int, work);
        let mut j: usize = 0;
        while j < nv {
            match crate::rvm::obj_at(&self.objs, x as usize).get_slot(j) {
                Val::Obj(c) => {
                    self.drop_ref(c, work);
                }
                Val::Str(h) => {
                    self.sdec(h);
                }
                _ => {}
            }
            j += 1;
        }
        if xe == 0 {
            return;
        }
        let ne = crate::rvm::evec_at(&self.evecs, xe).v.len();
        j = 0;
        while j < ne {
            match *crate::rvm::val_at(&crate::rvm::evec_at(&self.evecs, xe).v, j) {
                Val::Obj(c) => {
                    self.drop_ref(c, work);
                }
                Val::Str(h) => {
                    self.sdec(h);
                }
                _ => {}
            }
            j += 1;
        }
    }

    /// drop_refs for what x's Cold holds: a home object, a primitive, the
    /// second elements.
    #[inline(never)]
    fn drop_refs_cold(&mut self, x: int, work: &mut Vec<int>) {
        let home = {
            let ob = crate::rvm::obj_at(&self.objs, x as usize);
            if ob.class == C_ARRAYBUFFER { -1 } else { crate::rvm::cold_at(&self.colds, ob.cold as usize).home }
        };
        let primv = crate::rvm::cold_at(&self.colds, crate::rvm::obj_at(&self.objs, x as usize).cold as usize).prim;
        let prim = obj_of(&primv);
        if crate::rvm::obj_at(&self.objs, x as usize).class == C_BIGINT {
            // out of the weak table before its text goes
            self.forget_bigint(x);
        }
        self.sdec(str_of(&primv));
        self.drop_ref(home, work);
        self.drop_ref(prim, work);
        let ne2 = crate::rvm::cold_at(&self.colds, crate::rvm::obj_at(&self.objs, x as usize).cold as usize).elems2.len();
        let mut j: usize = 0;
        while j < ne2 {
            match crate::rvm::cold_at(&self.colds, crate::rvm::obj_at(&self.objs, x as usize).cold as usize).elems2[j] {
                Val::Obj(c) => {
                    self.drop_ref(c, work);
                }
                Val::Str(h) => {
                    self.sdec(h);
                }
                _ => {}
            }
            j += 1;
        }
    }

    #[inline]
    fn drop_ref(&mut self, c: int, work: &mut Vec<int>) {
        if c < 0 {
            return;
        }
        let ob = crate::rvm::obj_mut(&mut self.objs, c as usize);
        ob.rc -= 1;
        if ob.rc > 0 || ob.free {
            return;
        }
        if ob.marked {
            // a root holds it
            if !ob.zct {
                ob.zct = true;
                self.zct.push(c);
            }
            return;
        }
        ob.free = true;
        work.push(c);
    }

    /// rc_check: which objects a full mark reaches (the marks set by
    /// mark_roots stay).
    fn reachable(&mut self) -> Vec<bool> {
        let total = self.objs.len();
        let mut work: Vec<int> = Vec::new();
        let mut i: usize = 0;
        while i < total {
            if self.objs[i].marked {
                work.push(i as int);
            }
            i += 1;
        }
        let roots = work.clone();
        self.trace(&mut work, false);
        let mut out: Vec<bool> = Vec::new();
        i = 0;
        while i < total {
            out.push(self.objs[i].marked);
            i += 1;
        }
        // back to the roots' marks only
        i = 0;
        while i < total {
            self.objs[i].marked = false;
            i += 1;
        }
        for r in roots {
            self.objs[r as usize].marked = true;
        }
        out
    }

    /// Counts every object's references again and puts the objects with
    /// none in `zct` (a restored snapshot).
    pub fn recount(&mut self) {
        let total = self.objs.len();
        let mut i: usize = 0;
        while i < total {
            self.objs[i].rc = 0;
            self.objs[i].zct = false;
            i += 1;
        }
        self.zct = Vec::new();
        let mut live: int = 0;
        i = 0;
        while i < total {
            if !self.objs[i].free {
                live += 1;
                self.count_refs(i as int);
            }
            i += 1;
        }
        i = 0;
        while i < total {
            if !self.objs[i].free && self.objs[i].rc == 0 && self.rc_on {
                self.objs[i].zct = true;
                self.zct.push(i as int);
            }
            i += 1;
        }
        self.live = live;
        self.zct_limit = self.next_zct_limit();
        self.recount_strs();
        self.gc_recheck();
    }

    /// recount(): the strings' counts again from the live objects and
    /// the prefixes; the unreferenced ones go to szct.
    fn recount_strs(&mut self) {
        let ns = self.strs.len();
        let mut i: usize = 0;
        while i < ns {
            self.strs[i].rc = 0;
            self.strs[i].inz = false;
            i += 1;
        }
        let total = self.objs.len();
        i = 0;
        while i < total {
            if !self.objs[i].free {
                let p = self.colds[self.objs[i].cold as usize].prim;
                self.sinc(str_of(&p));
                let nv = self.objs[i].nv as usize;
                let mut j: usize = 0;
                while j < nv {
                    let h = str_of(&self.objs[i].get_slot(j));
                    self.sinc(h);
                    j += 1;
                }
                let ne = self.evecs[self.objs[i].elems as usize].v.len();
                j = 0;
                while j < ne {
                    let h = str_of(&self.evecs[self.objs[i].elems as usize].v[j]);
                    self.sinc(h);
                    j += 1;
                }
                let ne2 = self.colds[self.objs[i].cold as usize].elems2.len();
                j = 0;
                while j < ne2 {
                    let h = str_of(&self.colds[self.objs[i].cold as usize].elems2[j]);
                    self.sinc(h);
                    j += 1;
                }
            }
            i += 1;
        }
        i = 1;
        while i < ns {
            if self.strs[i].flag != 2 && self.strs[i].up >= 0 {
                let u = self.strs[i].up as usize;
                self.strs[u].rc += 1;
            }
            i += 1;
        }
        self.szct.clear();
        i = 1;
        while i < ns {
            if self.strs[i].flag == 0 && self.strs[i].rc == 0 {
                self.strs[i].inz = true;
                self.szct.push(i as int);
            }
            i += 1;
        }
        self.szct_limit = self.next_szct_limit();
    }

    fn count_refs(&mut self, o: int) {
        let (proto, env, home) = {
            let ob = &self.objs[o as usize];
            (ob.proto, ob.env, if ob.class == C_ARRAYBUFFER { -1 } else { self.colds[ob.cold as usize].home })
        };
        let prim = obj_of(&self.colds[self.objs[o as usize].cold as usize].prim);
        self.inc_o(proto as int);
        self.inc_o(env as int);
        self.inc_o(home);
        self.inc_o(prim);
        let nv = self.objs[o as usize].nv as usize;
        let mut j: usize = 0;
        while j < nv {
            if let Val::Obj(c) = self.objs[o as usize].get_slot(j) {
                self.objs[c as usize].rc += 1;
            }
            j += 1;
        }
        let ne = self.evecs[self.objs[o as usize].elems as usize].v.len();
        j = 0;
        while j < ne {
            if let Val::Obj(c) = self.evecs[self.objs[o as usize].elems as usize].v[j] {
                self.objs[c as usize].rc += 1;
            }
            j += 1;
        }
        let ne2 = self.colds[self.objs[o as usize].cold as usize].elems2.len();
        j = 0;
        while j < ne2 {
            if let Val::Obj(c) = self.colds[self.objs[o as usize].cold as usize].elems2[j] {
                self.objs[c as usize].rc += 1;
            }
            j += 1;
        }
    }
}

/// Objects made that make a collection due, at least (Vm.made_limit), and
/// per live object: a collection's cost is the live heap's, so it stays a
/// small part of the work that made the objects.
pub const MADE_MIN: int = 262144;
pub const MADE_PER_LIVE: int = 8;

/// The smallest zct that makes a reclaim. Small, so that the objects a
/// reclaim frees, and the slots it hands back, are still in the cache:
/// at 8192 a reclaim reads objects made ~1 MB of work ago, and Chart.js
/// missed a 2 MB L2 18M times (2048: 6.0M; 1024 and below cost more in
/// reclaims than they save).
pub const ZCT_MIN: int = 2048;
/// strings made that make a collection due, at least (Vm.slimit)
pub const SLIMIT_MIN: int = 65536;
/// bytes of new strings that make a collection due, at least
/// (Vm.sbytes_limit)
pub const SBYTES_MIN: int = 4194304;

pub fn pow2(n: int) -> double {
    let mut r: double = 1.0;
    let mut i: int = 0;
    while i < n {
        r = r * 2.0;
        i += 1;
    }
    r
}

/// Math.imul: the low 32 bits of the product, in pieces that stay exact.
pub fn imul(a: int, b: int) -> int {
    let al = a & 65535;
    let ah = (a >> 16) & 65535;
    let bl = b & 65535;
    let bh = (b >> 16) & 65535;
    let mid = wrap32(((ah * bl + al * bh) & 65535) * 65536);
    wrap32(al * bl + mid)
}

/// The low 32 bits of `v` as a signed integer.
pub fn wrap32(v: int) -> int {
    let m = v & 4294967295;
    if m >= 2147483648 {
        m - 4294967296
    } else {
        m
    }
}

pub fn arith_num(code: int, x: double, y: double) -> double {
    match code {
        OP_SUB => x - y,
        OP_MUL => x * y,
        OP_DIV => x / y,
        OP_MOD => {
            if y == 0.0 || !is_finite(x) || is_nan(y) {
                return nan();
            }
            if !is_finite(y) {
                return x;
            }
            let r = x % y;
            r
        }
        OP_EXP => {
            if is_nan(y) {
                return nan();
            }
            if (x == 1.0 || x == -1.0) && !is_finite(y) {
                return nan();
            }
            x.powf(y)
        }
        OP_BAND => (to_int32(x) & to_int32(y)) as double,
        OP_BOR => (to_int32(x) | to_int32(y)) as double,
        OP_BXOR => (to_int32(x) ^ to_int32(y)) as double,
        OP_SHL => wrap32(to_int32(x) << (to_int32(y) & 31)) as double,
        OP_SHR => (to_int32(x) >> (to_int32(y) & 31)) as double,
        // in doubles: a target's `int` bit operations may be 32 bits wide
        OP_USHR => (to_uint32(x) / pow2(to_int32(y) & 31)).floor(),
        _ => nan(),
    }
}

/// Rust: the slot table grows by an eighth (at least 1024 slots), not by
/// doubling: a JsObj is big and most of a doubled table would stay empty.
/// `v`'s items, `v` left empty. Rust: `v` takes `spare`'s buffer and the
/// items keep theirs (no copy); the other targets copy them.
#[ranger::target(rust)]
pub fn take_ints(v: &mut Vec<int>, spare: &mut Vec<int>) -> Vec<int> {
    let mut out = std::mem::take(spare);
    std::mem::swap(v, &mut out);
    out
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn take_ints(v: &mut Vec<int>, _spare: &mut Vec<int>) -> Vec<int> {
    let out = v.clone();
    v.clear();
    out
}

/// `list`'s buffer becomes the next take_ints' spare (Rust only).
#[ranger::target(rust)]
pub fn give_ints(spare: &mut Vec<int>, mut list: Vec<int>) {
    list.clear();
    *spare = list;
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn give_ints(_spare: &mut Vec<int>, _list: Vec<int>) {}

/// A vector for a native call's arguments, from the pool (Rust only; the
/// other targets allocate one).
#[ranger::target(rust)]
pub fn take_args(pool: &mut Vec<Vec<Val>>) -> Vec<Val> {
    match pool.pop() {
        Some(v) => v,
        None => Vec::new(),
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn take_args(_pool: &mut Vec<Vec<Val>>) -> Vec<Val> {
    Vec::new()
}

#[ranger::target(rust)]
pub fn give_args(pool: &mut Vec<Vec<Val>>, mut args: Vec<Val>) {
    args.clear();
    pool.push(args);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn give_args(_pool: &mut Vec<Vec<Val>>, _args: Vec<Val>) {}

#[ranger::target(rust)]
pub fn grow_objs(v: &mut Vec<JsObj>) {
    if v.len() == v.capacity() {
        // by half: a growth copies the whole vector (in WebAssembly realloc
        // has no mremap), so an eighth copied 8x the final size; the spare
        // capacity is never touched natively and costs no resident memory
        let n = v.len() / 2;
        v.reserve_exact(if n < 1024 { 1024 } else { n });
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn grow_objs(_v: &mut Vec<JsObj>) {}

#[ranger::target(rust)]
pub fn grow_evecs(v: &mut Vec<Elems>) {
    if v.len() == v.capacity() {
        // by half: a growth copies the whole vector (in WebAssembly realloc
        // has no mremap), so an eighth copied 8x the final size; the spare
        // capacity is never touched natively and costs no resident memory
        let n = v.len() / 2;
        v.reserve_exact(if n < 1024 { 1024 } else { n });
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn grow_evecs(_v: &mut Vec<Elems>) {}

/// Rust: a compiled function's code and constants to their size (a
/// compiler's buffers doubled as they grew).
#[ranger::target(rust)]
pub fn trim_proto(p: &mut Proto) {
    p.code.shrink_to_fit();
    p.consts.shrink_to_fit();
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn trim_proto(_p: &mut Proto) {}

/// A free slot's buffers go (a collection: the slot may wait long for its
/// next object).
pub fn drop_bufs(o: &mut JsObj, elems: &mut Vec<Val>) {
    if !o.vals.is_empty() || !elems.is_empty() {
        o.vals = Vec::new();
        elems.truncate(0);
    }
    drop_caps(o, elems);
}

/// A freed object's elements: small vectors stay allocated for the next
/// object that takes them.
pub fn clear_elems(elems: &mut Vec<Val>) {
    if elems.len() <= 16 {
        elems.truncate(0);
    } else {
        *elems = Vec::new();
    }
}

/// Rust: a cleared Vec keeps its capacity; past KEEP_CAP it goes too.
/// Keeping 4 saved Chart.js 1 % of its instructions but cost earley-boyer
/// 12 MB (59 -> 71 MB), so a collection frees them all.
#[ranger::target(rust)]
pub fn drop_caps(o: &mut JsObj, elems: &mut Vec<Val>) {
    if o.vals.capacity() > KEEP_CAP {
        o.vals = Vec::new();
    }
    if elems.capacity() > KEEP_CAP {
        *elems = Vec::new();
    }
}

/// Rust: a full `v` grows to room for `most` values (up to what doubling
/// gives) instead of doubling: an object on a chain of shapes most often
/// gets as many properties as the most the chain has seen.
#[ranger::target(rust)]
pub fn grow_vals(v: &mut Vec<Val>, most: usize) {
    let n = v.len();
    if n == v.capacity() && most > n + 1 {
        // at most what doubling would give (16 for a new object): a
        // branch off a long chain does not take the chain's room
        let lim = if n == 0 { 16 } else { 2 * n + 4 };
        let cap = if most < lim { most } else { lim };
        v.reserve_exact(cap - n);
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn grow_vals(_v: &mut Vec<Val>, _most: usize) {}

/// A freed Cold as new (Rust: emptied in place, small buffers kept and no
/// new hash state; a map's table goes).
#[ranger::target(rust)]
pub fn reset_cold(colds: &mut Vec<Cold>, i: usize) {
    let c = &mut colds[i];
    crate::value::drop_indexes(c);
    if c.marks.capacity() > 16 {
        c.marks = Vec::new();
    } else {
        c.marks.clear();
    }
    if c.nums.capacity() > 16 {
        c.nums = Vec::new();
    } else {
        c.nums.clear();
    }
    if c.elems2.capacity() > 16 {
        c.elems2 = Vec::new();
    } else {
        c.elems2.clear();
    }
    c.prim = Val::Undef;
    c.home = -1;
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn reset_cold(colds: &mut Vec<Cold>, i: usize) {
    colds[i] = Cold::new();
}

/// The stack grows to `n` values, undefined.
#[ranger::target(rust)]
pub fn grow_stack(v: &mut Vec<Val>, n: usize) {
    v.resize(n, Val::Undef);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn grow_stack(v: &mut Vec<Val>, n: usize) {
    while v.len() < n {
        v.push(Val::Undef);
    }
}

/// Room for `n` more values (Rust; the other targets grow as they push).
#[ranger::target(rust)]
pub fn reserve_vals(v: &mut Vec<Val>, n: usize) {
    v.reserve_exact(n);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn reserve_vals(_v: &mut Vec<Val>, _n: usize) {}

/// The capacity a free slot's buffers keep through a collection (0: none).
pub const KEEP_CAP: usize = 0;

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn drop_caps(_o: &mut JsObj, _elems: &mut Vec<Val>) {}

/// Rust: a live object's buffers shrink to what they hold, when a quarter
/// or more is spare (Vm::collect); the other targets size their arrays.
#[ranger::target(rust)]
pub fn trim_obj(o: &mut JsObj, elems: &mut Vec<Val>) {
    if o.vals.capacity() > o.vals.len() + o.vals.len() / 4 {
        o.vals.shrink_to_fit();
    }
    if elems.capacity() > elems.len() + elems.len() / 4 {
        elems.shrink_to_fit();
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn trim_obj(_o: &mut JsObj, _elems: &mut Vec<Val>) {}

/// Appends `b` to the shared string `rc` when nothing else holds it, so
/// `s += …` does not copy `s` (Vm::str_append); false (and nothing changed)
/// when it cannot.
#[ranger::target(rust)]
pub fn rc_push(rc: &mut Rc<String>, b: &str) -> bool {
    if let Some(s) = Rc::get_mut(rc) {
        s.push_str(b);
        return true;
    }
    false
}

/// A freed string slot lets its text go. Rust: the box stays when nothing
/// else holds it, for the next string of the slot (rc_refill); the other
/// targets point the slot at the empty string.
#[ranger::target(rust)]
pub fn rc_vacate(rc: &mut Rc<String>, empty: &Rc<String>) {
    if let Some(s) = Rc::get_mut(rc) {
        // a small buffer stays too (rc_take_buf)
        if s.capacity() <= 64 {
            s.clear();
        } else {
            *s = String::new();
        }
        return;
    }
    *rc = empty.clone();
}

/// The buffer a reused slot kept, empty, for its new text (Rust; the
/// other targets start a new one).
#[ranger::target(rust)]
pub fn rc_take_buf(rc: &mut Rc<String>, n: usize) -> String {
    if let Some(s) = Rc::get_mut(rc) {
        let mut b = std::mem::take(s);
        b.clear();
        b.reserve(n);
        return b;
    }
    String::with_capacity(n)
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn rc_take_buf(_rc: &mut Rc<String>, _n: usize) -> String {
    String::new()
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn rc_vacate(rc: &mut Rc<String>, empty: &Rc<String>) {
    *rc = empty.clone();
}

/// Slot `rc` takes the text `s`: into its own box when nothing else
/// holds that (Rust), else a new one.
#[ranger::target(rust)]
pub fn rc_refill(rc: &mut Rc<String>, s: String) {
    if let Some(m) = Rc::get_mut(rc) {
        *m = s;
        return;
    }
    *rc = Rc::new(s);
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn rc_refill(rc: &mut Rc<String>, s: String) {
    *rc = Rc::new(s);
}

/// Slot `rc` takes a copy of `s`: into the buffer it kept (rc_vacate) when
/// nothing else holds it (Rust), else a new one.
#[ranger::target(rust)]
pub fn rc_fill_str(rc: &mut Rc<String>, s: &str) {
    if let Some(m) = Rc::get_mut(rc) {
        m.clear();
        m.push_str(s);
        return;
    }
    *rc = Rc::new(String::from(s));
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn rc_fill_str(rc: &mut Rc<String>, s: &str) {
    *rc = Rc::new(String::from(s));
}

/// Rust: a string's buffer to its size when a quarter or more is spare
/// and nothing else holds it (Vm::collect; appending in place doubles it).
#[ranger::target(rust)]
pub fn rc_trim(rc: &mut Rc<String>) {
    if let Some(s) = Rc::get_mut(rc) {
        let n = s.len();
        if n >= 256 && s.capacity() > n + n / 4 {
            s.shrink_to_fit();
        }
    }
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn rc_trim(_rc: &mut Rc<String>) {}

/// The other targets' strings are immutable values (and their engines
/// concatenate without copying where they can): always the ordinary way,
/// so no handle there is ever a prefix.
#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn rc_push(_rc: &mut Rc<String>, _b: &str) -> bool {
    false
}

/// `b` written over the bytes at `at` of the buffer (room of ASCII spaces
/// before a front view's text, Vm::str_prepend) when nothing else holds
/// it: whole characters over whole characters, so the text stays UTF-8.
#[ranger::target(rust)]
pub fn rc_front_write(rc: &mut Rc<String>, at: int, b: &str) -> bool {
    if let Some(s) = Rc::get_mut(rc) {
        let a = at as usize;
        let n = b.len();
        if a + n > s.len() || !s.as_bytes()[a..a + n].iter().all(|c| *c == b' ') {
            return false;
        }
        // SAFETY: bytes a..a+n are ASCII spaces (each a whole character);
        // b is UTF-8, so the buffer stays UTF-8
        unsafe {
            s.as_bytes_mut()[a..a + n].copy_from_slice(b.as_bytes());
        }
        return true;
    }
    false
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn rc_front_write(_rc: &mut Rc<String>, _at: int, _b: &str) -> bool {
    false
}

/// Whether str_prepend's front-room buffers are used (Rust: their room is
/// written in place; the other targets' strings are immutable values).
#[ranger::target(rust)]
pub fn prepend_ok() -> bool {
    true
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn prepend_ok() -> bool {
    false
}

/// Back jumps between two reads of the clock while slices are on.
pub const SLICE_TICKS: int = 1000;


/// One string handle of the table (`Vm.strs`): its text and what the
/// table knows of it, together, so a handle's checks touch one line.
pub struct SEnt {
    pub s: Rc<String>,
    /// >= 0 when an append in place moved the buffer on to that newer
    /// handle (this one is then its first `len` bytes)
    pub up: i32,
    /// with up >= 0: the text is `len` bytes from byte `off` of that
    /// handle's buffer (off 0 for a prefix; a front view's start, see
    /// str_prepend); a front-room buffer's own handle keeps its text's
    /// start in `len`
    pub len: i32,
    pub off: i32,
    /// the text's length in UTF-16 units, -1 until asked (Vm::hlen); an
    /// append in place carries it on, so `s += x; s.length` is not a scan
    pub ulen: i32,
    /// 1 pinned (constants, atoms), 2 free
    pub flag: i32,
    /// the heap's references to the handle (object slots and the prefixes
    /// whose text lies in it), as JsObj.rc is for objects: the stack,
    /// frames and other roots are not counted
    pub rc: i32,
    /// the atom the text interns to, -1 before it is asked (to_key: a
    /// string key looked up once per string, not per use)
    pub atom: i32,
    /// marked by collect() (mark_strs)
    pub mark: bool,
    /// in szct
    pub inz: bool,
}

impl SEnt {
    pub fn new(s: Rc<String>, flag: i32) -> SEnt {
        SEnt { s: s, up: -1, len: 0, off: 0, ulen: -1, flag: flag, rc: 0, atom: -1, mark: false, inz: false }
    }
}

/// A counted reference more to each object and string in `e` (Vm.inc
/// over a whole vector, the tables borrowed once).
pub fn inc_refs(objs: &mut Vec<JsObj>, strs: &mut Vec<SEnt>, e: &Elems) {
    let n = e.v.len();
    let mut i: usize = 0;
    while i < n {
        match e.v[i] {
            Val::Obj(o) => {
                objs[o as usize].rc += 1;
            }
            Val::Str(h) => {
                if h > 0 {
                    strs[h as usize].rc += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// The handles' counts (a check of them, rc_check).
pub fn srcs_of(v: &Vec<SEnt>) -> Vec<i32> {
    let mut out: Vec<i32> = Vec::with_capacity(v.len());
    for e in v.iter() {
        out.push(e.rc);
    }
    out
}

/// Room for one more property value in `ob` (slots 2 on are in vals),
/// for a chain whose objects get `most` properties.
pub fn slot_room(ob: &mut JsObj, most: usize) {
    if ob.nv >= 2 && crate::rvm::vals_full(&ob.vals) {
        grow_vals(&mut ob.vals, if most > 2 { most - 2 } else { 0 });
    }
}

/// Whether `s` is an array index's canonical text (0 .. 2^32 - 2): such
/// keys come first in own-key order, ascending.
pub fn is_index_key(s: &str) -> bool {
    let n = s.as_bytes().len();
    if n == 0 || n > 10 {
        return false;
    }
    let mut first = true;
    for x in s.bytes() {
        if x < 48 || x > 57 || (first && x == 48 && n > 1) {
            return false;
        }
        first = false;
    }
    n < 10 || index_value(s) <= 4294967294.0
}

/// The value of a canonical integer key's text (all its digits).
pub fn index_value(s: &str) -> double {
    let mut v: double = 0.0;
    for x in s.bytes() {
        v = v * 10.0 + ((x as int) - 48) as double;
    }
    v
}

/// `ids` ordered by `keys` (keys[i] belongs to ids[i]): a stable merge sort.
pub fn sort_ids_by_keys(ids: &mut Vec<int>, keys: &Vec<double>) {
    let n = ids.len();
    let mut kv: Vec<double> = keys.clone();
    let mut tk: Vec<double> = keys.clone();
    let mut ti: Vec<int> = ids.clone();
    let mut w: usize = 1;
    while w < n {
        let mut lo: usize = 0;
        while lo < n {
            let mid = if lo + w < n { lo + w } else { n };
            let hi = if lo + 2 * w < n { lo + 2 * w } else { n };
            let mut i = lo;
            let mut j = mid;
            let mut k = lo;
            while k < hi {
                if i < mid && (j >= hi || kv[i] <= kv[j]) {
                    tk[k] = kv[i];
                    ti[k] = ids[i];
                    i += 1;
                } else {
                    tk[k] = kv[j];
                    ti[k] = ids[j];
                    j += 1;
                }
                k += 1;
            }
            lo = hi;
        }
        let mut q: usize = 0;
        while q < n {
            kv[q] = tk[q];
            ids[q] = ti[q];
            q += 1;
        }
        w = w * 2;
    }
}

/// FNV-1a over the bytes, 32 bits: intern's table hash.
pub fn str_hash(s: &str) -> int {
    let mut h: int = 2166136261;
    for x in s.bytes() {
        h = ((h ^ (x as int)) * 16777619) & 4294967295;
    }
    h
}

/// Whether native_fast has a path for native `id`: the others are rejected
/// at once (its checks one by one cost a native call ~150 instructions).
/// A new path in native_fast needs its id here.
pub fn native_fast_id(id: int) -> bool {
    let m = id - crate::builtins::NF_MATH;
    (m >= 0 && m <= 32)
        || (id >= crate::builtins::NF_N_ISINTEGER && id <= crate::builtins::NF_N_ISNAN)
        || id == crate::typed::NF_DVM_GET
        || id == crate::typed::NF_DVM_SET
        || id == crate::typed::NF_FAST_WRAP
        || id == crate::builtins::NF_ISNAN
        || id == crate::builtins::NF_ISFINITE
        || id == crate::builtins::NF_STRING
        || id == crate::builtins::NF_SP_SLICE
        || id == crate::builtins::NF_SP_SUBSTRING
        || id == crate::builtins::NF_SP_CHARCODEAT
        || id == crate::builtins::NF_NP_TOSTRING
        || id == crate::builtins::NF_OP_HASOWN
        || id == crate::builtins::NF_AP_PUSH
        || id == crate::builtins::NF_MP_GET
        || id == crate::builtins::NF_MP_HAS
        || id == crate::builtins::NF_MP_SET
        || id == crate::builtins::NF_SP_HAS
        || id == crate::builtins::NF_SETP_ADD
        || id == crate::builtins::NF_WM_GET
        || id == crate::builtins::NF_WM_HAS
        || id == crate::builtins::NF_WM_SET
        || id == crate::builtins::NF_WS_HAS
}

/// A bound native_fast's slice/substring takes: undefined or a whole
/// number (the rest is the builtin's coercion).
pub fn slice_bound_ok(v: &Val) -> bool {
    match v {
        Val::Undef => true,
        Val::Num(x) => x.abs() < 1e9 && *x == ((*x as int) as double),
        _ => false,
    }
}

/// Bound `v` (undefined: dflt) clamped to [0, len]; slice counts a
/// negative one from the end, substring takes it as 0.
pub fn slice_at(v: &Val, dflt: int, len: int, slice: bool) -> int {
    let n = match v {
        Val::Num(x) => *x as int,
        _ => dflt,
    };
    if slice && n < 0 {
        return if len + n < 0 { 0 } else { len + n };
    }
    if n < 0 {
        return 0;
    }
    if n > len {
        return len;
    }
    n
}

/// JSON.parse over the text's bytes (Rust): no char vector is made, a
/// string with no escapes is one copy of its slice and a key is interned
/// from its slice. True when it parsed the text (vm.throwing on a syntax
/// error, else `out` holds the value); the other targets return false and
/// the caller parses chars (Vm::json_parse_value).
#[ranger::target(rust)]
pub fn json_parse_fast(vm: &mut Vm, s: &str, out: &mut Val) -> bool {
    let b = s.as_bytes();
    let mut pos: usize = 0;
    let mut buf = String::new();
    let mut memo: Vec<int> = vec![-1; 128];
    let v = jf_value(vm, s, b, &mut pos, &mut buf, &mut memo);
    if vm.throwing {
        return true;
    }
    jf_ws(b, &mut pos);
    if pos < b.len() {
        vm.throw_syntax("Unexpected non-whitespace character after JSON");
        return true;
    }
    *out = v;
    true
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn json_parse_fast(_vm: &mut Vm, _s: &str, _out: &mut Val) -> bool {
    false
}

#[ranger::target(rust)]
#[inline(always)]
fn jf_ws(b: &[u8], pos: &mut usize) {
    while *pos < b.len() {
        let c = b[*pos];
        if c == b' ' || c == b'\n' || c == b'\r' || c == b'\t' {
            *pos += 1;
        } else {
            break;
        }
    }
}

/// The JSON string at `pos` (its opening quote): (start, end, escaped).
/// Not escaped: the text is s[start..end]; escaped: it is in `buf`.
#[ranger::target(rust)]
fn jf_string(vm: &mut Vm, s: &str, b: &[u8], pos: &mut usize, buf: &mut String) -> (usize, usize, bool) {
    *pos += 1;
    let start = *pos;
    let n = b.len();
    let mut i = start;
    while i < n {
        let c = b[i];
        if c == b'"' {
            *pos = i + 1;
            return (start, i, false);
        }
        if c == b'\\' {
            break;
        }
        if c < 32 {
            vm.throw_syntax("Bad control character in string literal in JSON");
            return (start, i, false);
        }
        i += 1;
    }
    if i >= n {
        vm.throw_syntax("Unterminated string in JSON");
        return (start, i, false);
    }
    // escapes: the text so far, then decoded runs into buf
    buf.clear();
    buf.push_str(&s[start..i]);
    let mut run = i;
    while i < n {
        let c = b[i];
        if c == b'"' {
            buf.push_str(&s[run..i]);
            *pos = i + 1;
            return (start, i, true);
        }
        if c < 32 {
            vm.throw_syntax("Bad control character in string literal in JSON");
            return (start, i, true);
        }
        if c != b'\\' {
            i += 1;
            continue;
        }
        buf.push_str(&s[run..i]);
        i += 1;
        if i >= n {
            break;
        }
        let e = b[i];
        i += 1;
        match e {
            b'n' => buf.push('\n'),
            b't' => buf.push('\t'),
            b'r' => buf.push('\r'),
            b'b' => buf.push('\u{8}'),
            b'f' => buf.push('\u{c}'),
            b'/' => buf.push('/'),
            b'\\' => buf.push('\\'),
            b'"' => buf.push('"'),
            b'u' => {
                let v0 = jf_hex4(b, i);
                if v0 < 0 {
                    vm.throw_syntax("Bad Unicode escape in JSON");
                    return (start, i, true);
                }
                let mut v = v0;
                i += 4;
                // a surrogate pair written as two escapes
                if v >= 0xd800 && v <= 0xdbff && i + 5 < n && b[i] == b'\\' && b[i + 1] == b'u' {
                    let lo = jf_hex4(b, i + 2);
                    if lo >= 0xdc00 && lo <= 0xdfff {
                        i += 6;
                        v = 0x10000 + ((v - 0xd800) << 10) + (lo - 0xdc00);
                    }
                }
                jsstr::push_cp(buf, v);
            }
            _ => {
                vm.throw_syntax("Bad escaped character in JSON");
                return (start, i, true);
            }
        }
        run = i;
    }
    vm.throw_syntax("Unterminated string in JSON");
    (start, i, true)
}

/// The text is a JSON number: -?(0|[1-9]d*)(.d+)?([eE][+-]?d+)?
#[ranger::target(rust)]
fn jf_num_ok(t: &[u8]) -> bool {
    let n = t.len();
    let mut i = 0;
    if i < n && t[i] == b'-' {
        i += 1;
    }
    let d = i;
    while i < n && t[i].is_ascii_digit() {
        i += 1;
    }
    if i == d {
        return false;
    }
    if i < n && t[i] == b'.' {
        i += 1;
        let f = i;
        while i < n && t[i].is_ascii_digit() {
            i += 1;
        }
        if i == f {
            return false;
        }
    }
    if i < n && (t[i] == b'e' || t[i] == b'E') {
        i += 1;
        if i < n && (t[i] == b'+' || t[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < n && t[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return false;
        }
    }
    i == n
}

/// Four hex digits at b[i..], -1 when not.
#[ranger::target(rust)]
fn jf_hex4(b: &[u8], i: usize) -> int {
    if i + 4 > b.len() {
        return -1;
    }
    let mut v: int = 0;
    for k in 0..4 {
        let h = b[i + k];
        let d = if h >= b'0' && h <= b'9' {
            (h - b'0') as int
        } else if h >= b'a' && h <= b'f' {
            (h - b'a') as int + 10
        } else if h >= b'A' && h <= b'F' {
            (h - b'A') as int + 10
        } else {
            return -1;
        };
        v = v * 16 + d;
    }
    v
}

/// A string value from a slice, into a reclaimed handle's own buffer
/// when it has one.
#[ranger::target(rust)]
fn jf_str_val(vm: &mut Vm, t: &str) -> Val {
    if t.is_empty() {
        return Val::Str(0);
    }
    if t.len() == 1 {
        return vm.unit_val(t.as_bytes()[0] as int);
    }
    let h = vm.take_handle(t.len() as int, 0);
    let rc = &mut vm.strs[h as usize].s;
    if let Some(m) = Rc::get_mut(rc) {
        m.clear();
        m.push_str(t);
    } else {
        *rc = Rc::new(String::from(t));
    }
    Val::Str(h)
}

#[ranger::target(rust)]
fn jf_value(vm: &mut Vm, s: &str, b: &[u8], pos: &mut usize, buf: &mut String, memo: &mut Vec<int>) -> Val {
    jf_ws(b, pos);
    let n = b.len();
    if *pos >= n {
        vm.throw_syntax("Unexpected end of JSON input");
        return Val::Undef;
    }
    let c = b[*pos];
    if c == b'{' {
        *pos += 1;
        let o = vm.new_object();
        vm.temp_roots.push(Val::Obj(o));
        jf_ws(b, pos);
        if *pos < n && b[*pos] == b'}' {
            *pos += 1;
            vm.temp_roots.pop();
            return Val::Obj(o);
        }
        loop {
            jf_ws(b, pos);
            if *pos >= n || b[*pos] != b'"' {
                vm.throw_syntax("Unexpected token in JSON");
                vm.temp_roots.pop();
                return Val::Undef;
            }
            let (ka, kb, kesc) = jf_string(vm, s, b, pos, buf);
            if vm.throwing {
                vm.temp_roots.pop();
                return Val::Undef;
            }
            // the key straight to its atom: no string value in between. The
            // key a shared shape last gained in this text (objects of one
            // shape in a row; `memo`, direct-mapped by shape: shape, atom)
            // is that atom, and new to o.
            let sh0 = vm.objs[o as usize].shape as int;
            let mi = ((sh0 & 63) * 2) as usize;
            let mut fresh = false;
            let atom = {
                let k: &str = if kesc { buf.as_str() } else { &s[ka..kb] };
                if memo[mi] == sh0 && vm.atom_names[memo[mi + 1] as usize].as_str() == k {
                    fresh = true;
                    memo[mi + 1]
                } else {
                    let ki = jsstr::array_index(k);
                    if ki >= 0 {
                        vm.index_atom(ki)
                    } else {
                        vm.intern(k)
                    }
                }
            };
            jf_ws(b, pos);
            if *pos >= n || b[*pos] != b':' {
                vm.throw_syntax("Expected ':' in JSON");
                vm.temp_roots.pop();
                return Val::Undef;
            }
            *pos += 1;
            let v = jf_value(vm, s, b, pos, buf, memo);
            if vm.throwing {
                vm.temp_roots.pop();
                return Val::Undef;
            }
            if fresh {
                vm.add_own(o, atom, v, 0);
            } else {
                vm.define(o, atom, v, 0);
                // a key new to a shared shape (o was not given an owned one)
                let sh1 = vm.objs[o as usize].shape as int;
                if sh1 != sh0 && !vm.shapes[sh0 as usize].owned && !vm.shapes[sh1 as usize].owned {
                    memo[mi] = sh0;
                    memo[mi + 1] = atom;
                }
            }
            jf_ws(b, pos);
            if *pos < n && b[*pos] == b',' {
                *pos += 1;
                continue;
            }
            if *pos < n && b[*pos] == b'}' {
                *pos += 1;
                break;
            }
            vm.throw_syntax("Unexpected token in JSON");
            vm.temp_roots.pop();
            return Val::Undef;
        }
        vm.temp_roots.pop();
        return Val::Obj(o);
    }
    if c == b'[' {
        *pos += 1;
        let a = vm.new_array(Vec::new());
        vm.temp_roots.push(Val::Obj(a));
        jf_ws(b, pos);
        if *pos < n && b[*pos] == b']' {
            *pos += 1;
            vm.temp_roots.pop();
            return Val::Obj(a);
        }
        loop {
            let v = jf_value(vm, s, b, pos, buf, memo);
            if vm.throwing {
                vm.temp_roots.pop();
                return Val::Undef;
            }
            vm.push_elem(a, v);
            jf_ws(b, pos);
            if *pos < n && b[*pos] == b',' {
                *pos += 1;
                continue;
            }
            if *pos < n && b[*pos] == b']' {
                *pos += 1;
                break;
            }
            vm.throw_syntax("Unexpected token in JSON");
            vm.temp_roots.pop();
            return Val::Undef;
        }
        vm.temp_roots.pop();
        return Val::Obj(a);
    }
    if c == b'"' {
        let (a, e, esc) = jf_string(vm, s, b, pos, buf);
        if vm.throwing {
            return Val::Undef;
        }
        return if esc { jf_str_val(vm, buf.as_str()) } else { jf_str_val(vm, &s[a..e]) };
    }
    if c == b't' && b[*pos..].starts_with(b"true") {
        *pos += 4;
        return Val::Bool(true);
    }
    if c == b'f' && b[*pos..].starts_with(b"false") {
        *pos += 5;
        return Val::Bool(false);
    }
    if c == b'n' && b[*pos..].starts_with(b"null") {
        *pos += 4;
        return Val::Null;
    }
    if c == b'-' || (c >= b'0' && c <= b'9') {
        let start = *pos;
        if c == b'-' {
            *pos += 1;
        }
        if *pos + 1 < n && b[*pos] == b'0' && b[*pos + 1].is_ascii_digit() {
            vm.throw_syntax("Unexpected number in JSON");
            return Val::Undef;
        }
        // an integer of up to 15 digits is exact as it is summed
        let d0 = *pos;
        let mut iv: double = 0.0;
        while *pos < n && b[*pos].is_ascii_digit() {
            iv = iv * 10.0 + ((b[*pos] - b'0') as double);
            *pos += 1;
        }
        let nd = *pos - d0;
        let more = |p: usize| p < n && matches!(b[p], b'.' | b'e' | b'E' | b'+' | b'-');
        if nd > 0 && nd <= 15 && !more(*pos) {
            return Val::Num(if c == b'-' { -iv } else { iv });
        }
        // d.ddd with at most 15 digits in all and no exponent: the integer
        // over a power of ten, both exact, is the correctly rounded value
        if nd > 0 && nd < 15 && *pos < n && b[*pos] == b'.' {
            let f0 = *pos + 1;
            let mut p = f0;
            let mut fv = iv;
            while p < n && b[p].is_ascii_digit() && p - f0 + nd < 15 {
                fv = fv * 10.0 + ((b[p] - b'0') as double);
                p += 1;
            }
            let nf = p - f0;
            if nf > 0 && !(p < n && (b[p].is_ascii_digit() || b[p] == b'e' || b[p] == b'E' || b[p] == b'.' || b[p] == b'+' || b[p] == b'-')) {
                const P10: [double; 16] = [1.0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15];
                *pos = p;
                let r = fv / P10[nf];
                return Val::Num(if c == b'-' { -r } else { r });
            }
        }
        while *pos < n && (b[*pos].is_ascii_digit() || more(*pos)) {
            *pos += 1;
        }
        let v = if jf_num_ok(&b[start..*pos]) { string_to_number(&s[start..*pos]) } else { nan() };
        if is_nan(v) {
            vm.throw_syntax("Unexpected number in JSON");
            return Val::Undef;
        }
        return Val::Num(v);
    }
    let ch = s[*pos..].chars().next().unwrap_or(' ');
    vm.throw_syntax(format!("Unexpected token '{}' in JSON", ch).as_str());
    Val::Undef
}

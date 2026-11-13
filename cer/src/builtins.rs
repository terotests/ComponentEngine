// SPDX-License-Identifier: AGPL-3.0-or-later
//! The global object and the built-in constructors and prototypes. A
//! built-in function is an object of class C_NATIVE whose `func` is one of
//! the NF_ ids below; `call_native` runs it.

use ranger::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;

use crate::jsstr;
use crate::num::*;
use crate::regex;
use crate::value::*;
use crate::vm::*;

// Function.prototype
pub const NF_CALL: int = 1;
pub const NF_APPLY: int = 2;
pub const NF_BIND: int = 3;
pub const NF_FN_TOSTRING: int = 4;
pub const NF_FUNCTION: int = 5;
pub const NF_NOOP: int = 6;
// globals
pub const NF_PRINT: int = 10;
pub const NF_PARSEINT: int = 11;
pub const NF_PARSEFLOAT: int = 12;
pub const NF_ISNAN: int = 13;
pub const NF_ISFINITE: int = 14;
pub const NF_STRING: int = 15;
pub const NF_NUMBER: int = 16;
pub const NF_BOOLEAN: int = 17;
pub const NF_OBJECT: int = 18;
pub const NF_ARRAY: int = 19;
pub const NF_ERROR: int = 20; // + kind 0..6
pub const NF_SYMBOL: int = 30;
pub const NF_REGEXP: int = 31;
pub const NF_DATE: int = 32;
pub const NF_MAP: int = 33;
pub const NF_SET: int = 34;
pub const NF_PERF_NOW: int = 35;
pub const NF_ENCODE_URI_COMPONENT: int = 36;
pub const NF_DECODE_URI_COMPONENT: int = 37;
pub const NF_ENCODE_URI: int = 38;
pub const NF_DECODE_URI: int = 39;
pub const NF_PROMISE: int = 40;
pub const NF_WEAKMAP: int = 41;
pub const NF_WEAKSET: int = 42;
pub const NF_GC: int = 43;
pub const NF_ESCAPE: int = 44;
pub const NF_UNESCAPE: int = 45;
// Object
pub const NF_O_KEYS: int = 50;
pub const NF_O_VALUES: int = 51;
pub const NF_O_ENTRIES: int = 52;
pub const NF_O_ASSIGN: int = 53;
pub const NF_O_CREATE: int = 54;
pub const NF_O_GETPROTO: int = 55;
pub const NF_O_SETPROTO: int = 56;
pub const NF_O_DEFPROP: int = 57;
pub const NF_O_DEFPROPS: int = 58;
pub const NF_O_OWNNAMES: int = 59;
pub const NF_O_OWNDESC: int = 60;
pub const NF_O_FREEZE: int = 61;
pub const NF_O_ISFROZEN: int = 62;
pub const NF_O_SEAL: int = 63;
pub const NF_O_ISSEALED: int = 64;
pub const NF_O_PREVENTEXT: int = 65;
pub const NF_O_ISEXT: int = 66;
pub const NF_O_IS: int = 67;
pub const NF_O_FROMENTRIES: int = 68;
pub const NF_O_OWNSYMBOLS: int = 69;
pub const NF_O_OWNDESCS: int = 70;
pub const NF_OP_HASOWN: int = 71;
pub const NF_OP_ISPROTO: int = 72;
pub const NF_OP_PROPENUM: int = 73;
pub const NF_OP_TOSTRING: int = 74;
pub const NF_OP_VALUEOF: int = 75;
pub const NF_OP_TOLOCALE: int = 76;
pub const NF_O_HASOWN: int = 77;
pub const NF_O_GROUPBY: int = 78;
// Array
pub const NF_A_ISARRAY: int = 80;
pub const NF_A_FROM: int = 81;
pub const NF_A_OF: int = 82;
pub const NF_AP_PUSH: int = 83;
pub const NF_AP_POP: int = 84;
pub const NF_AP_SHIFT: int = 85;
pub const NF_AP_UNSHIFT: int = 86;
pub const NF_AP_SLICE: int = 87;
pub const NF_AP_SPLICE: int = 88;
pub const NF_AP_CONCAT: int = 89;
pub const NF_AP_JOIN: int = 90;
pub const NF_AP_REVERSE: int = 91;
pub const NF_AP_INDEXOF: int = 92;
pub const NF_AP_LASTINDEXOF: int = 93;
pub const NF_AP_INCLUDES: int = 94;
pub const NF_AP_FOREACH: int = 95;
pub const NF_AP_MAP: int = 96;
pub const NF_AP_FILTER: int = 97;
pub const NF_AP_REDUCE: int = 98;
pub const NF_AP_REDUCERIGHT: int = 99;
pub const NF_AP_SOME: int = 100;
pub const NF_AP_EVERY: int = 101;
pub const NF_AP_FIND: int = 102;
pub const NF_AP_FINDINDEX: int = 103;
pub const NF_AP_FINDLAST: int = 104;
pub const NF_AP_FINDLASTINDEX: int = 105;
pub const NF_AP_SORT: int = 106;
pub const NF_AP_FILL: int = 107;
pub const NF_AP_KEYS: int = 108;
pub const NF_AP_VALUES: int = 109;
pub const NF_AP_ENTRIES: int = 110;
pub const NF_AP_FLAT: int = 111;
pub const NF_AP_FLATMAP: int = 112;
pub const NF_AP_TOSTRING: int = 113;
pub const NF_AP_AT: int = 114;
pub const NF_AP_COPYWITHIN: int = 115;
pub const NF_AP_TOREVERSED: int = 116;
pub const NF_AP_TOSORTED: int = 117;
pub const NF_AP_WITH: int = 118;
pub const NF_ITER_NEXT: int = 119;
pub const NF_ITER_SELF: int = 120;
// String
pub const NF_S_FROMCHARCODE: int = 130;
pub const NF_S_FROMCODEPOINT: int = 131;
pub const NF_S_RAW: int = 132;
pub const NF_SP_CHARAT: int = 133;
pub const NF_SP_CHARCODEAT: int = 134;
pub const NF_SP_CODEPOINTAT: int = 135;
pub const NF_SP_INDEXOF: int = 136;
pub const NF_SP_LASTINDEXOF: int = 137;
pub const NF_SP_INCLUDES: int = 138;
pub const NF_SP_STARTSWITH: int = 139;
pub const NF_SP_ENDSWITH: int = 140;
pub const NF_SP_SLICE: int = 141;
pub const NF_SP_SUBSTRING: int = 142;
pub const NF_SP_SUBSTR: int = 143;
pub const NF_SP_UPPER: int = 144;
pub const NF_SP_LOWER: int = 145;
pub const NF_SP_TRIM: int = 146;
pub const NF_SP_TRIMSTART: int = 147;
pub const NF_SP_TRIMEND: int = 148;
pub const NF_SP_SPLIT: int = 149;
pub const NF_SP_REPLACE: int = 150;
pub const NF_SP_REPLACEALL: int = 151;
pub const NF_SP_MATCH: int = 152;
pub const NF_SP_MATCHALL: int = 153;
pub const NF_SP_SEARCH: int = 154;
pub const NF_SP_REPEAT: int = 155;
pub const NF_SP_PADSTART: int = 156;
pub const NF_SP_PADEND: int = 157;
pub const NF_SP_CONCAT: int = 158;
pub const NF_SP_AT: int = 159;
pub const NF_SP_LOCALECOMPARE: int = 160;
pub const NF_SP_TOSTRING: int = 161;
pub const NF_SP_NORMALIZE: int = 162;
pub const NF_SP_ITERATOR: int = 163;
pub const NF_SP_ISWELLFORMED: int = 164;
pub const NF_SP_TOWELLFORMED: int = 165;
// Number / Boolean / Symbol / Error
pub const NF_N_ISINTEGER: int = 170;
pub const NF_N_ISSAFEINTEGER: int = 171;
pub const NF_N_ISFINITE: int = 172;
pub const NF_N_ISNAN: int = 173;
pub const NF_NP_TOSTRING: int = 174;
pub const NF_NP_TOFIXED: int = 175;
pub const NF_NP_VALUEOF: int = 176;
pub const NF_NP_TOPRECISION: int = 177;
pub const NF_NP_TOEXPONENTIAL: int = 178;
pub const NF_BP_TOSTRING: int = 179;
pub const NF_BP_VALUEOF: int = 180;
pub const NF_SYM_FOR: int = 181;
pub const NF_SYMP_TOSTRING: int = 182;
pub const NF_SYMP_DESCRIPTION: int = 183;
pub const NF_EP_TOSTRING: int = 184;
pub const NF_SYM_KEYFOR: int = 185;
// Math
pub const NF_MATH: int = 200; // + index into MATH_NAMES
/// __hostSync(text): the host's answer (Vm::set_host_sync), synchronously
pub const NF_HOST_SYNC: int = 1990;
// JSON
pub const NF_JSON_STRINGIFY: int = 250;
pub const NF_JSON_PARSE: int = 251;
// RegExp
pub const NF_RP_EXEC: int = 260;
pub const NF_RP_TEST: int = 261;
pub const NF_RP_TOSTRING: int = 262;
pub const NF_RP_FLAGS: int = 263;
pub const NF_RP_SOURCE: int = 264;
/// the flag getters hasIndices, global, ignoreCase, multiline, dotAll,
/// unicode, unicodeSets, sticky
pub const NF_RP_FLAG0: int = 1900;
// Date
pub const NF_DATE_NOW: int = 270;
pub const NF_DATE_UTC: int = 271;
pub const NF_DATE_PARSE: int = 272;
pub const NF_DP_GET: int = 280; // + field
pub const NF_DP_SET: int = 300; // + field
pub const NF_DP_TOISO: int = 320;
pub const NF_DP_TOSTRING: int = 321;
pub const NF_DP_VALUEOF: int = 322;
pub const NF_DP_TOJSON: int = 323;
// Map / Set
pub const NF_MP_GET: int = 330;
pub const NF_MP_SET: int = 331;
pub const NF_MP_HAS: int = 332;
pub const NF_MP_DELETE: int = 333;
pub const NF_MP_CLEAR: int = 334;
pub const NF_MP_SIZE: int = 335;
pub const NF_MP_FOREACH: int = 336;
pub const NF_MP_KEYS: int = 337;
pub const NF_MP_VALUES: int = 338;
pub const NF_MP_ENTRIES: int = 339;
pub const NF_SETP_ADD: int = 340;
// Set / WeakMap / WeakSet methods: own ids so each checks its receiver's
// brand, then runs as the NF_MP_* method it maps to (coll_base)
pub const NF_SP_HAS: int = 341;
pub const NF_SP_DELETE: int = 342;
pub const NF_SP_CLEAR: int = 343;
pub const NF_SP_FOREACH: int = 344;
pub const NF_SP_SIZE: int = 345;
pub const NF_SP_KEYS: int = 346;
pub const NF_SP_ENTRIES: int = 347;
pub const NF_WM_GET: int = 348;
pub const NF_WM_SET: int = 349;
pub const NF_WM_HAS: int = 359;
pub const NF_WM_DELETE: int = 360;
pub const NF_WS_ADD: int = 361;
pub const NF_WS_HAS: int = 362;
pub const NF_WS_DELETE: int = 363;
pub const NF_MP_GETORINSERT: int = 364;
pub const NF_MP_GETORINSERTC: int = 365;
pub const NF_WM_GETORINSERT: int = 366;
pub const NF_WM_GETORINSERTC: int = 367;
// Promise
pub const NF_PR_THEN: int = 350;
pub const NF_PR_CATCH: int = 351;
pub const NF_PR_RESOLVE: int = 352;
pub const NF_PR_REJECT: int = 353;
pub const NF_PR_RESOLVE_FN: int = 354;
pub const NF_PR_REJECT_FN: int = 355;
pub const NF_PR_FINALLY: int = 356;
pub const NF_PR_ALL: int = 357;
pub const NF_QUEUE_MICROTASK: int = 358;
// Reflect
pub const NF_REFLECT_APPLY: int = 370;
pub const NF_REFLECT_CONSTRUCT: int = 371;
pub const NF_REFLECT_OWNKEYS: int = 372;
pub const NF_REFLECT_HAS: int = 373;
pub const NF_REFLECT_GET: int = 374;
pub const NF_REFLECT_SET: int = 375;
pub const NF_REFLECT_GETPROTO: int = 376;
pub const NF_REFLECT_DEFPROP: int = 377;
pub const NF_REFLECT_DELETE: int = 378;

fn math_names() -> Vec<String> {
    let v = vec![
        "abs", "floor", "ceil", "round", "trunc", "sign", "sqrt", "cbrt", "exp", "expm1", "log", "log2", "log10", "log1p", "sin", "cos", "tan",
        "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "fround", "clz32", "pow", "atan2", "imul", "min", "max",
        "random", "hypot",
    ];
    let mut out: Vec<String> = Vec::new();
    for s in v {
        out.push(String::from(s));
    }
    out
}

fn date_fields() -> Vec<String> {
    let v = vec![
        "FullYear", "Month", "Date", "Day", "Hours", "Minutes", "Seconds", "Milliseconds", "Time", "TimezoneOffset", "Year",
    ];
    let mut out: Vec<String> = Vec::new();
    for s in v {
        out.push(String::from(s));
    }
    out
}

pub fn arg(args: &Vec<Val>, i: usize) -> Val {
    if i < args.len() {
        args[i].clone()
    } else {
        Val::Undef
    }
}

fn days_from_civil(y: int, m: int, d: int) -> int {
    let yy = if m <= 2 { y - 1 } else { y };
    let era = if yy >= 0 { yy } else { yy - 399 } / 400;
    let yoe = yy - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z0: int) -> (int, int, int) {
    let z = z0 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}


pub fn now_ms() -> double {
    // the prelude's clock: milliseconds since the epoch, fractional
    wall_clock_ms()
}

// The running host call's deadline (Vm::begin_call) where code with no Vm
// at hand checks it: the regex matcher, BigInt arithmetic, a number sort
// (Vm::check_expired); and the regex matcher's backtracking budget
// (Vm::set_regex_step_limit). Rust keeps them per thread (limits); the
// other targets have no limits.

/// A promise job stopped by its slice heads the queue as [JOB_CONT + kind,
/// the async call's generator, the script's value, d] (Vm::run_jobs); the
/// kinds: a reaction, an await's continuation, queueMicrotask, a thenable.
pub const JOB_CONT: double = 1000000000.0;
pub const JK_REACT: int = 1;
pub const JK_ASYNC: int = 2;
pub const JK_MICRO: int = 3;
pub const JK_THENABLE: int = 4;

/// Units of work (elements, steps, limbs) between two reads of the clock in
/// time_up: about a millisecond's worth.
pub const POLL_EVERY: int = 1048576;

/// The default backtracking budget: about 1.5 s of matching.
pub const REGEX_STEPS: int = 10000000;

pub const LIM_SET_DEADLINE: int = 0;
pub const LIM_TIME_UP: int = 1;
pub const LIM_EXPIRED: int = 2;
pub const LIM_SET_REGEX: int = 3;
pub const LIM_REGEX: int = 4;
pub const LIM_REGEX_OVER: int = 5;
pub const LIM_STOPPED: int = 6;
pub const LIM_TAKE_OVER: int = 7;

/// The per-thread limit state: op LIM_* with its argument; answers a
/// number (1 / 0 for yes / no).
#[ranger::target(rust)]
pub fn limits(op: int, x: double) -> double {
    // [deadline, units left before the clock is read, expired, regex
    // budget, regex budget ran out]
    thread_local! {
        static L: std::cell::Cell<[f64; 5]> = const { std::cell::Cell::new([0.0, 1048576.0, 0.0, 10000000.0, 0.0]) };
    }
    L.with(|c| {
        let mut v = c.get();
        let mut r = 0.0;
        match op {
            LIM_SET_DEADLINE => {
                v[0] = x;
                v[1] = POLL_EVERY as f64;
                v[2] = 0.0;
            }
            LIM_TIME_UP => {
                if v[0] <= 0.0 {
                    return 0.0;
                }
                v[1] -= x;
                if v[1] > 0.0 {
                    c.set(v);
                    return 0.0;
                }
                v[1] = POLL_EVERY as f64;
                if v[2] == 0.0 && now_ms() >= v[0] {
                    v[2] = 1.0;
                }
                r = v[2];
            }
            LIM_EXPIRED => return v[2],
            LIM_SET_REGEX => v[3] = if x > 0.0 { x } else { 9.0e18 },
            LIM_REGEX => return v[3],
            LIM_REGEX_OVER => v[4] = 1.0,
            LIM_STOPPED => return if v[4] != 0.0 || v[2] != 0.0 { 1.0 } else { 0.0 },
            LIM_TAKE_OVER => {
                r = v[4];
                v[4] = 0.0;
            }
            _ => {}
        }
        c.set(v);
        r
    })
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn limits(op: int, _x: double) -> double {
    if op == LIM_REGEX {
        return 9.0e18;
    }
    0.0
}

pub fn set_deadline(t: double) {
    limits(LIM_SET_DEADLINE, t);
}

/// `cost` more units of work were done: true once the deadline has passed
/// (and from then on until the next set_deadline). Reads the clock once in
/// POLL_EVERY units.
pub fn time_up(cost: int) -> bool {
    limits(LIM_TIME_UP, cost as double) != 0.0
}

/// time_up has seen the deadline pass.
pub fn expired() -> bool {
    limits(LIM_EXPIRED, 0.0) != 0.0
}

pub fn set_regex_limit(n: double) {
    limits(LIM_SET_REGEX, n);
}

/// A double: the C++ target's int is 32 bits.
pub fn regex_limit() -> double {
    limits(LIM_REGEX, 0.0)
}

/// A match attempt has taken `steps`: true when it is to stop, past the
/// deadline (the clock is read every 16384 steps).
pub fn regex_time(steps: int) -> bool {
    (steps & 16383) == 0 && time_up(1048576)
}

/// A match attempt went over the backtracking budget (regex_limit).
pub fn regex_over() {
    limits(LIM_REGEX_OVER, 0.0);
}

/// A match stopped (regex_time, regex_over): later attempts give up at once.
pub fn regex_stopped() -> bool {
    limits(LIM_STOPPED, 0.0) != 0.0
}

/// Whether an attempt ran out of the budget since the last call; clears it.
pub fn take_regex_over() -> bool {
    limits(LIM_TAKE_OVER, 0.0) != 0.0
}

impl Vm {
    pub fn native_fn(&mut self, name: &str, id: int, length: int) -> int {
        let fp = self.function_proto;
        let f = self.alloc(C_NATIVE, fp);
        self.objs[f as usize].func = id as i32;
        let a = self.intern(name);
        let _ = a;
        let sv9 = self.str_val(name);
        self.add_prop(f, A_NAME, sv9, P_HIDDEN | P_READONLY);
        self.add_prop(f, A_LENGTH, Val::Num(length as double), P_HIDDEN | P_READONLY);
        f
    }

    /// A method on `o`: not enumerable.
    pub fn method(&mut self, o: int, name: &str, id: int, length: int) -> int {
        let f = self.native_fn(name, id, length);
        let a = self.intern(name);
        self.add_own(o, a, Val::Obj(f), P_HIDDEN);
        f
    }

    pub fn sym_method(&mut self, o: int, atom: int, label: &str, id: int, length: int) -> int {
        let f = self.native_fn(label, id, length);
        self.add_own(o, atom, Val::Obj(f), P_HIDDEN);
        f
    }

    pub fn getter(&mut self, o: int, name: &str, id: int) {
        // the function's own name is "get size", "get [Symbol.species]"
        let fname = if name.starts_with("@@") {
            format!("get [Symbol.{}]", String::from(&name[2..name.as_bytes().len()]))
        } else {
            format!("get {}", name)
        };
        let f = self.native_fn(fname.as_str(), id, 0);
        let a = self.intern(name);
        self.define_accessor(o, a, Val::Obj(f), 0, true);
    }

    pub fn value_prop(&mut self, o: int, name: &str, v: Val) {
        let a = self.intern(name);
        self.add_prop(o, a, v, P_HIDDEN | P_READONLY | P_FIXED);
    }

    /// A constructor with its prototype object wired both ways.
    pub fn ctor(&mut self, name: &str, id: int, length: int, proto: int) -> int {
        let f = self.native_fn(name, id, length);
        self.add_prop(f, A_PROTOTYPE, Val::Obj(proto), P_HIDDEN | P_READONLY | P_FIXED);
        self.add_own(proto, A_CONSTRUCTOR, Val::Obj(f), P_HIDDEN);
        let g = self.global;
        let a = self.intern(name);
        self.add_own(g, a, Val::Obj(f), P_HIDDEN);
        f
    }

    pub fn well_known_symbol(&mut self, atom: int, desc: &str) -> int {
        let sp = self.symbol_proto;
        let s = self.alloc(C_SYMBOL, sp);
        self.objs[s as usize].pos = atom as i32;
        let sv8 = self.str_val(desc);
        self.put_prim(s, sv8);
        self.symbol_atoms.insert(atom, s);
        if (atom as usize) < self.atom_kinds.len() {
            self.atom_kinds[atom as usize] = 0;
        }
        self.symbols.push(s);
        s
    }

    pub fn setup(&mut self) {
        let op = self.alloc(C_OBJECT, -1);
        self.object_proto = op;
        let fp = self.alloc(C_NATIVE, op);
        self.objs[fp as usize].func = NF_NOOP as i32;
        self.function_proto = fp;
        let g = self.alloc(C_OBJECT, op);
        self.global = g;
        let ap = self.alloc(C_ARRAY, op);
        self.array_proto = ap;
        let sp = self.alloc(C_STRING, op);
        let sv7 = self.str_val("");
        self.put_prim(sp, sv7);
        self.string_proto = sp;
        let np = self.alloc(C_NUMBER, op);
        self.put_prim(np, Val::Num(0.0));
        self.number_proto = np;
        let bp = self.alloc(C_BOOLEAN, op);
        self.put_prim(bp, Val::Bool(false));
        self.boolean_proto = bp;
        let symp = self.alloc(C_OBJECT, op);
        self.symbol_proto = symp;
        let ep = self.alloc(C_OBJECT, op);
        self.error_proto = ep;
        let rp = self.alloc(C_OBJECT, op);
        self.regexp_proto = rp;
        let dp = self.alloc(C_OBJECT, op);
        self.date_proto = dp;
        let mp = self.alloc(C_OBJECT, op);
        self.map_proto = mp;
        let setp = self.alloc(C_OBJECT, op);
        self.set_proto = setp;
        let ip = self.alloc(C_OBJECT, op);
        self.iter_proto = ip;
        let pp = self.alloc(C_OBJECT, op);
        self.promise_proto = pp;
        self.roots = vec![op, fp, g, ap, sp, np, bp, symp, ep, rp, dp, mp, setp, ip, pp];

        let sv6 = self.str_val("");
        self.add_prop(fp, A_NAME, sv6, P_HIDDEN | P_READONLY);
        self.add_prop(fp, A_LENGTH, Val::Num(0.0), P_HIDDEN | P_READONLY);

        let it_sym = self.well_known_symbol(A_ITERATOR, "Symbol.iterator");
        let a_async = self.intern("@@asyncIterator");
        let async_sym = self.well_known_symbol(a_async, "Symbol.asyncIterator");
        let tp_sym = self.well_known_symbol(A_TOPRIMITIVE, "Symbol.toPrimitive");
        let hi_sym = self.well_known_symbol(A_HASINSTANCE, "Symbol.hasInstance");
        let a_tag = self.intern("@@toStringTag");
        let tag_sym = self.well_known_symbol(a_tag, "Symbol.toStringTag");
        let a_species = self.intern("@@species");
        let species_sym = self.well_known_symbol(a_species, "Symbol.species");
        let a_unscop = self.intern("@@unscopables");
        let unscop_sym = self.well_known_symbol(a_unscop, "Symbol.unscopables");

        // globals
        self.add_own(g, self.atom_lookup("globalThis"), Val::Obj(g), P_HIDDEN);
        self.value_prop(g, "NaN", Val::Num(nan()));
        self.value_prop(g, "Infinity", Val::Num(infinity()));
        self.value_prop(g, "undefined", Val::Undef);
        self.method(g, "print", NF_PRINT, 0);
        self.method(g, "parseInt", NF_PARSEINT, 2);
        self.method(g, "parseFloat", NF_PARSEFLOAT, 1);
        self.method(g, "isNaN", NF_ISNAN, 1);
        self.method(g, "isFinite", NF_ISFINITE, 1);
        self.method(g, "encodeURIComponent", NF_ENCODE_URI_COMPONENT, 1);
        self.method(g, "decodeURIComponent", NF_DECODE_URI_COMPONENT, 1);
        self.method(g, "encodeURI", NF_ENCODE_URI, 1);
        self.method(g, "decodeURI", NF_DECODE_URI, 1);
        self.method(g, "escape", NF_ESCAPE, 1);
        self.method(g, "unescape", NF_UNESCAPE, 1);
        self.method(g, "gc", NF_GC, 0);
        self.method(g, "queueMicrotask", NF_QUEUE_MICROTASK, 1);
        self.method(g, "__hostSync", NF_HOST_SYNC, 1);
        let console = self.new_object();
        self.method(console, "log", NF_PRINT, 0);
        self.method(console, "info", NF_PRINT, 0);
        self.method(console, "warn", NF_PRINT, 0);
        self.method(console, "error", NF_PRINT, 0);
        self.method(console, "debug", NF_PRINT, 0);
        let a_console = self.intern("console");
        self.add_own(g, a_console, Val::Obj(console), P_HIDDEN);
        let perf = self.new_object();
        self.method(perf, "now", NF_PERF_NOW, 0);
        let a_perf = self.intern("performance");
        self.add_own(g, a_perf, Val::Obj(perf), P_HIDDEN);

        // Object
        let oc = self.ctor("Object", NF_OBJECT, 1, op);
        self.method(oc, "keys", NF_O_KEYS, 1);
        self.method(oc, "values", NF_O_VALUES, 1);
        self.method(oc, "entries", NF_O_ENTRIES, 1);
        self.method(oc, "assign", NF_O_ASSIGN, 2);
        self.method(oc, "create", NF_O_CREATE, 2);
        self.method(oc, "getPrototypeOf", NF_O_GETPROTO, 1);
        self.method(oc, "setPrototypeOf", NF_O_SETPROTO, 2);
        self.method(oc, "defineProperty", NF_O_DEFPROP, 3);
        self.method(oc, "defineProperties", NF_O_DEFPROPS, 2);
        self.method(oc, "getOwnPropertyNames", NF_O_OWNNAMES, 1);
        self.method(oc, "getOwnPropertyDescriptor", NF_O_OWNDESC, 2);
        self.method(oc, "getOwnPropertyDescriptors", NF_O_OWNDESCS, 1);
        self.method(oc, "getOwnPropertySymbols", NF_O_OWNSYMBOLS, 1);
        self.method(oc, "freeze", NF_O_FREEZE, 1);
        self.method(oc, "isFrozen", NF_O_ISFROZEN, 1);
        self.method(oc, "seal", NF_O_SEAL, 1);
        self.method(oc, "isSealed", NF_O_ISSEALED, 1);
        self.method(oc, "preventExtensions", NF_O_PREVENTEXT, 1);
        self.method(oc, "isExtensible", NF_O_ISEXT, 1);
        self.method(oc, "is", NF_O_IS, 2);
        self.method(oc, "fromEntries", NF_O_FROMENTRIES, 1);
        self.method(oc, "hasOwn", NF_O_HASOWN, 2);
        self.method(oc, "groupBy", NF_O_GROUPBY, 2);
        self.method(op, "hasOwnProperty", NF_OP_HASOWN, 1);
        self.method(op, "isPrototypeOf", NF_OP_ISPROTO, 1);
        self.method(op, "propertyIsEnumerable", NF_OP_PROPENUM, 1);
        self.method(op, "toString", NF_OP_TOSTRING, 0);
        self.method(op, "toLocaleString", NF_OP_TOLOCALE, 0);
        self.method(op, "valueOf", NF_OP_VALUEOF, 0);

        // Function
        let fc = self.ctor("Function", NF_FUNCTION, 1, fp);
        let _ = fc;
        self.method(fp, "call", NF_CALL, 1);
        self.method(fp, "apply", NF_APPLY, 2);
        self.method(fp, "bind", NF_BIND, 1);
        self.method(fp, "toString", NF_FN_TOSTRING, 0);

        // Array
        let ac = self.ctor("Array", NF_ARRAY, 1, ap);
        self.method(ac, "isArray", NF_A_ISARRAY, 1);
        self.method(ac, "from", NF_A_FROM, 1);
        self.method(ac, "of", NF_A_OF, 0);
        let names = vec![
            ("push", NF_AP_PUSH, 1),
            ("pop", NF_AP_POP, 0),
            ("shift", NF_AP_SHIFT, 0),
            ("unshift", NF_AP_UNSHIFT, 1),
            ("slice", NF_AP_SLICE, 2),
            ("splice", NF_AP_SPLICE, 2),
            ("concat", NF_AP_CONCAT, 1),
            ("join", NF_AP_JOIN, 1),
            ("reverse", NF_AP_REVERSE, 0),
            ("indexOf", NF_AP_INDEXOF, 1),
            ("lastIndexOf", NF_AP_LASTINDEXOF, 1),
            ("includes", NF_AP_INCLUDES, 1),
            ("forEach", NF_AP_FOREACH, 1),
            ("map", NF_AP_MAP, 1),
            ("filter", NF_AP_FILTER, 1),
            ("reduce", NF_AP_REDUCE, 1),
            ("reduceRight", NF_AP_REDUCERIGHT, 1),
            ("some", NF_AP_SOME, 1),
            ("every", NF_AP_EVERY, 1),
            ("find", NF_AP_FIND, 1),
            ("findIndex", NF_AP_FINDINDEX, 1),
            ("findLast", NF_AP_FINDLAST, 1),
            ("findLastIndex", NF_AP_FINDLASTINDEX, 1),
            ("sort", NF_AP_SORT, 1),
            ("fill", NF_AP_FILL, 1),
            ("keys", NF_AP_KEYS, 0),
            ("entries", NF_AP_ENTRIES, 0),
            ("flat", NF_AP_FLAT, 0),
            ("flatMap", NF_AP_FLATMAP, 1),
            ("toString", NF_AP_TOSTRING, 0),
            ("at", NF_AP_AT, 1),
            ("copyWithin", NF_AP_COPYWITHIN, 2),
            ("toReversed", NF_AP_TOREVERSED, 0),
            ("toSorted", NF_AP_TOSORTED, 1),
            ("with", NF_AP_WITH, 2),
        ];
        for (n, id, l) in names {
            self.method(ap, n, id, l);
        }
        let values = self.method(ap, "values", NF_AP_VALUES, 0);
        self.array_values_fn = values;
        self.add_own(ap, A_ITERATOR, Val::Obj(values), P_HIDDEN);
        self.method(ip, "next", NF_ITER_NEXT, 0);

        // String
        let sc = self.ctor("String", NF_STRING, 1, sp);
        self.method(sc, "fromCharCode", NF_S_FROMCHARCODE, 1);
        self.method(sc, "fromCodePoint", NF_S_FROMCODEPOINT, 1);
        self.method(sc, "raw", NF_S_RAW, 1);
        let snames = vec![
            ("charAt", NF_SP_CHARAT, 1),
            ("charCodeAt", NF_SP_CHARCODEAT, 1),
            ("codePointAt", NF_SP_CODEPOINTAT, 1),
            ("indexOf", NF_SP_INDEXOF, 1),
            ("lastIndexOf", NF_SP_LASTINDEXOF, 1),
            ("includes", NF_SP_INCLUDES, 1),
            ("startsWith", NF_SP_STARTSWITH, 1),
            ("endsWith", NF_SP_ENDSWITH, 1),
            ("slice", NF_SP_SLICE, 2),
            ("substring", NF_SP_SUBSTRING, 2),
            ("substr", NF_SP_SUBSTR, 2),
            ("toUpperCase", NF_SP_UPPER, 0),
            ("toLowerCase", NF_SP_LOWER, 0),
            ("toLocaleUpperCase", NF_SP_UPPER, 0),
            ("toLocaleLowerCase", NF_SP_LOWER, 0),
            ("trim", NF_SP_TRIM, 0),
            ("trimStart", NF_SP_TRIMSTART, 0),
            ("trimEnd", NF_SP_TRIMEND, 0),
            ("trimLeft", NF_SP_TRIMSTART, 0),
            ("trimRight", NF_SP_TRIMEND, 0),
            ("split", NF_SP_SPLIT, 2),
            ("replace", NF_SP_REPLACE, 2),
            ("replaceAll", NF_SP_REPLACEALL, 2),
            ("match", NF_SP_MATCH, 1),
            ("matchAll", NF_SP_MATCHALL, 1),
            ("search", NF_SP_SEARCH, 1),
            ("repeat", NF_SP_REPEAT, 1),
            ("padStart", NF_SP_PADSTART, 2),
            ("padEnd", NF_SP_PADEND, 2),
            ("concat", NF_SP_CONCAT, 1),
            ("at", NF_SP_AT, 1),
            ("localeCompare", NF_SP_LOCALECOMPARE, 1),
            ("toString", NF_SP_TOSTRING, 0),
            ("valueOf", NF_SP_TOSTRING, 0),
            ("normalize", NF_SP_NORMALIZE, 0),
            ("isWellFormed", NF_SP_ISWELLFORMED, 0),
            ("toWellFormed", NF_SP_TOWELLFORMED, 0),
        ];
        for (n, id, l) in snames {
            self.method(sp, n, id, l);
        }
        self.sym_method(sp, A_ITERATOR, "[Symbol.iterator]", NF_SP_ITERATOR, 0);

        // Number
        let nc = self.ctor("Number", NF_NUMBER, 1, np);
        self.method(nc, "isInteger", NF_N_ISINTEGER, 1);
        self.method(nc, "isSafeInteger", NF_N_ISSAFEINTEGER, 1);
        self.method(nc, "isFinite", NF_N_ISFINITE, 1);
        self.method(nc, "isNaN", NF_N_ISNAN, 1);
        self.method(nc, "parseFloat", NF_PARSEFLOAT, 1);
        self.method(nc, "parseInt", NF_PARSEINT, 2);
        self.value_prop(nc, "MAX_VALUE", Val::Num(1.7976931348623157e308));
        self.value_prop(nc, "MIN_VALUE", Val::Num(5e-324));
        self.value_prop(nc, "EPSILON", Val::Num(2.220446049250313e-16));
        self.value_prop(nc, "MAX_SAFE_INTEGER", Val::Num(9007199254740991.0));
        self.value_prop(nc, "MIN_SAFE_INTEGER", Val::Num(-9007199254740991.0));
        self.value_prop(nc, "POSITIVE_INFINITY", Val::Num(infinity()));
        self.value_prop(nc, "NEGATIVE_INFINITY", Val::Num(-infinity()));
        self.value_prop(nc, "NaN", Val::Num(nan()));
        self.method(np, "toString", NF_NP_TOSTRING, 1);
        self.method(np, "toLocaleString", NF_NP_TOSTRING, 0);
        self.method(np, "toFixed", NF_NP_TOFIXED, 1);
        self.method(np, "toPrecision", NF_NP_TOPRECISION, 1);
        self.method(np, "toExponential", NF_NP_TOEXPONENTIAL, 1);
        self.method(np, "valueOf", NF_NP_VALUEOF, 0);

        // Boolean
        self.ctor("Boolean", NF_BOOLEAN, 1, bp);
        self.method(bp, "toString", NF_BP_TOSTRING, 0);
        self.method(bp, "valueOf", NF_BP_VALUEOF, 0);

        // Symbol
        let symc = self.ctor("Symbol", NF_SYMBOL, 0, symp);
        self.method(symc, "for", NF_SYM_FOR, 1);
        self.method(symc, "keyFor", NF_SYM_KEYFOR, 1);
        self.value_prop(symc, "iterator", Val::Obj(it_sym));
        self.value_prop(symc, "asyncIterator", Val::Obj(async_sym));
        self.value_prop(symc, "toPrimitive", Val::Obj(tp_sym));
        self.value_prop(symc, "hasInstance", Val::Obj(hi_sym));
        self.value_prop(symc, "toStringTag", Val::Obj(tag_sym));
        self.value_prop(symc, "species", Val::Obj(species_sym));
        self.value_prop(symc, "unscopables", Val::Obj(unscop_sym));
        for (name, desc) in vec![("match", "Symbol.match"), ("matchAll", "Symbol.matchAll"), ("replace", "Symbol.replace"), ("search", "Symbol.search"), ("split", "Symbol.split"), ("isConcatSpreadable", "Symbol.isConcatSpreadable"), ("dispose", "Symbol.dispose"), ("asyncDispose", "Symbol.asyncDispose")] {
            let a = self.intern(format!("@@{}", name).as_str());
            let s = self.well_known_symbol(a, desc);
            self.value_prop(symc, name, Val::Obj(s));
        }
        self.method(symp, "toString", NF_SYMP_TOSTRING, 0);
        self.getter(symp, "description", NF_SYMP_DESCRIPTION);

        // Errors
        let err_names = vec!["Error", "TypeError", "RangeError", "ReferenceError", "SyntaxError", "EvalError", "URIError", "AggregateError", "SuppressedError"];
        let mut k: int = 0;
        let mut base_ctor: int = -1;
        for n in err_names {
            let proto = if k == 0 { ep } else { self.alloc(C_OBJECT, ep) };
            let c = self.ctor(n, NF_ERROR + k, if n == "AggregateError" { 2 } else if n == "SuppressedError" { 3 } else { 1 }, proto);
            if k == 0 {
                base_ctor = c;
            } else {
                self.put_proto(c, base_ctor);
            }
            let sv5 = self.str_val(n);
            self.add_own(proto, A_NAME, sv5, P_HIDDEN);
            let sv4 = self.str_val("");
            self.add_own(proto, A_MESSAGE, sv4, P_HIDDEN);
            if k == 1 {
                self.type_error_proto = proto;
            } else if k == 2 {
                self.range_error_proto = proto;
            } else if k == 3 {
                self.reference_error_proto = proto;
            } else if k == 4 {
                self.syntax_error_proto = proto;
            }
            self.roots.push(proto);
            k += 1;
        }
        self.method(ep, "toString", NF_EP_TOSTRING, 0);

        // Math
        let math = self.new_object();
        let a_math = self.intern("Math");
        self.add_own(g, a_math, Val::Obj(math), P_HIDDEN);
        let mut i: int = 0;
        for n in math_names() {
            let len = if n.as_str() == "pow" || n.as_str() == "atan2" || n.as_str() == "imul" || n.as_str() == "min" || n.as_str() == "max" || n.as_str() == "hypot" {
                2
            } else if n.as_str() == "random" {
                0
            } else {
                1
            };
            self.method(math, n.as_str(), NF_MATH + i, len);
            i += 1;
        }
        self.value_prop(math, "PI", Val::Num(3.141592653589793));
        self.value_prop(math, "E", Val::Num(2.718281828459045));
        self.value_prop(math, "LN2", Val::Num(0.6931471805599453));
        self.value_prop(math, "LN10", Val::Num(2.302585092994046));
        self.value_prop(math, "LOG2E", Val::Num(1.4426950408889634));
        self.value_prop(math, "LOG10E", Val::Num(0.4342944819032518));
        self.value_prop(math, "SQRT2", Val::Num(1.4142135623730951));
        self.value_prop(math, "SQRT1_2", Val::Num(0.7071067811865476));

        // JSON
        let json = self.new_object();
        let a_json = self.intern("JSON");
        self.add_own(g, a_json, Val::Obj(json), P_HIDDEN);
        self.method(json, "stringify", NF_JSON_STRINGIFY, 3);
        self.method(json, "parse", NF_JSON_PARSE, 2);

        // Reflect
        let reflect = self.new_object();
        let a_reflect = self.intern("Reflect");
        self.add_own(g, a_reflect, Val::Obj(reflect), P_HIDDEN);
        self.method(reflect, "apply", NF_REFLECT_APPLY, 3);
        self.method(reflect, "construct", NF_REFLECT_CONSTRUCT, 2);
        self.method(reflect, "ownKeys", NF_REFLECT_OWNKEYS, 1);
        self.method(reflect, "has", NF_REFLECT_HAS, 2);
        self.method(reflect, "get", NF_REFLECT_GET, 2);
        self.method(reflect, "set", NF_REFLECT_SET, 3);
        self.method(reflect, "getPrototypeOf", NF_REFLECT_GETPROTO, 1);
        self.method(reflect, "setPrototypeOf", NF_O_SETPROTO, 2);
        self.method(reflect, "defineProperty", NF_REFLECT_DEFPROP, 3);
        self.method(reflect, "deleteProperty", NF_REFLECT_DELETE, 2);

        // RegExp
        self.ctor("RegExp", NF_REGEXP, 2, rp);
        self.method(rp, "exec", NF_RP_EXEC, 1);
        self.method(rp, "test", NF_RP_TEST, 1);
        self.method(rp, "toString", NF_RP_TOSTRING, 0);
        self.getter(rp, "flags", NF_RP_FLAGS);
        self.getter(rp, "source", NF_RP_SOURCE);
        let rflags: Vec<&str> = vec!["hasIndices", "global", "ignoreCase", "multiline", "dotAll", "unicode", "unicodeSets", "sticky"];
        let mut rfi: int = 0;
        while rfi < 8 {
            self.getter(rp, rflags[rfi as usize], NF_RP_FLAG0 + rfi);
            rfi += 1;
        }

        // Date
        let dc = self.ctor("Date", NF_DATE, 7, dp);
        self.method(dc, "now", NF_DATE_NOW, 0);
        self.method(dc, "UTC", NF_DATE_UTC, 7);
        self.method(dc, "parse", NF_DATE_PARSE, 1);
        let mut fi: int = 0;
        for f in date_fields() {
            let gname = format!("get{}", f);
            self.method(dp, gname.as_str(), NF_DP_GET + fi, 0);
            let uname = format!("getUTC{}", f);
            self.method(dp, uname.as_str(), NF_DP_GET + fi, 0);
            if f.as_str() != "Day" && f.as_str() != "TimezoneOffset" {
                // the setters' lengths: how many fields each takes
                let arity: int = if fi == 0 || fi == 5 { 3 } else if fi == 1 || fi == 6 { 2 } else if fi == 4 { 4 } else { 1 };
                let sname = format!("set{}", f);
                self.method(dp, sname.as_str(), NF_DP_SET + fi, arity);
                if f.as_str() != "Year" && f.as_str() != "Time" {
                    let suname = format!("setUTC{}", f);
                    self.method(dp, suname.as_str(), NF_DP_SET + fi, arity);
                }
            }
            fi += 1;
        }
        self.method(dp, "toISOString", NF_DP_TOISO, 0);
        self.method(dp, "toJSON", NF_DP_TOJSON, 1);
        self.method(dp, "toString", NF_DP_TOSTRING, 0);
        let utcs = self.method(dp, "toUTCString", NF_DP_TOSTRING, 0);
        let a_gmt = self.intern("toGMTString");
        self.add_own(dp, a_gmt, Val::Obj(utcs), P_HIDDEN);
        self.method(dp, "toDateString", NF_DP_TOSTRING, 0);
        self.method(dp, "toLocaleString", NF_DP_TOSTRING, 0);
        self.method(dp, "valueOf", NF_DP_VALUEOF, 0);

        // Map / Set
        self.ctor("Map", NF_MAP, 0, mp);
        self.method(mp, "get", NF_MP_GET, 1);
        self.method(mp, "set", NF_MP_SET, 2);
        self.method(mp, "has", NF_MP_HAS, 1);
        self.method(mp, "delete", NF_MP_DELETE, 1);
        self.method(mp, "clear", NF_MP_CLEAR, 0);
        self.method(mp, "forEach", NF_MP_FOREACH, 1);
        self.method(mp, "keys", NF_MP_KEYS, 0);
        self.method(mp, "values", NF_MP_VALUES, 0);
        let me = self.method(mp, "entries", NF_MP_ENTRIES, 0);
        self.add_own(mp, A_ITERATOR, Val::Obj(me), P_HIDDEN);
        self.getter(mp, "size", NF_MP_SIZE);
        self.method(mp, "getOrInsert", NF_MP_GETORINSERT, 2);
        self.method(mp, "getOrInsertComputed", NF_MP_GETORINSERTC, 2);
        self.ctor("Set", NF_SET, 0, setp);
        self.method(setp, "add", NF_SETP_ADD, 1);
        self.method(setp, "has", NF_SP_HAS, 1);
        self.method(setp, "delete", NF_SP_DELETE, 1);
        self.method(setp, "clear", NF_SP_CLEAR, 0);
        self.method(setp, "forEach", NF_SP_FOREACH, 1);
        self.method(setp, "entries", NF_SP_ENTRIES, 0);
        let sv = self.method(setp, "values", NF_SP_KEYS, 0);
        let a_keys = self.intern("keys");
        self.add_own(setp, a_keys, Val::Obj(sv), P_HIDDEN);
        self.add_own(setp, A_ITERATOR, Val::Obj(sv), P_HIDDEN);
        self.getter(setp, "size", NF_SP_SIZE);
        // WeakMap / WeakSet: a Map / Set underneath, with only the methods a
        // weak collection has (no size, no iteration)
        let op0 = self.object_proto;
        let wmp = self.alloc(C_OBJECT, op0);
        self.roots.push(wmp);
        self.ctor("WeakMap", NF_WEAKMAP, 0, wmp);
        self.method(wmp, "get", NF_WM_GET, 1);
        self.method(wmp, "set", NF_WM_SET, 2);
        self.method(wmp, "has", NF_WM_HAS, 1);
        self.method(wmp, "delete", NF_WM_DELETE, 1);
        self.method(wmp, "getOrInsert", NF_WM_GETORINSERT, 2);
        self.method(wmp, "getOrInsertComputed", NF_WM_GETORINSERTC, 2);
        let wsp = self.alloc(C_OBJECT, op0);
        self.roots.push(wsp);
        self.ctor("WeakSet", NF_WEAKSET, 0, wsp);
        self.method(wsp, "add", NF_WS_ADD, 1);
        self.method(wsp, "has", NF_WS_HAS, 1);
        self.method(wsp, "delete", NF_WS_DELETE, 1);
        let a_tag2 = self.intern("@@toStringTag");
        let sv3 = self.str_val("WeakMap");
        self.add_prop(wmp, a_tag2, sv3, P_HIDDEN | P_READONLY);
        let sv2 = self.str_val("WeakSet");
        self.add_prop(wsp, a_tag2, sv2, P_HIDDEN | P_READONLY);

        // Promise
        let prc = self.ctor("Promise", NF_PROMISE, 1, pp);
        self.method(prc, "resolve", NF_PR_RESOLVE, 1);
        self.method(prc, "reject", NF_PR_REJECT, 1);
        self.method(prc, "all", NF_PR_ALL, 1);
        self.method(pp, "then", NF_PR_THEN, 2);
        self.method(pp, "catch", NF_PR_CATCH, 1);
        self.method(pp, "finally", NF_PR_FINALLY, 1);
        self.setup_coroutines();
        self.setup_typed();
        self.setup_proxy();
        self.setup_dynamic();
        self.setup_intl();
        self.setup_bigint();
    }

    // ---- helpers

    pub fn this_num(&mut self, this: &Val) -> double {
        match this {
            Val::Num(n) => *n,
            Val::Obj(o) => {
                if self.objs[*o as usize].class == C_NUMBER {
                    if let Val::Num(n) = self.colds[self.objs[*o as usize].cold as usize].prim {
                        return n;
                    }
                }
                self.throw_type("Number.prototype method called on incompatible receiver");
                0.0
            }
            _ => {
                self.throw_type("Number.prototype method called on incompatible receiver");
                0.0
            }
        }
    }

    pub fn this_str(&mut self, this: &Val) -> Rc<String> {
        match this {
            Val::Str(s) => self.sget(*s),
            Val::Undef | Val::Null => {
                self.throw_type("String.prototype method called on null or undefined");
                Rc::new(String::new())
            }
            _ => self.to_str(this),
        }
    }

    pub fn len_of(&mut self, v: &Val) -> int {
        if let Val::Obj(o) = v {
            let c = self.objs[*o as usize].class;
            if c == C_ARRAY || c == C_ARGUMENTS {
                return self.evecs[self.objs[*o as usize].elems as usize].v.len() as int;
            }
        }
        let l = self.get(v, A_LENGTH);
        let n = to_integer(self.to_number(&l));
        if n < 0.0 {
            0
        } else {
            n as int
        }
    }

    /// A relative index clamped to [0, len]
    pub fn rel_index(&mut self, v: &Val, len: int, dflt: int) -> int {
        if matches!(v, Val::Undef) {
            return dflt;
        }
        let n = to_integer(self.to_number(v));
        if n < 0.0 {
            let r = (len as double) + n;
            if r < 0.0 {
                0
            } else {
                r as int
            }
        } else if n > len as double {
            len
        } else {
            n as int
        }
    }

    pub fn error_ctor_proto(&self, kind: int) -> int {
        let names = vec!["Error", "TypeError", "RangeError", "ReferenceError", "SyntaxError", "EvalError", "URIError", "AggregateError", "SuppressedError"];
        let g = self.global;
        let a = self.atom_lookup(names[kind as usize]);
        let slot = self.find(g, a);
        let c = obj_of(&self.objs[g as usize].get_slot(slot as usize));
        let ps = self.find(c, A_PROTOTYPE);
        obj_of(&self.objs[c as usize].get_slot(ps as usize))
    }

    /// The prototype a constructor call should use: new.target's, else `d`.
    pub fn proto_from(&mut self, new_target: &Val, d: int) -> int {
        if let Val::Obj(nt) = new_target {
            let p = self.get_obj(*nt, A_PROTOTYPE, new_target);
            if let Val::Obj(po) = p {
                return po;
            }
        }
        d
    }

    pub fn display(&mut self, v: &Val) -> String {
        match v {
            Val::Obj(o) => {
                let c = self.objs[*o as usize].class;
                if c == C_SYMBOL {
                    let d = self.colds[self.objs[*o as usize].cold as usize].prim.clone();
                    let ds = if let Val::Str(s) = d { self.sref(s).to_string() } else { String::new() };
                    return format!("Symbol({})", ds);
                }
                if c == C_BIGINT {
                    return format!("{}n", self.big_text(v));
                }
                let s = self.to_string(v);
                if self.throwing {
                    self.throwing = false;
                    self.exc = Val::Undef;
                    return String::from("[object]");
                }
                s
            }
            Val::Big(_) => format!("{}n", self.big_text(v)),
            _ => self.to_string(v),
        }
    }

    // ---- RegExp

    pub fn new_regexp(&mut self, src: &Val, flags: &Val) -> Val {
        let s = if matches!(src, Val::Undef) { String::from("(?:)") } else { self.to_string(src) };
        let f = if matches!(flags, Val::Undef) { String::new() } else { self.to_string(flags) };
        let idx = self.regexp_index(s.as_str(), f.as_str());
        if idx < 0 {
            return Val::Undef;
        }
        let sv = self.string_val(s);
        self.make_regexp(idx, sv)
    }

    /// The compiled regex of source `s` and flags `f` (in `regexes`), -1
    /// after a SyntaxError.
    pub fn regexp_index(&mut self, s: &str, f: &str) -> int {
        let key = format!("{}/{}", s, f);
        match self.regex_cache.get(&key) {
            Some(i) => *i,
            None => {
                let re = regex::compile(s, f);
                if !re.error.is_empty() {
                    let m = format!("{}: /{}/", re.error, s);
                    self.throw_syntax(m.as_str());
                    return -1;
                }
                self.regexes.push(re);
                let i = (self.regexes.len() as int) - 1;
                self.regex_cache.insert(key, i);
                i
            }
        }
    }

    /// The `source` a RegExp of compiled regex `idx` shows. A restored
    /// snapshot keeps the regexes by their cache key ("source/flags") but
    /// not the shown texts, so those are made again here when missing.
    pub fn regex_shown_at(&mut self, idx: int) -> Val {
        let ix = idx as usize;
        if ix < self.regex_shown.len() && !matches!(self.regex_shown[ix], Val::Undef) {
            return self.regex_shown[ix].clone();
        }
        let mut key = String::new();
        for (k, v) in self.regex_cache.iter() {
            if *v == idx {
                key = k.clone();
            }
        }
        // "source/flags": the flags are the letters after the last '/'
        let cs: Vec<char> = key.chars().collect();
        let mut cut = cs.len();
        while cut > 0 && cs[cut - 1] != '/' {
            cut -= 1;
        }
        if cut == 0 {
            return Val::Undef;
        }
        let mut source = String::new();
        let mut j: usize = 0;
        while j + 1 < cut {
            source.push(cs[j]);
            j += 1;
        }
        let shown = shown_source(source.as_str());
        while self.regex_shown.len() <= ix {
            self.regex_shown.push(Val::Undef);
        }
        let v = self.string_val(shown);
        self.regex_shown[ix] = v.clone();
        v
    }

    /// A RegExp object of compiled regex `idx`; `src` its source text.
    pub fn make_regexp(&mut self, idx: int, src: Val) -> Val {
        // the `source` shown, made once per regex
        while self.regex_shown.len() <= idx as usize {
            self.regex_shown.push(Val::Undef);
        }
        if matches!(self.regex_shown[idx as usize], Val::Undef) {
            let s = self.to_string(&src);
            let shown = shown_source(s.as_str());
            self.regex_shown[idx as usize] = self.string_val(shown);
            // read-only properties (as add_prop marks them)
            self.mark_setter(A_LASTINDEX);
            let mut a = A_GLOBAL;
            while a <= A_STICKY {
                self.mark_setter(a);
                a += 1;
            }
            self.mark_setter(A_SOURCE);
        }
        let shown = self.regex_shown[idx as usize].clone();
        let rp = self.regexp_proto;
        let o = self.alloc(C_REGEXP, rp);
        self.objs[o as usize].func = idx as i32;
        self.put_prim(o, src);
        let re = &self.regexes[idx as usize];
        let (g, ic, m, sa, u, y) = (re.global, re.ignore_case, re.multiline, re.dot_all, re.unicode && !re.unicode_sets, re.sticky);
        if self.regex_shape >= 0 {
            // the shape the first one got: no transitions looked up
            self.objs[o as usize].shape = self.regex_shape as i32;
            self.push_val(o, Val::Num(0.0));
            return Val::Obj(o);
        }
        // only lastIndex is its own; source and the flags are getters
        self.add_own(o, A_LASTINDEX, Val::Num(0.0), P_HIDDEN | P_FIXED);
        let sh = self.objs[o as usize].shape as int;
        if !self.shapes[sh as usize].owned {
            self.regex_shape = sh;
        }
        Val::Obj(o)
    }

    /// Sets a regexp's lastIndex as RegExpBuiltinExec does: a read-only
    /// one throws whether or not the caller is strict.
    pub fn set_last_index(&mut self, ro: int, n: double) {
        let slot = self.find(ro, A_LASTINDEX);
        if slot >= 0 {
            let attr = self.attr_at(ro, slot);
            if (attr & P_READONLY) != 0 && (attr & P_ACCESSOR) == 0 {
                self.throw_type("Cannot assign to read only property 'lastIndex' of object");
                return;
            }
        }
        self.set_obj(ro, A_LASTINDEX, Val::Num(n));
    }

    pub fn regexp_flags(&self, o: int) -> String {
        let re = &self.regexes[self.objs[o as usize].func as usize];
        let mut f = String::new();
        if re.has_indices {
            f.push('d');
        }
        if re.global {
            f.push('g');
        }
        if re.ignore_case {
            f.push('i');
        }
        if re.multiline {
            f.push('m');
        }
        if re.dot_all {
            f.push('s');
        }
        if re.unicode && !re.unicode_sets {
            f.push('u');
        }
        if re.unicode_sets {
            f.push('v');
        }
        if re.sticky {
            f.push('y');
        }
        f
    }

    /// RegExp.prototype.exec: the match array or null; updates lastIndex.
    pub fn regexp_exec(&mut self, ro: int, s: &Rc<String>) -> Val {
        self.regexp_exec_in(ro, s, Val::Undef, true)
    }

    /// regexp_exec with the input's own string value when the caller has
    /// one (`input` then shares its handle instead of a new one counting
    /// the whole text again toward the next collection), and for test()
    /// (`want` false) only true or null, no match array.
    pub fn regexp_exec_in(&mut self, ro: int, s: &Rc<String>, inp: Val, want: bool) -> Val {
        let idx = self.objs[ro as usize].func as int;
        let (global, sticky, ngroups, has_indices) = {
            let re = &self.regexes[idx as usize];
            (re.global, re.sticky, re.ngroups, re.has_indices)
        };
        self.rx_cache(s);
        let ulen = self.rx_units.len() as int;
        let mut start: int = 0;
        let li = self.get_obj(ro, A_LASTINDEX, &Val::Obj(ro));
        let lin = self.to_number(&li);
        if self.throwing {
            return Val::Undef;
        }
        if global || sticky {
            let l = to_integer(lin);
            if l > ulen as double {
                self.set_last_index(ro, 0.0);
                return Val::Null;
            }
            if l > 0.0 {
                start = l as int;
            }
        }
        let caps = self.regexes[idx as usize].exec(&self.rx_units, start);
        self.rx_after();
        if caps.is_empty() {
            if global || sticky {
                self.set_last_index(ro, 0.0);
            }
            return Val::Null;
        }
        if global || sticky {
            self.set_last_index(ro, caps[1] as double);
            if self.throwing {
                return Val::Undef;
            }
        }
        if !want {
            return Val::Bool(true);
        }
        let mut items: Vec<Val> = Vec::new();
        let mut g: int = 0;
        while g <= ngroups {
            let a = caps[(g * 2) as usize];
            let b = caps[(g * 2 + 1) as usize];
            if a < 0 || b < 0 {
                items.push(Val::Undef);
            } else {
                items.push(self.string_val(jsstr::sub_units(s.as_str(), &self.rx_units, a, b)));
            }
            g += 1;
        }
        let plain = self.regexes[idx as usize].names.is_empty() && !has_indices;
        let sv9 = if let Val::Str(_) = inp { inp } else { self.rc_val(s.clone()) };
        if plain {
            // index, input and groups (undefined): the shape they give a new
            // array kept (rx_shapes), the slots put at once
            let ma = self.new_array(items);
            let sh = self.objs[ma as usize].shape;
            if self.rx_shapes.len() == 2 && self.rx_shapes[0] == sh {
                let nsh = self.rx_shapes[1];
                self.objs[ma as usize].shape = nsh;
                let most = self.shapes[self.holder(nsh as int)].keys.len();
                slot_room(&mut self.objs[ma as usize], most);
                self.push_val(ma, Val::Num(caps[0] as double));
                self.push_val(ma, sv9);
                self.push_val(ma, Val::Undef);
            } else {
                self.add_own(ma, A_INDEX, Val::Num(caps[0] as double), 0);
                self.add_own(ma, A_INPUT, sv9, 0);
                self.add_own(ma, A_GROUPS, Val::Undef, 0);
                if !self.shapes[sh as usize].owned && !self.shapes[self.objs[ma as usize].shape as usize].owned {
                    self.rx_shapes = vec![sh, self.objs[ma as usize].shape];
                }
            }
            return Val::Obj(ma);
        }
        let arr = self.new_array(items.clone());
        self.add_own(arr, A_INDEX, Val::Num(caps[0] as double), 0);
        self.add_own(arr, A_INPUT, sv9, 0);
        let names = self.regexes[idx as usize].names.clone();
        let nidx = self.regexes[idx as usize].name_index.clone();
        if names.is_empty() {
            self.add_own(arr, A_GROUPS, Val::Undef, 0);
        } else {
            let go = self.alloc(C_OBJECT, -1);
            for g in group_pairs(&names, &nidx, &items).iter() {
                let a = self.intern(g.0.as_str());
                self.add_own(go, a, g.1, 0);
            }
            self.add_own(arr, A_GROUPS, Val::Obj(go), 0);
        }
        if has_indices {
            // `d`: [start, end] of each group, and of each named group
            let mut pairs: Vec<Val> = Vec::new();
            let mut k: int = 0;
            while k <= ngroups {
                let a = caps[(k * 2) as usize];
                let b = caps[(k * 2 + 1) as usize];
                if a < 0 || b < 0 {
                    pairs.push(Val::Undef);
                } else {
                    let pa = self.new_array(vec![Val::Num(a as double), Val::Num(b as double)]);
                    pairs.push(Val::Obj(pa));
                }
                k += 1;
            }
            let ind = self.new_array(pairs.clone());
            if names.is_empty() {
                self.add_own(ind, A_GROUPS, Val::Undef, 0);
            } else {
                let gi = self.alloc(C_OBJECT, -1);
                for g in group_pairs(&names, &nidx, &pairs).iter() {
                    let a = self.intern(g.0.as_str());
                    self.add_own(gi, a, g.1, 0);
                }
                self.add_own(ind, A_GROUPS, Val::Obj(gi), 0);
            }
            let a_ind = self.intern("indices");
            self.add_own(arr, a_ind, Val::Obj(ind), 0);
        }
        Val::Obj(arr)
    }

    /// The replacement text for one match: `$&`, `$1`, `$<name>`, …
    pub fn expand_replacement(&self, repl: &str, matched: &str, pos: int, units: &Vec<int>, caps: &Vec<Val>, groups: &Vec<(String, Val)>) -> String {
        let r = repl.chars().collect::<Vec<char>>();
        let mut out = String::new();
        let n = r.len();
        let mut i: usize = 0;
        while i < n {
            let c = r[i];
            if c != '$' || i + 1 >= n {
                out.push(c);
                i += 1;
                continue;
            }
            let d = r[i + 1];
            if d == '$' {
                out.push('$');
                i += 2;
            } else if d == '&' {
                jsstr::push_js(&mut out, matched);
                i += 2;
            } else if d == '`' {
                jsstr::push_js(&mut out, jsstr::from_units(units, 0, pos).as_str());
                i += 2;
            } else if d == '\'' {
                let end = pos + jsstr::len(matched);
                jsstr::push_js(&mut out, jsstr::from_units(units, end, units.len() as int).as_str());
                i += 2;
            } else if d >= '0' && d <= '9' {
                let mut num = (d as int) - 48;
                let mut used = 2;
                if i + 2 < n && r[i + 2] >= '0' && r[i + 2] <= '9' {
                    let two = num * 10 + ((r[i + 2] as int) - 48);
                    if two >= 1 && two < caps.len() as int {
                        num = two;
                        used = 3;
                    }
                }
                if num >= 1 && num < caps.len() as int {
                    if let Val::Str(s) = &caps[num as usize] {
                        let s = self.sget(s.clone());
                        jsstr::push_js(&mut out, s.as_str());
                    }
                    i += used;
                } else {
                    out.push('$');
                    i += 1;
                }
            } else if d == '<' && !groups.is_empty() {
                let mut j = i + 2;
                let mut name = String::new();
                while j < n && r[j] != '>' {
                    name.push(r[j]);
                    j += 1;
                }
                if j >= n {
                    out.push('$');
                    i += 1;
                    continue;
                }
                for g in groups.iter() {
                    let gv = g.1;
                    if g.0.as_str() == name.as_str() {
                        if let Val::Str(s) = gv {
                            jsstr::push_js(&mut out, self.sref(s));
                        }
                    }
                }
                i = j + 1;
            } else {
                out.push('$');
                i += 1;
            }
        }
        out
    }

    /// A __fastWrap method `f` on the stack frame at fpos, for a string
    /// `this` and text (or no) operands: replace / replaceAll of a text by a
    /// text, split by a text. True when done, the result in stack[fpos];
    /// else the prelude's function runs (call_native).
    #[inline(never)]
    pub fn fast_wrap(&mut self, f: int, fpos: int, argc: int) -> bool {
        let inner = self.objs[f as usize].pos as int;
        let bp = (fpos + 2) as usize;
        let h = match self.stack[bp - 1] {
            Val::Str(h) => h,
            _ => return false,
        };
        let a0 = if argc >= 1 { self.stack[bp] } else { Val::Undef };
        let a1 = if argc >= 2 { self.stack[bp + 1] } else { Val::Undef };
        if (inner == NF_SP_REPLACE || inner == NF_SP_REPLACEALL) && matches!(a0, Val::Str(_)) && matches!(a1, Val::Str(_)) {
            let s = self.sget(h);
            let r = self.string_replace(&s, &a0, &a1, inner == NF_SP_REPLACEALL);
            self.stack[fpos as usize] = r;
            return true;
        }
        // a RegExp straight on RegExp.prototype: the wrapper would call the
        // native itself
        if inner == NF_SP_REPLACE || inner == NF_SP_REPLACEALL {
            let ro = obj_of(&a0);
            if ro >= 0 && self.objs[ro as usize].class == C_REGEXP && (self.objs[ro as usize].proto as int) == self.regexp_proto {
                let s = self.sget(h);
                let r = self.string_replace(&s, &a0, &a1, inner == NF_SP_REPLACEALL);
                self.stack[fpos as usize] = r;
                return true;
            }
        }
        if inner == NF_SP_SPLIT && matches!(a0, Val::Str(_)) && matches!(a1, Val::Undef) {
            let s = self.sget(h);
            let r = self.string_split(&s, &a0, &a1);
            self.stack[fpos as usize] = r;
            return true;
        }
        false
    }

    pub fn string_replace(&mut self, s: &Rc<String>, pattern: &Val, repl: &Val, all: bool) -> Val {
        if let (Val::Str(ph), Val::Str(rh)) = (pattern, repl) {
            // an ASCII text for a text without `$`: no UTF-16 units needed
            if replace_plain_ok(self.sref(*ph), self.sref(*rh)) {
                let out = replace_plain(s.as_str(), self.sref(*ph), self.sref(*rh), all);
                return self.string_val(out);
            }
        }
        let units = self.rx_take(s);
        let r = self.string_replace_units(s, &units, pattern, repl, all);
        self.rx_put(s, units);
        r
    }

    fn string_replace_units(&mut self, s: &Rc<String>, units: &Vec<int>, pattern: &Val, repl: &Val, all: bool) -> Val {
        let fn_repl = self.is_callable(repl);
        let repl_str = if fn_repl { String::new() } else { self.to_string(repl) };
        let plain_repl = !fn_repl && !repl_str.contains("$");
        let mut out = String::new();
        let mut last: int = 0;
        if self.class_of(pattern) == C_REGEXP {
            let ro = obj_of(pattern);
            let idx = self.objs[ro as usize].func as int;
            let global = self.regexes[idx as usize].global;
            if all && !global {
                self.throw_type("replaceAll must be called with a global RegExp");
                return Val::Undef;
            }
            let names = self.regexes[idx as usize].names.clone();
            let nidx = self.regexes[idx as usize].name_index.clone();
            let ngroups = self.regexes[idx as usize].ngroups;
            let mut start: int = 0;
            if !global {
                let li = self.get_obj(ro, A_LASTINDEX, pattern);
                let _ = li;
            }
            loop {
                if start > units.len() as int {
                    break;
                }
                let caps = self.regexes[idx as usize].exec(units, start);
                self.rx_after();
                if self.throwing || self.poll(64) {
                    return Val::Undef;
                }
                if caps.is_empty() {
                    break;
                }
                let (ms, me) = (caps[0], caps[1]);
                jsstr::push_js(&mut out, jsstr::sub_units(s.as_str(), units, last, ms).as_str());
                if plain_repl {
                    // a text without `$`: itself, no captures made
                    jsstr::push_js(&mut out, repl_str.as_str());
                    last = me;
                    if !global {
                        break;
                    }
                    start = if me == ms { me + 1 } else { me };
                    continue;
                }
                let matched = jsstr::sub_units(s.as_str(), units, ms, me);
                let mut capv: Vec<Val> = Vec::new();
                let mut g: int = 0;
                while g <= ngroups {
                    let a = caps[(g * 2) as usize];
                    let b = caps[(g * 2 + 1) as usize];
                    capv.push(if a < 0 || b < 0 { Val::Undef } else { self.string_val(jsstr::sub_units(s.as_str(), units, a, b)) });
                    g += 1;
                }
                let groups: Vec<(String, Val)> = group_pairs(&names, &nidx, &capv);
                if fn_repl {
                    let mut args: Vec<Val> = capv.clone();
                    args.push(Val::Num(ms as double));
                    args.push(self.rc_val(s.clone()));
                    if !groups.is_empty() {
                        let go = self.new_object();
                        for g in groups.iter() {
                            let gv = g.1;
                            let a = self.intern(g.0.as_str());
                            self.add_own(go, a, gv, 0);
                        }
                        args.push(Val::Obj(go));
                    }
                    let r = self.call_value(repl.clone(), Val::Undef, args);
                    if self.throwing {
                        return Val::Undef;
                    }
                    let rs = self.to_string(&r);
                    jsstr::push_js(&mut out, rs.as_str());
                } else {
                    let e = self.expand_replacement(repl_str.as_str(), matched.as_str(), ms, units, &capv, &groups);
                    jsstr::push_js(&mut out, e.as_str());
                }
                last = me;
                if !global {
                    break;
                }
                start = if me == ms { me + 1 } else { me };
            }
            if global {
                self.set_obj(ro, A_LASTINDEX, Val::Num(0.0));
            }
            jsstr::push_js(&mut out, jsstr::sub_units(s.as_str(), units, last, units.len() as int).as_str());
            return self.string_val(out);
        }
        let pat = self.to_string(pattern);
        let plen = jsstr::len(pat.as_str());
        let mut from: int = 0;
        loop {
            let p = jsstr::index_of(s.as_str(), pat.as_str(), from);
            if p < 0 || self.poll(64) {
                break;
            }
            jsstr::push_js(&mut out, jsstr::sub_units(s.as_str(), units, last, p).as_str());
            if fn_repl {
                let sv7 = self.string_val(pat.clone());
                let sv8 = self.rc_val(s.clone());
                let r = self.call_fixed(repl.clone(), Val::Undef, 3, sv7, Val::Num(p as double), sv8);
                if self.throwing {
                    return Val::Undef;
                }
                let rs = self.to_string(&r);
                jsstr::push_js(&mut out, rs.as_str());
            } else {
                let sv6 = self.string_val(pat.clone());
                let e = self.expand_replacement(repl_str.as_str(), pat.as_str(), p, units, &vec![sv6], &Vec::new());
                jsstr::push_js(&mut out, e.as_str());
            }
            last = p + plen;
            if !all {
                break;
            }
            from = if plen == 0 { p + 1 } else { p + plen };
            if from > units.len() as int {
                break;
            }
        }
        jsstr::push_js(&mut out, jsstr::sub_units(s.as_str(), units, last, units.len() as int).as_str());
        self.string_val(out)
    }

    pub fn string_split(&mut self, s: &Rc<String>, sep: &Val, limit: &Val) -> Val {
        let lim: int = if matches!(limit, Val::Undef) {
            2147483647
        } else {
            let l = to_uint32(self.to_number(limit));
            if l > 2147483647.0 {
                2147483647
            } else {
                l as int
            }
        };
        let mut out: Vec<Val> = Vec::new();
        if lim == 0 {
            let a = self.new_array(out);
            return Val::Obj(a);
        }
        if matches!(sep, Val::Undef) {
            out.push(self.rc_val(s.clone()));
            let a = self.new_array(out);
            return Val::Obj(a);
        }
        let units = self.rx_take(s);
        let r = self.split_units(s, &units, sep, lim);
        self.rx_put(s, units);
        r
    }

    fn split_units(&mut self, s: &Rc<String>, units: &Vec<int>, sep: &Val, lim: int) -> Val {
        let mut out: Vec<Val> = Vec::new();
        let n = units.len() as int;
        if self.class_of(sep) == C_REGEXP {
            let ro = obj_of(sep);
            let idx = self.objs[ro as usize].func as int;
            let ngroups = self.regexes[idx as usize].ngroups;
            if n == 0 {
                let caps = self.regexes[idx as usize].exec(units, 0);
                self.rx_after();
                if caps.is_empty() || caps[1] != 0 {
                    out.push(self.rc_val(s.clone()));
                }
                let a = self.new_array(out);
                return Val::Obj(a);
            }
            let mut p: int = 0;
            let mut q: int = 0;
            while q < n {
                let caps = self.regexes[idx as usize].exec(units, q);
                self.rx_after();
                if caps.is_empty() {
                    break;
                }
                let (ms, me) = (caps[0], caps[1]);
                if me >= n + 1 || ms >= n {
                    break;
                }
                if me == p || (me == ms && ms == q && me == p) {
                    q = ms + 1;
                    if me == ms {
                        q = if ms >= q { ms + 1 } else { q };
                    }
                    continue;
                }
                if me == ms && ms < q {
                    q += 1;
                    continue;
                }
                out.push(self.string_val(jsstr::sub_units(s.as_str(), units, p, ms)));
                if out.len() as int >= lim {
                    let a = self.new_array(out);
                    return Val::Obj(a);
                }
                let mut g: int = 1;
                while g <= ngroups {
                    let a = caps[(g * 2) as usize];
                    let b = caps[(g * 2 + 1) as usize];
                    out.push(if a < 0 || b < 0 { Val::Undef } else { self.string_val(jsstr::sub_units(s.as_str(), units, a, b)) });
                    if out.len() as int >= lim {
                        let arr = self.new_array(out);
                        return Val::Obj(arr);
                    }
                    g += 1;
                }
                p = me;
                q = if me == ms { me + 1 } else { me };
            }
            out.push(self.string_val(jsstr::sub_units(s.as_str(), units, p, n)));
            let a = self.new_array(out);
            return Val::Obj(a);
        }
        let sepstr = self.to_string(sep);
        if sepstr.is_empty() {
            let mut i: int = 0;
            while i < n && (out.len() as int) < lim {
                let c = units[i as usize];
                out.push(self.unit_val(c));
                i += 1;
            }
            let a = self.new_array(out);
            return Val::Obj(a);
        }
        if jsstr::is_ascii(s.as_str()) && jsstr::is_ascii(sepstr.as_str()) {
            if sepstr.as_bytes().len() == 1 {
                // one separator byte: a scan, no substring search
                let c = sepstr.as_bytes()[0];
                let nb = s.as_bytes().len();
                let mut a: usize = 0;
                let mut i: usize = 0;
                while i < nb {
                    if s.as_bytes()[i] == c {
                        if (out.len() as int) >= lim {
                            break;
                        }
                        out.push(self.str_val(&s[a..i]));
                        a = i + 1;
                    }
                    i += 1;
                }
                if (out.len() as int) < lim {
                    out.push(self.str_val(&s[a..]));
                }
                let arr = self.new_array(out);
                return Val::Obj(arr);
            }
            for part in s.split(sepstr.as_str()) {
                if (out.len() as int) >= lim {
                    break;
                }
                out.push(self.str_val(part));
            }
            let a = self.new_array(out);
            return Val::Obj(a);
        }
        let slen = jsstr::len(sepstr.as_str());
        let mut from: int = 0;
        loop {
            let p = jsstr::index_of(s.as_str(), sepstr.as_str(), from);
            if p < 0 || (out.len() as int) >= lim {
                break;
            }
            out.push(self.string_val(jsstr::sub_units(s.as_str(), units, from, p)));
            from = p + slen;
        }
        if (out.len() as int) < lim {
            out.push(self.string_val(jsstr::sub_units(s.as_str(), units, from, n)));
        }
        let a = self.new_array(out);
        Val::Obj(a)
    }

    // ---- Map / Set

    /// The key a Map or Set indexes `k` by. An integer-valued number and an
    /// object get a number of their own below -1 (integers even, objects
    /// odd), which no atom is; anything else is an interned tagged string.
    /// Formatting and interning every number cost more than the rest of
    /// `set` / `get` together, and kept every key ever used as an atom.
    // CanBeHeldWeakly: an object, or a symbol not made by Symbol.for
    pub fn held_weakly(&self, v: &Val) -> bool {
        let o = obj_of(v);
        if o < 0 {
            return false;
        }
        if self.objs[o as usize].class != C_SYMBOL {
            return true;
        }
        let a = self.objs[o as usize].pos as usize;
        !self.atom_names[a].starts_with("@@for:")
    }

    // the Map / Set method a Set / WeakMap / WeakSet method id runs as
    pub fn coll_base(id: int) -> int {
        match id {
            NF_SP_HAS | NF_WM_HAS | NF_WS_HAS => NF_MP_HAS,
            NF_SP_DELETE | NF_WM_DELETE | NF_WS_DELETE => NF_MP_DELETE,
            NF_SP_CLEAR => NF_MP_CLEAR,
            NF_SP_FOREACH => NF_MP_FOREACH,
            NF_SP_SIZE => NF_MP_SIZE,
            NF_SP_KEYS => NF_MP_KEYS,
            NF_SP_ENTRIES => NF_MP_ENTRIES,
            NF_WM_GET => NF_MP_GET,
            NF_WM_SET => NF_MP_SET,
            NF_WS_ADD => NF_SETP_ADD,
            NF_WM_GETORINSERT => NF_MP_GETORINSERT,
            NF_WM_GETORINSERTC => NF_MP_GETORINSERTC,
            _ => id,
        }
    }

    pub fn map_key(&mut self, k: &Val) -> int {
        match k {
            Val::Num(n) => {
                let x = *n;
                if x == x.floor() && x.abs() <= 1125899906842624.0 {
                    let i = x as int;
                    let z = if i >= 0 { 2 * i } else { -2 * i - 1 };
                    return -2 - 2 * z;
                }
            }
            Val::Obj(o) => {
                return -3 - 2 * *o;
            }
            _ => {}
        }
        // a string: twice its atom (cached per string handle); the rest:
        // twice the atom of a tagged text, plus one
        if let Val::Str(_) = k {
            let (i, a) = self.to_key(k);
            if a >= 0 {
                return 2 * a;
            }
            let t = format!("{}", i);
            return 2 * self.intern(t.as_str());
        }
        let s = match k {
            Val::Undef => String::from("u"),
            Val::Null => String::from("l"),
            Val::Bool(b) => format!("b{}", b),
            Val::Num(n) => {
                if *n == 0.0 {
                    String::from("n0")
                } else {
                    format!("n{}", number_to_string(*n))
                }
            }
            Val::Str(_) => String::new(),
            Val::Obj(o) => format!("o{}", o),
            Val::Big(n) => format!("g{}", crate::bigint::int_text(*n as int)),
        };
        2 * self.intern(s.as_str()) + 1
    }

    /// The pair in map `m`'s string table (Cold.stab) holding key text
    /// `k` (hash `hash`), -1 none.
    pub fn stab_slot(&self, m: int, k: int, hash: int) -> int {
        let t = &self.colds[self.objs[m as usize].cold as usize].stab;
        if t.is_empty() {
            return -1;
        }
        let keys = &self.evecs[self.objs[m as usize].elems as usize].v;
        let mask = ((t.len() / 2) as int) - 1;
        let mut i = hash & mask;
        loop {
            let e = t[(i * 2) as usize];
            if e == 0 {
                return -1;
            }
            if e > 0 && t[(i * 2 + 1) as usize] == hash {
                if let Val::Str(h2) = &keys[(e - 1) as usize] {
                    if *h2 == k || self.sref(*h2) == self.sref(k) {
                        return i;
                    }
                }
            }
            i = (i + 1) & mask;
        }
    }

    /// Adds entry `pos` (a string key absent from the table) to map `m`'s
    /// string table, growing it to keep it at most half full.
    pub fn stab_add(&mut self, m: int, pos: int, hash: int) {
        let c = self.cold_id(m) as usize;
        let cap = (self.colds[c].stab.len() / 2) as int;
        if (self.colds[c].sused + 1) * 2 > cap {
            let mut spare: Vec<int> = Vec::new();
            let old = take_ints(&mut self.colds[c].stab, &mut spare);
            let mut live: int = 0;
            let mut j: usize = 0;
            while j < old.len() {
                if old[j] > 0 {
                    live += 1;
                }
                j += 2;
            }
            let mut n: int = 8;
            while n < (live + 1) * 4 {
                n *= 2;
            }
            let mut t = zero_ints((n * 2) as usize);
            let mask = n - 1;
            j = 0;
            while j < old.len() {
                if old[j] > 0 {
                    let mut i = old[j + 1] & mask;
                    while t[(i * 2) as usize] != 0 {
                        i = (i + 1) & mask;
                    }
                    t[(i * 2) as usize] = old[j];
                    t[(i * 2 + 1) as usize] = old[j + 1];
                }
                j += 2;
            }
            self.colds[c].stab = t;
            self.colds[c].sused = live;
        }
        let t = &mut self.colds[c].stab;
        let mask = ((t.len() / 2) as int) - 1;
        let mut i = hash & mask;
        while t[(i * 2) as usize] > 0 {
            i = (i + 1) & mask;
        }
        if t[(i * 2) as usize] == 0 {
            self.colds[c].sused += 1;
        }
        let t = &mut self.colds[c].stab;
        t[(i * 2) as usize] = pos + 1;
        t[(i * 2 + 1) as usize] = hash;
    }

    pub fn map_find(&mut self, m: int, k: &Val) -> int {
        if let Val::Str(h) = k {
            let hash = jsstr::str_hash(self.sref(*h));
            let i = self.stab_slot(m, *h, hash);
            if i < 0 {
                return -1;
            }
            return self.colds[self.objs[m as usize].cold as usize].stab[(i * 2) as usize] - 1;
        }
        let key = self.map_key(k);
        match self.colds[self.objs[m as usize].cold as usize].index.get(&key) {
            Some(i) => *i,
            None => -1,
        }
    }

    pub fn map_set(&mut self, m: int, k: Val, v: Val) {
        let is_str = matches!(k, Val::Str(_));
        let key = if is_str { 0 } else { self.map_key(&k) };
        let hash = if let Val::Str(h) = &k { jsstr::str_hash(self.sref(*h)) } else { 0 };
        let found = if let Val::Str(h) = &k {
            let i = self.stab_slot(m, *h, hash);
            if i < 0 { -1 } else { self.colds[self.objs[m as usize].cold as usize].stab[(i * 2) as usize] - 1 }
        } else {
            match self.colds[self.objs[m as usize].cold as usize].index.get(&key) {
                Some(i) => *i,
                None => -1,
            }
        };
        if found >= 0 {
            self.put_elem2(m, found, v);
            return;
        }
        let pos = self.evecs[self.objs[m as usize].elems as usize].v.len() as int;
        let kk = if let Val::Num(n) = k {
            if n == 0.0 {
                Val::Num(0.0)
            } else {
                Val::Num(n)
            }
        } else {
            k
        };
        self.push_elem(m, kk);
        self.push_elem2(m, v);
        let c = self.cold_id(m) as usize;
        self.colds[c].marks.push(0);
        if is_str {
            self.stab_add(m, pos, hash);
        } else {
            self.colds[c].index.insert(key, pos);
        }
        self.objs[m as usize].pos += 1;
    }

    pub fn map_delete(&mut self, m: int, k: &Val) -> bool {
        let i = self.map_find(m, k);
        if i < 0 {
            return false;
        }
        let c = self.cold_id(m) as usize;
        if let Val::Str(h) = k {
            let hash = jsstr::str_hash(self.sref(*h));
            let slot = self.stab_slot(m, *h, hash);
            if slot >= 0 {
                self.colds[c].stab[(slot * 2) as usize] = -1;
            }
        } else {
            let key = self.map_key(k);
            self.colds[c].index.remove(&key);
        }
        self.colds[c].marks[i as usize] = 1;
        self.put_elem(m, i, Val::Undef);
        self.put_elem2(m, i, Val::Undef);
        self.objs[m as usize].pos -= 1;
        true
    }

    pub fn this_collection(&mut self, this: &Val, class: i32, name: &str) -> int {
        let o = obj_of(this);
        if o < 0 || self.objs[o as usize].class != class {
            self.throw_type(format!("Method {} called on incompatible receiver", name).as_str());
            return -1;
        }
        o
    }

    pub fn collection_iter(&mut self, m: int, kind: int) -> Val {
        // live over the entries (coll_next): ones added while iterating are
        // reached, deleted ones skipped; pos holds the kind (32 | 0 keys,
        // 1 values, 2 entries) and the map's clear count (coll_gen) above
        let is_set = self.objs[m as usize].class == C_SET;
        let ip = if is_set { self.set_iter_proto } else { self.map_iter_proto };
        let it = self.alloc(C_ITER, ip);
        self.put_env(it, m);
        self.objs[it as usize].func = 0;
        self.objs[it as usize].pos = (32 | kind | (self.coll_gen(m) << 8)) as i32;
        Val::Obj(it)
    }

    /// How many times Map / Set `m` was cleared (kept in its func: -1 at
    /// first, then 0, 2, 4...; 1 marks a weak one), 22 bits.
    pub fn coll_gen(&self, m: int) -> int {
        ((self.objs[m as usize].func as int) >> 1) & 4194303
    }

    /// The next entry of a Map / Set iterator (collection_iter): from
    /// position i the first one not deleted; after a clear, from the start.
    pub fn coll_next(&mut self, o: int, m: int, i0: int, kind: int) -> Option<Val> {
        let mut i = i0;
        let g = self.coll_gen(m);
        if (kind >> 8) != g {
            i = 0;
            self.objs[o as usize].pos = ((kind & 255) | (g << 8)) as i32;
        }
        let e = self.objs[m as usize].elems as usize;
        let c = self.objs[m as usize].cold as usize;
        let len = self.evecs[e].v.len() as int;
        while i < len && self.colds[c].marks[i as usize] != 0 {
            i += 1;
        }
        if i >= len {
            self.put_env(o, -1);
            return None;
        }
        self.objs[o as usize].func = (i + 1) as i32;
        let k = self.evecs[e].v[i as usize];
        let sub = kind & 3;
        if sub == 0 {
            return Some(k);
        }
        let v = if self.objs[m as usize].class == C_SET { k } else { self.colds[c].elems2[i as usize] };
        if sub == 1 {
            return Some(v);
        }
        let p = self.new_array(vec![k, v]);
        Some(Val::Obj(p))
    }

    // ---- property descriptors

    /// One attribute of a property descriptor as its bit: the descriptor's
    /// field when present, else the existing property's, else off (the bit
    /// set: hidden, read-only, fixed).
    pub fn desc_flag(&mut self, d: int, desc: &Val, name: int, bit: int, existing_attr: int) -> int {
        if self.has_atom(d, name) {
            let v = self.get_obj(d, name, desc);
            if self.truthy_v(&v) {
                0
            } else {
                bit
            }
        } else if existing_attr >= 0 {
            existing_attr & bit
        } else {
            bit
        }
    }

    /// ObjectDefineProperties: ToObject(props), then each own enumerable
    /// key's descriptor read before any is defined.
    pub fn define_props(&mut self, o: int, props: &Val) {
        let po = self.to_object(props);
        if self.throwing {
            return;
        }
        let pv = Val::Obj(po);
        let keys = self.own_keys(po, false, true);
        let mut descs: Vec<Val> = Vec::new();
        for k in keys.iter() {
            if self.throwing {
                break;
            }
            let d = self.get_elem(&pv, k);
            if self.throwing {
                break;
            }
            let dob = obj_of(&d);
            if dob < 0 || self.objs[dob as usize].class == C_SYMBOL || self.objs[dob as usize].class == C_BIGINT {
                self.throw_type("Property description must be an object");
                break;
            }
            self.temp_roots.push(d.clone());
            descs.push(d);
        }
        let mut i: usize = 0;
        while i < keys.len() && !self.throwing {
            let d = descs[i].clone();
            self.to_descriptor(o, &keys[i], &d);
            i += 1;
        }
        i = 0;
        while i < descs.len() {
            self.temp_roots.pop();
            i += 1;
        }
    }

    pub fn to_descriptor(&mut self, o: int, key: &Val, desc: &Val) {
        let c = self.objs[o as usize].class;
        if c != C_ARGUMENTS && c != C_SARGS {
            self.to_descriptor0(o, key, desc);
            return;
        }
        let (i, a) = self.to_key(key);
        if self.throwing {
            return;
        }
        let atom = if i >= 0 { self.index_atom(i) } else { a };
        if c == C_ARGUMENTS && (i >= 0 || atom == A_LENGTH) {
            self.slow_args(o);
        }
        let k = self.key_val(atom);
        let mapped = self.mapped_sync(o, atom);
        if mapped {
            let s0 = self.find(o, atom);
            let a0 = self.attr_at(o, s0);
            self.set_attr(o, s0, (a0 | P_MAPPED) - P_MAPPED);
        }
        self.to_descriptor0(o, &k, desc);
        if !mapped {
            return;
        }
        let slot = self.find(o, atom);
        if slot < 0 {
            return;
        }
        let at = self.attr_at(o, slot);
        if (at & P_ACCESSOR) != 0 {
            return;
        }
        // the value (given or kept) goes to the parameter; a read-only
        // element is no longer mapped
        let v = self.objs[o as usize].get_slot(slot as usize).clone();
        self.mapped_put(o, atom, &v);
        if (at & P_READONLY) == 0 {
            self.set_attr(o, slot, at | P_MAPPED);
        }
    }

    pub fn to_descriptor0(&mut self, o: int, key: &Val, desc: &Val) {
        let d = obj_of(desc);
        if d < 0 || self.objs[d as usize].class == C_SYMBOL || self.objs[d as usize].class == C_BIGINT {
            self.throw_type("Property description must be an object");
            return;
        }
        let (i, a) = self.to_key(key);
        let atom = if i >= 0 { self.index_atom(i) } else { a };
        if i >= 0 && (o == self.array_proto || o == self.object_proto) {
            self.idx_protos = true;
        }
        if atom == A_PROTOTYPE && self.objs[o as usize].class == C_FUNCTION && self.lazy_prototype(o) {
            // made now, so that a descriptor without a value keeps it
            self.function_prototype(o);
        }
        let has_get = self.has_atom(d, A_GET);
        let has_set = self.has_atom(d, A_SET);
        let get_v = self.get_obj(d, A_GET, desc);
        let set_v = self.get_obj(d, A_SET, desc);
        let has_value = {
            let a_value = A_VALUE;
            self.has_atom(d, a_value)
        };
        let value = self.get_obj(d, A_VALUE, desc);
        let has_writable = self.has_atom(d, A_WRITABLE);
        if (has_get || has_set) && (has_value || has_writable) {
            self.throw_type("Invalid property descriptor. Cannot both specify accessors and a value or writable attribute");
            return;
        }
        if i < 0 && self.objs[o as usize].class == C_TYPED && self.canon_numeric(atom) {
            self.throw_type("Cannot redefine property: not a typed array index");
            return;
        }
        if i >= 0 && self.objs[o as usize].class == C_TYPED {
            // a typed array's element (TypedArray [[DefineOwnProperty]])
            let bad_flag = |vm: &mut Vm, a: int| -> bool {
                if vm.has_atom(d, a) {
                    let f = vm.get_obj(d, a, desc);
                    return !vm.truthy_v(&f);
                }
                false
            };
            let nc = bad_flag(self, A_CONFIGURABLE);
            let ne = bad_flag(self, A_ENUMERABLE);
            let nw = bad_flag(self, A_WRITABLE);
            if i >= self.ta_length(o) || nc || ne || nw || has_get || has_set {
                self.throw_type("Cannot redefine property: typed array element");
                return;
            }
            if has_value {
                self.ta_set(o, i, &value);
            }
            return;
        }
        if i >= 0 && self.objs[o as usize].class == C_ARRAY && i >= self.evecs[self.objs[o as usize].elems as usize].v.len() as int && self.len_readonly(o) {
            self.throw_type(format!("Cannot define property {}, array length is read-only", i).as_str());
            return;
        }
        let slot = self.find(o, atom);
        let in_elems = i >= 0 && (self.objs[o as usize].class == C_ARRAY || self.objs[o as usize].class == C_ARGUMENTS) && (i as usize) < self.evecs[self.objs[o as usize].elems as usize].v.len();
        let plain_elem = slot < 0 && in_elems && self.objs[o as usize].class == C_ARRAY && !self.is_hole(o, i);
        let existing_attr = if slot >= 0 { self.attr_at(o, slot) } else if plain_elem { 0 } else { -1 };
        if slot < 0 && !in_elems && !self.objs[o as usize].extensible && !(self.objs[o as usize].class == C_ARRAY && atom == A_LENGTH) {
            let n = self.atom_str(atom);
            self.throw_type(format!("Cannot define property {}, object is not extensible", n).as_str());
            return;
        }
        if has_value && atom == A_LENGTH && self.objs[o as usize].class == C_ARRAY && slot >= 0 {
            // ArraySetLength: an invalid length is a RangeError first
            let n = self.to_number(&value);
            if self.throwing {
                return;
            }
            if n != n || n < 0.0 || n > 4294967295.0 || n.floor() != n {
                self.throw_range("Invalid array length");
                return;
            }
        }
        let hidden = self.desc_flag(d, desc, A_ENUMERABLE, P_HIDDEN, existing_attr);
        let fixed = self.desc_flag(d, desc, A_CONFIGURABLE, P_FIXED, existing_attr);
        if existing_attr >= 0 && (existing_attr & P_FIXED) != 0 {
            // a non-configurable property (ValidateAndApplyPropertyDescriptor)
            let n = self.atom_str(atom);
            let msg = format!("Cannot redefine property: {}", n);
            if fixed == 0 || (self.has_atom(d, A_ENUMERABLE) && hidden != (existing_attr & P_HIDDEN)) {
                self.throw_type(msg.as_str());
                return;
            }
            let was_accessor = (existing_attr & P_ACCESSOR) != 0;
            if was_accessor {
                if has_value || has_writable {
                    self.throw_type(msg.as_str());
                    return;
                }
                let pair = obj_of(&self.objs[o as usize].get_slot(slot as usize));
                let g0 = self.evecs[self.objs[pair as usize].elems as usize].v[0].clone();
                let s0 = self.evecs[self.objs[pair as usize].elems as usize].v[1].clone();
                if (has_get && !self.same_value(&get_v, &g0)) || (has_set && !self.same_value(&set_v, &s0)) {
                    self.throw_type(msg.as_str());
                    return;
                }
                return;
            }
            if has_get || has_set {
                self.throw_type(msg.as_str());
                return;
            }
            if (existing_attr & P_READONLY) != 0 {
                let wv = self.get_obj(d, A_WRITABLE, desc);
                if has_writable && self.truthy_v(&wv) {
                    self.throw_type(msg.as_str());
                    return;
                }
                let cur = self.objs[o as usize].get_slot(slot as usize).clone();
                if has_value && !self.same_value(&value, &cur) {
                    self.throw_type(msg.as_str());
                    return;
                }
                return;
            }
        }
        if (has_get || has_set) && i >= 0 && self.objs[o as usize].class == C_ARRAY && slot < 0 {
            // an accessor element: kept as a property
            if has_get && !matches!(get_v, Val::Undef) && !self.is_callable(&get_v) {
                self.throw_type("Getter must be a function");
                return;
            }
            if has_set && !matches!(set_v, Val::Undef) && !self.is_callable(&set_v) {
                self.throw_type("Setter must be a function");
                return;
            }
            self.elem_to_prop(o, i);
        }
        if has_get || has_set {
            if has_get {
                if !matches!(get_v, Val::Undef) && !self.is_callable(&get_v) {
                    self.throw_type("Getter must be a function");
                    return;
                }
                self.define_accessor(o, atom, get_v, 0, false);
            }
            if has_set {
                if !matches!(set_v, Val::Undef) && !self.is_callable(&set_v) {
                    self.throw_type("Setter must be a function");
                    return;
                }
                self.define_accessor(o, atom, set_v, 1, false);
            }
            let s2 = self.find(o, atom);
            self.set_attr(o, s2, P_ACCESSOR | hidden | fixed);
            return;
        }
        if !has_value && !has_writable && slot >= 0 && (existing_attr & P_ACCESSOR) != 0 {
            // a generic descriptor ({enumerable}, {configurable}): an accessor
            // stays one, with its getter and setter
            self.set_attr(o, slot, P_ACCESSOR | hidden | fixed);
            return;
        }
        let mut readonly = self.desc_flag(d, desc, A_WRITABLE, P_READONLY, existing_attr);
        if !has_writable && existing_attr >= 0 && (existing_attr & P_ACCESSOR) != 0 {
            // an accessor turned into a data property: writable defaults to false
            readonly = P_READONLY;
        }
        let class = self.objs[o as usize].class;
        if class == C_ARRAY && atom == A_LENGTH {
            if has_value {
                let n = self.to_number(&value);
                self.set_length(o, n);
                if !self.throwing && (self.evecs[self.objs[o as usize].elems as usize].v.len() as double) != n {
                    self.throw_type("Cannot redefine property: length");
                    return;
                }
            }
            if self.throwing {
                return;
            }
            if has_writable && readonly != 0 {
                // an own read-only length; writes go the slow way (pos 3)
                let n = self.evecs[self.objs[o as usize].elems as usize].v.len() as double;
                self.add_own(o, A_LENGTH, Val::Num(n), P_HIDDEN | P_READONLY | P_FIXED);
                if self.objs[o as usize].pos != 2 {
                    self.objs[o as usize].pos = 3;
                }
            }
            return;
        }
        if i >= 0 && class == C_ARRAY {
            if (hidden | readonly | fixed) == 0 {
                // a plain element (writable, enumerable, configurable)
                let v = if has_value {
                    value
                } else if plain_elem {
                    self.evecs[self.objs[o as usize].elems as usize].v[i as usize].clone()
                } else if slot >= 0 && (existing_attr & P_ACCESSOR) == 0 {
                    self.objs[o as usize].get_slot(slot as usize).clone()
                } else {
                    Val::Undef
                };
                if slot >= 0 {
                    self.remove_own(o, slot);
                    if (i as usize) < self.evecs[self.objs[o as usize].elems as usize].v.len() {
                        self.put_elem(o, i, v);
                        self.clear_elem_hole(o, i);
                        return;
                    }
                }
                self.set_index(o, i, v);
                return;
            }
            if slot < 0 {
                // an element with attributes: kept as a property
                let (cur, _) = self.elem_to_prop(o, i);
                let v = if has_value { value } else { cur };
                self.define(o, atom, v, hidden | readonly | fixed);
                return;
            }
        }
        let v = if has_value {
            value
        } else if slot >= 0 && (existing_attr & P_ACCESSOR) == 0 {
            self.objs[o as usize].get_slot(slot as usize).clone()
        } else {
            Val::Undef
        };
        self.define(o, atom, v, hidden | readonly | fixed);
    }

    pub fn from_descriptor(&mut self, o: int, key: &Val) -> Val {
        let (i, a) = self.to_key(key);
        let class = self.objs[o as usize].class;
        if class == C_TYPED && (i >= 0 || self.canon_numeric(a)) {
            if i < 0 || i >= self.ta_length(o) {
                return Val::Undef;
            }
            let v = self.ta_get(o, i);
            let d = self.new_object();
            self.add_own(d, A_VALUE, v, 0);
            self.add_own(d, A_WRITABLE, Val::Bool(true), 0);
            self.add_own(d, A_ENUMERABLE, Val::Bool(true), 0);
            self.add_own(d, A_CONFIGURABLE, Val::Bool(true), 0);
            return Val::Obj(d);
        }
        if class == C_SARGS && i >= 0 {
            let ma = self.index_atom(i);
            self.mapped_sync(o, ma);
        }
        let d = self.new_object();
        if i >= 0 && (class == C_ARRAY || class == C_ARGUMENTS) && (i as usize) < self.evecs[self.objs[o as usize].elems as usize].v.len() && !self.is_hole(o, i) {
            let v = self.evecs[self.objs[o as usize].elems as usize].v[i as usize].clone();
            self.add_own(d, A_VALUE, v, 0);
            self.add_own(d, A_WRITABLE, Val::Bool(true), 0);
            self.add_own(d, A_ENUMERABLE, Val::Bool(true), 0);
            self.add_own(d, A_CONFIGURABLE, Val::Bool(true), 0);
            return Val::Obj(d);
        }
        if i >= 0 && class == C_STRING {
            if let Val::Str(s) = self.colds[self.objs[o as usize].cold as usize].prim.clone() {
                let s = self.sget(s.clone());
                let c = jsstr::at(s.as_str(), i);
                if c >= 0 {
                    let sv5 = self.string_val(jsstr::from_unit(c));
                    self.add_own(d, A_VALUE, sv5, 0);
                    self.add_own(d, A_WRITABLE, Val::Bool(false), 0);
                    self.add_own(d, A_ENUMERABLE, Val::Bool(true), 0);
                    self.add_own(d, A_CONFIGURABLE, Val::Bool(false), 0);
                    return Val::Obj(d);
                }
            }
        }
        let atom = if i >= 0 { self.index_atom(i) } else { a };
        let slot = self.find(o, atom);
        if slot < 0 {
            if (class == C_ARRAY || class == C_STRING) && atom == A_LENGTH {
                let l = self.get_obj(o, A_LENGTH, &Val::Obj(o));
                self.add_own(d, A_VALUE, l, 0);
                self.add_own(d, A_WRITABLE, Val::Bool(class == C_ARRAY && self.objs[o as usize].pos != 2), 0);
                self.add_own(d, A_ENUMERABLE, Val::Bool(false), 0);
                self.add_own(d, A_CONFIGURABLE, Val::Bool(false), 0);
                return Val::Obj(d);
            }
            if (class == C_FUNCTION || class == C_BOUND) && (atom == A_LENGTH || atom == A_NAME || atom == A_PROTOTYPE) {
                let v = self.function_own(o, atom);
                if matches!(v, Val::Undef) {
                    return Val::Undef;
                }
                let is_proto = atom == A_PROTOTYPE;
                self.add_own(d, A_VALUE, v, 0);
                self.add_own(d, A_WRITABLE, Val::Bool(is_proto), 0);
                self.add_own(d, A_ENUMERABLE, Val::Bool(false), 0);
                self.add_own(d, A_CONFIGURABLE, Val::Bool(!is_proto), 0);
                return Val::Obj(d);
            }
            return Val::Undef;
        }
        let attr = self.attr_at(o, slot);
        if (attr & P_ACCESSOR) != 0 {
            let pair = obj_of(&self.objs[o as usize].get_slot(slot as usize));
            let g = self.evecs[self.objs[pair as usize].elems as usize].v[0].clone();
            let s = self.evecs[self.objs[pair as usize].elems as usize].v[1].clone();
            self.add_own(d, A_GET, g, 0);
            self.add_own(d, A_SET, s, 0);
        } else {
            let v = self.objs[o as usize].get_slot(slot as usize).clone();
            self.add_own(d, A_VALUE, v, 0);
            self.add_own(d, A_WRITABLE, Val::Bool((attr & P_READONLY) == 0), 0);
        }
        self.add_own(d, A_ENUMERABLE, Val::Bool((attr & P_HIDDEN) == 0), 0);
        self.add_own(d, A_CONFIGURABLE, Val::Bool((attr & P_FIXED) == 0), 0);
        Val::Obj(d)
    }

    // ---- sort

    pub fn sort_compare(&mut self, cmp: &Val, a: &Val, b: &Val) -> double {
        let ua = matches!(a, Val::Undef);
        let ub = matches!(b, Val::Undef);
        if ua && ub {
            return 0.0;
        }
        if ua {
            return 1.0;
        }
        if ub {
            return -1.0;
        }
        if !matches!(cmp, Val::Undef) {
            let r = self.call2(cmp, a, b);
            if let Val::Num(n) = r {
                return if is_nan(n) { 0.0 } else { n };
            }
            let n = self.to_number(&r);
            return if is_nan(n) { 0.0 } else { n };
        }
        if let (Val::Num(x), Val::Num(y)) = (a, b) {
            if x == y {
                return 0.0;
            }
            // numbers sort as their strings
            let sx = number_to_string(*x);
            let sy = number_to_string(*y);
            return jsstr::compare(sx.as_str(), sy.as_str()) as double;
        }
        let sa = self.to_str(a);
        let sb = self.to_str(b);
        jsstr::compare(sa.as_str(), sb.as_str()) as double
    }

    /// A stable merge sort; the comparator may throw.
    /// 2 when `cmp` is a function whose whole text is `(a, b) => a - b`
    /// (or `function (a, b) { return a - b; }`), 3 for `b - a`, else 1:
    /// on numbers merge_sort then needs no calls (kept per function).
    pub fn sort_kind(&mut self, cmp: &Val) -> i32 {
        let o = obj_of(cmp);
        if o < 0 || self.objs[o as usize].class != C_FUNCTION {
            return 1;
        }
        let pi = self.objs[o as usize].func as usize;
        while self.cmp_kinds.len() <= pi {
            self.cmp_kinds.push(0);
        }
        if self.cmp_kinds[pi] != 0 {
            return self.cmp_kinds[pi];
        }
        let mut k: i32 = 1;
        if !self.protos[pi].generator && !self.protos[pi].is_async {
            let (a, b) = (self.protos[pi].src_start, self.protos[pi].src_end);
            // the forms are short: a longer text is not one of them
            if b - a > 64 {
                self.cmp_kinds[pi] = 1;
                return 1;
            }
            if let Val::Str(h) = &self.protos[pi].src {
                let src = self.sget(*h);
                let text = jsstr::char_range(src.as_str(), a, b);
                k = sub_cmp_kind(squeeze_ws(text.as_str()).as_str());
            }
        }
        self.cmp_kinds[pi] = k;
        k
    }

    pub fn merge_sort(&mut self, v: &mut Vec<Val>, cmp: &Val) {
        let n = v.len();
        if n < 2 {
            return;
        }
        let kind = self.sort_kind(cmp);
        if kind >= 2 && all_nums(v) {
            num_merge_sort(v, kind == 3);
            self.check_expired();
            return;
        }
        if matches!(cmp, Val::Undef) && all_nums(v) {
            num_str_merge_sort(v);
            self.check_expired();
            return;
        }
        let mut src: Vec<Val> = v.clone();
        let mut dst: Vec<Val> = v.clone();
        let mut width: usize = 1;
        while width < n {
            let mut lo: usize = 0;
            while lo < n {
                let mid = if lo + width < n { lo + width } else { n };
                let hi = if lo + 2 * width < n { lo + 2 * width } else { n };
                let mut i = lo;
                let mut j = mid;
                let mut k = lo;
                while i < mid && j < hi {
                    let c = self.sort_compare(cmp, &src[j], &src[i]);
                    if self.throwing || self.poll(64) {
                        return;
                    }
                    if c < 0.0 {
                        dst[k] = src[j].clone();
                        j += 1;
                    } else {
                        dst[k] = src[i].clone();
                        i += 1;
                    }
                    k += 1;
                }
                while i < mid {
                    dst[k] = src[i].clone();
                    i += 1;
                    k += 1;
                }
                while j < hi {
                    dst[k] = src[j].clone();
                    j += 1;
                    k += 1;
                }
                lo += 2 * width;
            }
            let t = src;
            src = dst;
            dst = t;
            width *= 2;
        }
        *v = src;
    }

    // ---- JSON

    pub fn json_quote(&self, s: &str, out: &mut String) {
        out.push('"');
        let mut plain = true;
        for c in s.chars() {
            if c == '"' || c == '\\' || (c as int) < 32 || (c as int) >= jsstr::LONE {
                plain = false;
                break;
            }
        }
        if plain {
            out.push_str(s);
            out.push('"');
            return;
        }
        // a lone surrogate as an escape (well-formed JSON.stringify); a
        // 0xdbfe/0xdbff pair kept as two stand-ins stays so
        let mut hold: int = -1;
        for c in s.chars() {
            let lu = jsstr::lone_unit(c as int);
            if hold >= 0 {
                if lu >= 0xdc00 {
                    // a pair: as it is (the engine keeps U+10F800.. so)
                    jsstr::push_cp(out, hold - 0xd800 + jsstr::LONE);
                    out.push(c);
                    hold = -1;
                    continue;
                }
                out.push_str("\\u");
                out.push_str(hex_digits(hold, 4, false).as_str());
                hold = -1;
            }
            if lu >= 0 {
                if lu >= 0xdbfe && lu <= 0xdbff {
                    hold = lu;
                } else {
                    out.push_str("\\u");
                    out.push_str(hex_digits(lu, 4, false).as_str());
                }
                continue;
            }
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                _ => {
                    let cp = c as int;
                    if cp < 32 {
                        out.push_str("\\u");
                        out.push_str(hex_digits(cp, 4, false).as_str());
                    } else {
                        out.push(c);
                    }
                }
            }
        }
        if hold >= 0 {
            out.push_str("\\u");
            out.push_str(hex_digits(hold, 4, false).as_str());
        }
        out.push('"');
    }

    /// The JSON.stringify plan of ordinary object `o` (index of its triple
    /// in js_plan), -1 when it has none: a shared (never changing) shape
    /// of data properties only, Object.prototype its prototype and neither
    /// with toJSON (kept while shape_epoch and Object.prototype's shape stay).
    /// Its values are read straight from their slots while
    /// `o` keeps that shape.
    pub fn json_plan(&mut self, o: int) -> int {
        let op = self.object_proto;
        if self.objs[o as usize].class != C_OBJECT || self.objs[o as usize].elems != 0 || (self.objs[o as usize].proto as int) != op {
            return -1;
        }
        let sh = self.objs[o as usize].shape as int;
        if self.shapes[sh as usize].owned {
            return -1;
        }
        // Object.prototype's own keys: its shape, or shape_epoch when it
        // changes an owned one in place
        let ps = self.objs[op as usize].shape as int;
        if self.js_epoch != self.shape_epoch || self.js_pshape != ps {
            self.js_plan.clear();
            self.js_epoch = self.shape_epoch;
            self.js_pshape = ps;
        }
        let mut i: usize = 0;
        while i < self.js_plan.len() {
            if self.js_plan[i] as int == sh {
                return if self.js_plan[i + 1] < 0 { -1 } else { i as int };
            }
            i += 3;
        }
        if self.js_plan.len() >= 24 || self.js_slots.len() > 4096 {
            return -1;
        }
        let at = self.js_plan.len();
        self.js_plan.push(sh as i32);
        self.js_plan.push(-1);
        self.js_plan.push(0);
        if (self.objs[op as usize].proto as int) >= 0 || self.find(op, A_TOJSON) >= 0 || self.find(o, A_TOJSON) >= 0 {
            return -1;
        }
        let atoms = self.own_string_atoms(o);
        for a in atoms.iter() {
            let slot = self.find(o, *a);
            if slot < 0 || (self.attr_at(o, slot) & (P_ACCESSOR | P_MAPPED)) != 0 {
                return -1;
            }
        }
        let start = self.js_slots.len();
        for a in atoms.iter() {
            let slot = self.find(o, *a);
            self.js_slots.push(slot as i32);
            self.js_atoms.push(*a as i32);
            let mut q = String::new();
            self.json_quote(self.atom_names[*a as usize].as_str(), &mut q);
            self.js_kq.push(q);
        }
        self.js_plan[at + 1] = start as i32;
        self.js_plan[at + 2] = atoms.len() as i32;
        at as int
    }

    /// SerializeJSONProperty steps 1-4: toJSON, the replacer and unwrapping of
    /// Number/String/Boolean objects. `kidx` >= 0 is an array index whose key
    /// string is made only when toJSON or the replacer needs it.
    pub fn json_val(&mut self, holder: &Val, key0: &Val, kidx: int, v0: Val, replacer: &Val) -> Val {
        let mut v = v0;
        // an object with a plan has no toJSON
        if let Val::Obj(o) = &v {
            if !self.is_callable(replacer) && self.json_plan(*o) >= 0 {
                return v;
            }
        }
        // an array index key is made only when toJSON or a replacer needs it
        let tj = if is_obj(&v) || matches!(v, Val::Big(_)) { self.get(&v, A_TOJSON) } else { Val::Undef };
        let has_tj = self.is_callable(&tj);
        let mut key = key0.clone();
        if kidx >= 0 && (has_tj || self.is_callable(replacer)) {
            key = self.string_val(format!("{}", kidx));
        }
        let key = &key;
        if is_obj(&v) || matches!(v, Val::Num(_) | Val::Big(_)) {
            if has_tj {
                v = self.call_fixed(tj, v.clone(), 1, key.clone(), Val::Undef, Val::Undef);
                if self.throwing {
                    return Val::Undef;
                }
            }
        }
        if self.is_callable(replacer) {
            v = self.call_fixed(replacer.clone(), holder.clone(), 2, key.clone(), v.clone(), Val::Undef);
            if self.throwing {
                return Val::Undef;
            }
        }
        if let Val::Obj(o) = &v {
            let c = self.objs[*o as usize].class;
            if c == C_NUMBER || c == C_STRING || c == C_BOOLEAN {
                v = if c == C_NUMBER {
                    let n = self.to_number(&v);
                    Val::Num(n)
                } else if c == C_STRING {
                    let s = self.to_str(&v);
                    self.rc_val(s)
                } else {
                    self.colds[self.objs[*o as usize].cold as usize].prim.clone()
                };
            }
        }
        v
    }

    /// true when the value is left out of the output (undefined, a function or a symbol)
    pub fn json_skip(&self, v: &Val) -> bool {
        match v {
            Val::Undef => true,
            Val::Obj(o) => self.is_callable(v) || self.objs[*o as usize].class == C_SYMBOL,
            _ => false,
        }
    }

    pub fn json_str(&mut self, holder: &Val, key0: &Val, kidx: int, v0: Val, replacer: &Val, allow: &Vec<String>, gap: &str, indent: &str, stack: &mut Vec<int>, out: &mut String) -> bool {
        let v = self.json_val(holder, key0, kidx, v0, replacer);
        if self.throwing || self.json_skip(&v) {
            return false;
        }
        self.json_write(v, replacer, allow, gap, indent, stack, out)
    }

    /// writes a value json_skip does not leave out; false only when throwing
    pub fn json_write(&mut self, v: Val, replacer: &Val, allow: &Vec<String>, gap: &str, indent: &str, stack: &mut Vec<int>, out: &mut String) -> bool {
        if self.poll(16) {
            return false;
        }
        match &v {
            Val::Null => {
                out.push_str("null");
                return true;
            }
            Val::Bool(b) => {
                out.push_str(if *b { "true" } else { "false" });
                return true;
            }
            Val::Str(s) => {
                let s = self.sget(s.clone());
                self.json_quote(s.as_str(), out);
                return true;
            }
            Val::Num(n) => {
                if push_small_int(out, *n) >= 0 {
                    return true;
                }
                if is_finite(*n) {
                    out.push_str(number_to_string(*n).as_str());
                } else {
                    out.push_str("null");
                }
                return true;
            }
            Val::Undef => return false,
            Val::Big(_) => {
                self.throw_type("Do not know how to serialize a BigInt");
                return false;
            }
            Val::Obj(o) => {
                let o = *o;
                if self.is_callable(&v) || self.objs[o as usize].class == C_SYMBOL {
                    return false;
                }
                // objects and arrays nest (stack holds them): the native
                // stack is looked at every 32 levels
                if (stack.len() & 31) == 31 && self.stack_exhausted() {
                    self.throw_range("Maximum call stack size exceeded");
                    return false;
                }
                if self.objs[o as usize].class == C_BIGINT {
                    self.throw_type("Do not know how to serialize a BigInt");
                    return false;
                }
                for s in stack.iter() {
                    if *s == o {
                        self.throw_type("Converting circular structure to JSON");
                        return false;
                    }
                }
                stack.push(o);
                let inner = if gap.is_empty() { String::new() } else { format!("{}{}", indent, gap) };
                let class = self.objs[o as usize].class;
                let plan = if class == C_OBJECT && allow.is_empty() && !self.is_callable(replacer) { self.json_plan(o) } else { -1 };
                if plan >= 0 {
                    // the plan's keys, values from their slots while o keeps
                    // its shape (a toJSON below may change it)
                    let sh = self.js_plan[plan as usize] as int;
                    let start = self.js_plan[plan as usize + 1] as usize;
                    let len = self.js_plan[plan as usize + 2] as usize;
                    out.push('{');
                    let mut first = true;
                    let mut j: usize = start;
                    while j < start + len {
                        let a = self.js_atoms[j] as int;
                        let item = if (self.objs[o as usize].shape as int) == sh {
                            self.objs[o as usize].get_slot(self.js_slots[j] as usize).clone()
                        } else {
                            self.get(&v, a)
                        };
                        if self.throwing {
                            return false;
                        }
                        let iv = if is_obj(&item) || matches!(item, Val::Big(_)) {
                            let kv = self.atom_val(a);
                            self.json_val(&v, &kv, -1, item, replacer)
                        } else {
                            item
                        };
                        if self.throwing {
                            return false;
                        }
                        j += 1;
                        if self.json_skip(&iv) {
                            continue;
                        }
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        if !gap.is_empty() {
                            out.push('\n');
                            out.push_str(inner.as_str());
                        }
                        out.push_str(self.js_kq[j - 1].as_str());
                        out.push(':');
                        if !gap.is_empty() {
                            out.push(' ');
                        }
                        if !self.json_write(iv, replacer, allow, gap, inner.as_str(), stack, out) {
                            return false;
                        }
                    }
                    if !first && !gap.is_empty() {
                        out.push('\n');
                        out.push_str(indent);
                    }
                    out.push('}');
                } else if class == C_ARRAY || (class == C_PROXY && self.is_array_val(&v)) {
                    let n = self.len_of(&v);
                    out.push('[');
                    let mut i: int = 0;
                    while i < n {
                        if i > 0 {
                            out.push(',');
                        }
                        if !gap.is_empty() {
                            out.push('\n');
                            out.push_str(inner.as_str());
                        }
                        let item = self.get_index(&v, i);
                        let iv = self.json_val(&v, &Val::Undef, i, item, replacer);
                        if self.throwing {
                            return false;
                        }
                        if self.json_skip(&iv) {
                            out.push_str("null");
                        } else if !self.json_write(iv, replacer, allow, gap, inner.as_str(), stack, out) {
                            return false;
                        }
                        i += 1;
                    }
                    if n > 0 && !gap.is_empty() {
                        out.push('\n');
                        out.push_str(indent);
                    }
                    out.push(']');
                } else if allow.is_empty() && self.objs[o as usize].class == C_OBJECT && self.objs[o as usize].elems == 0 {
                    // an ordinary object: its keys as atoms, no key strings
                    // unless toJSON or the replacer is given one
                    let atoms = self.own_string_atoms(o);
                    let rep = self.is_callable(replacer);
                    out.push('{');
                    let mut first = true;
                    for a in atoms {
                        let item = self.get(&v, a);
                        if self.throwing {
                            return false;
                        }
                        let kv = if rep || is_obj(&item) { self.atom_val(a) } else { Val::Undef };
                        let iv = self.json_val(&v, &kv, -1, item, replacer);
                        if self.throwing {
                            return false;
                        }
                        if self.json_skip(&iv) {
                            continue;
                        }
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        if !gap.is_empty() {
                            out.push('\n');
                            out.push_str(inner.as_str());
                        }
                        self.json_quote(self.atom_names[a as usize].as_str(), out);
                        out.push(':');
                        if !gap.is_empty() {
                            out.push(' ');
                        }
                        if !self.json_write(iv, replacer, allow, gap, inner.as_str(), stack, out) {
                            return false;
                        }
                    }
                    if !first && !gap.is_empty() {
                        out.push('\n');
                        out.push_str(indent);
                    }
                    out.push('}');
                } else {
                    let keys = if allow.is_empty() {
                        self.own_keys(o, false, false)
                    } else {
                        let mut ks: Vec<Val> = Vec::new();
                        for a in allow.iter() {
                            ks.push(self.str_val(a.as_str()));
                        }
                        ks
                    };
                    out.push('{');
                    let mut first = true;
                    for k in keys {
                        let item = self.get_elem(&v, &k);
                        if self.throwing {
                            return false;
                        }
                        let iv = self.json_val(&v, &k, -1, item, replacer);
                        if self.throwing {
                            return false;
                        }
                        if self.json_skip(&iv) {
                            continue;
                        }
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        if !gap.is_empty() {
                            out.push('\n');
                            out.push_str(inner.as_str());
                        }
                        let ks = self.to_string(&k);
                        self.json_quote(ks.as_str(), out);
                        out.push(':');
                        if !gap.is_empty() {
                            out.push(' ');
                        }
                        if !self.json_write(iv, replacer, allow, gap, inner.as_str(), stack, out) {
                            return false;
                        }
                    }
                    if !first && !gap.is_empty() {
                        out.push('\n');
                        out.push_str(indent);
                    }
                    out.push('}');
                }
                stack.pop();
                true
            }
        }
    }

    pub fn json_parse_value(&mut self, cs: &Vec<char>, pos: &mut usize) -> Val {
        self.json_ws(cs, pos);
        if *pos >= cs.len() {
            self.throw_syntax("Unexpected end of JSON input");
            return Val::Undef;
        }
        let c = cs[*pos];
        if (c == '{' || c == '[') && self.poll(64) {
            return Val::Undef;
        }
        if (c == '{' || c == '[') && self.stack_exhausted() {
            self.throw_range("Maximum call stack size exceeded");
            return Val::Undef;
        }
        if c == '{' {
            *pos += 1;
            let o = self.new_object();
            self.temp_roots.push(Val::Obj(o));
            self.json_ws(cs, pos);
            if *pos < cs.len() && cs[*pos] == '}' {
                *pos += 1;
                self.temp_roots.pop();
                return Val::Obj(o);
            }
            loop {
                self.json_ws(cs, pos);
                if *pos >= cs.len() || cs[*pos] != '"' {
                    self.throw_syntax("Unexpected token in JSON");
                    self.temp_roots.pop();
                    return Val::Undef;
                }
                let k = self.json_string(cs, pos);
                if self.throwing {
                    self.temp_roots.pop();
                    return Val::Undef;
                }
                self.json_ws(cs, pos);
                if *pos >= cs.len() || cs[*pos] != ':' {
                    self.throw_syntax("Expected ':' in JSON");
                    self.temp_roots.pop();
                    return Val::Undef;
                }
                *pos += 1;
                let v = self.json_parse_value(cs, pos);
                if self.throwing {
                    self.temp_roots.pop();
                    return Val::Undef;
                }
                // the key straight to its atom: no string value in between
                let ki = jsstr::array_index(k.as_str());
                let atom = if ki >= 0 { self.index_atom(ki) } else { self.intern(k.as_str()) };
                self.define(o, atom, v, 0);
                self.json_ws(cs, pos);
                if *pos < cs.len() && cs[*pos] == ',' {
                    *pos += 1;
                    continue;
                }
                if *pos < cs.len() && cs[*pos] == '}' {
                    *pos += 1;
                    break;
                }
                self.throw_syntax("Unexpected token in JSON");
                self.temp_roots.pop();
                return Val::Undef;
            }
            self.temp_roots.pop();
            return Val::Obj(o);
        }
        if c == '[' {
            *pos += 1;
            let a = self.new_array(Vec::new());
            self.temp_roots.push(Val::Obj(a));
            self.json_ws(cs, pos);
            if *pos < cs.len() && cs[*pos] == ']' {
                *pos += 1;
                self.temp_roots.pop();
                return Val::Obj(a);
            }
            loop {
                let v = self.json_parse_value(cs, pos);
                if self.throwing {
                    self.temp_roots.pop();
                    return Val::Undef;
                }
                self.push_elem(a, v);
                self.json_ws(cs, pos);
                if *pos < cs.len() && cs[*pos] == ',' {
                    *pos += 1;
                    continue;
                }
                if *pos < cs.len() && cs[*pos] == ']' {
                    *pos += 1;
                    break;
                }
                self.throw_syntax("Unexpected token in JSON");
                self.temp_roots.pop();
                return Val::Undef;
            }
            self.temp_roots.pop();
            return Val::Obj(a);
        }
        if c == '"' {
            let s = self.json_string(cs, pos);
            return self.string_val(s);
        }
        if c == 't' || c == 'f' || c == 'n' {
            let n = cs.len();
            let p = *pos;
            if c == 't' && p + 4 <= n && cs[p + 1] == 'r' && cs[p + 2] == 'u' && cs[p + 3] == 'e' {
                *pos += 4;
                return Val::Bool(true);
            }
            if c == 'f' && p + 5 <= n && cs[p + 1] == 'a' && cs[p + 2] == 'l' && cs[p + 3] == 's' && cs[p + 4] == 'e' {
                *pos += 5;
                return Val::Bool(false);
            }
            if c == 'n' && p + 4 <= n && cs[p + 1] == 'u' && cs[p + 2] == 'l' && cs[p + 3] == 'l' {
                *pos += 4;
                return Val::Null;
            }
            self.throw_syntax("Unexpected token in JSON");
            return Val::Undef;
        }
        if c == '-' || (c >= '0' && c <= '9') {
            let start = *pos;
            if cs[*pos] == '-' {
                *pos += 1;
            }
            if *pos < cs.len() && cs[*pos] == '0' && *pos + 1 < cs.len() && cs[*pos + 1] >= '0' && cs[*pos + 1] <= '9' {
                self.throw_syntax("Unexpected number in JSON");
                return Val::Undef;
            }
            // an integer of up to 15 digits is exact as it is summed
            let d0 = *pos;
            let mut iv: double = 0.0;
            while *pos < cs.len() && cs[*pos] >= '0' && cs[*pos] <= '9' {
                iv = iv * 10.0 + (((cs[*pos] as int) - 48) as double);
                *pos += 1;
            }
            let nd = *pos - d0;
            if nd > 0 && nd <= 15 && (*pos >= cs.len() || !(cs[*pos] == '.' || cs[*pos] == 'e' || cs[*pos] == 'E' || cs[*pos] == '+' || cs[*pos] == '-')) {
                return Val::Num(if cs[start] == '-' { -iv } else { iv });
            }
            while *pos < cs.len() && ((cs[*pos] >= '0' && cs[*pos] <= '9') || cs[*pos] == '.' || cs[*pos] == 'e' || cs[*pos] == 'E' || cs[*pos] == '+' || cs[*pos] == '-') {
                *pos += 1;
            }
            let mut t = String::new();
            let mut i = start;
            while i < *pos {
                t.push(cs[i]);
                i += 1;
            }
            // a point needs a digit after it ("1." and "1.e3" are not JSON)
            let dot = jsstr::byte_index_of(t.as_str(), ".", 0);
            let mut bad_dot = false;
            if dot >= 0 {
                let tl = t.as_bytes().len() as int;
                if dot + 1 >= tl {
                    bad_dot = true;
                } else {
                    let after = jsstr::byte_at(t.as_str(), dot + 1);
                    bad_dot = after < 48 || after > 57;
                }
            }
            let n = if bad_dot { nan() } else { string_to_number(t.as_str()) };
            if is_nan(n) {
                self.throw_syntax("Unexpected number in JSON");
                return Val::Undef;
            }
            return Val::Num(n);
        }
        self.throw_syntax(format!("Unexpected token '{}' in JSON", c).as_str());
        Val::Undef
    }

    pub fn json_ws(&self, cs: &Vec<char>, pos: &mut usize) {
        while *pos < cs.len() && (cs[*pos] == ' ' || cs[*pos] == '\t' || cs[*pos] == '\n' || cs[*pos] == '\r') {
            *pos += 1;
        }
    }

    pub fn json_string(&mut self, cs: &Vec<char>, pos: &mut usize) -> String {
        *pos += 1;
        let mut s = String::new();
        while *pos < cs.len() {
            let c = cs[*pos];
            if c == '"' {
                *pos += 1;
                return s;
            }
            if (c as int) < 32 {
                self.throw_syntax("Bad control character in string literal in JSON");
                return s;
            }
            if c == '\\' {
                *pos += 1;
                if *pos >= cs.len() {
                    break;
                }
                let e = cs[*pos];
                *pos += 1;
                match e {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'r' => s.push('\r'),
                    'b' => s.push('\u{8}'),
                    'f' => s.push('\u{c}'),
                    '/' => s.push('/'),
                    '\\' => s.push('\\'),
                    '"' => s.push('"'),
                    'u' => {
                        let mut v: int = 0;
                        let mut k = 0;
                        while k < 4 && *pos < cs.len() {
                            let h = cs[*pos];
                            let d = if h >= '0' && h <= '9' {
                                (h as int) - 48
                            } else if h >= 'a' && h <= 'f' {
                                (h as int) - 87
                            } else if h >= 'A' && h <= 'F' {
                                (h as int) - 55
                            } else {
                                -1
                            };
                            if d < 0 {
                                self.throw_syntax("Bad Unicode escape in JSON");
                                return s;
                            }
                            v = v * 16 + d;
                            *pos += 1;
                            k += 1;
                        }
                        // a surrogate pair written as two escapes
                        if v >= 0xd800 && v <= 0xdbff && *pos + 5 < cs.len() && cs[*pos] == '\\' && cs[*pos + 1] == 'u' {
                            let mut lo: int = 0;
                            let mut j = 2;
                            let mut ok = true;
                            while j < 6 {
                                let h = cs[*pos + j];
                                let d = if h >= '0' && h <= '9' {
                                    (h as int) - 48
                                } else if h >= 'a' && h <= 'f' {
                                    (h as int) - 87
                                } else if h >= 'A' && h <= 'F' {
                                    (h as int) - 55
                                } else {
                                    -1
                                };
                                if d < 0 {
                                    ok = false;
                                    break;
                                }
                                lo = lo * 16 + d;
                                j += 1;
                            }
                            if ok && lo >= 0xdc00 && lo <= 0xdfff {
                                *pos += 6;
                                v = 0x10000 + ((v - 0xd800) << 10) + (lo - 0xdc00);
                            }
                        }
                        jsstr::push_code(&mut s, v);
                    }
                    _ => {
                        self.throw_syntax("Bad escaped character in JSON");
                        return s;
                    }
                }
                continue;
            }
            s.push(c);
            *pos += 1;
        }
        self.throw_syntax("Unterminated string in JSON");
        s
    }

    pub fn json_revive(&mut self, holder: &Val, key: &Val, reviver: &Val) -> Val {
        let v = self.get_elem(holder, key);
        if let Val::Obj(o) = &v {
            let keys = self.own_keys(*o, false, false);
            for k in keys {
                let nv = self.json_revive(&v, &k, reviver);
                if self.throwing {
                    return Val::Undef;
                }
                if matches!(nv, Val::Undef) {
                    self.delete(*o, &k);
                } else {
                    self.define_elem(*o, &k, nv, 0);
                }
            }
        }
        self.call_fixed(reviver.clone(), holder.clone(), 2, key.clone(), v, Val::Undef)
    }

    // ---- Date

    pub fn date_value(&mut self, this: &Val) -> double {
        let o = obj_of(this);
        if o < 0 || self.objs[o as usize].class != C_DATE {
            self.throw_type("this is not a Date object.");
            return nan();
        }
        match self.colds[self.objs[o as usize].cold as usize].prim {
            Val::Num(n) => n,
            _ => nan(),
        }
    }

    pub fn make_time(fields: &Vec<double>) -> double {
        // [year, month, day, h, m, s, ms]
        for f in fields.iter() {
            if !is_finite(*f) {
                return nan();
            }
        }
        let y = to_integer(fields[0]);
        let m = to_integer(fields[1]);
        let ym = y + (m / 12.0).floor();
        let mn = m - (m / 12.0).floor() * 12.0;
        let days = days_from_civil(ym as int, (mn as int) + 1, 1) as double + to_integer(fields[2]) - 1.0;
        let t = days * 86400000.0 + to_integer(fields[3]) * 3600000.0 + to_integer(fields[4]) * 60000.0 + to_integer(fields[5]) * 1000.0 + to_integer(fields[6]);
        if t.abs() > 8.64e15 {
            return nan();
        }
        t
    }

    pub fn time_fields(t: double) -> Vec<double> {
        // days and the time of day in doubles: ms since 1970 do not fit a
        // 32-bit int
        let dayd = (t / 86400000.0).floor();
        let day = dayd as int;
        let rem = (t - dayd * 86400000.0) as int;
        let (y, m, d) = civil_from_days(day);
        let wd = ((day % 7) + 11) % 7;
        vec![
            y as double,
            (m - 1) as double,
            d as double,
            wd as double,
            (rem / 3600000) as double,
            ((rem / 60000) % 60) as double,
            ((rem / 1000) % 60) as double,
            (rem % 1000) as double,
        ]
    }

    pub fn iso_string(t: double) -> String {
        let f = Vm::time_fields(t);
        let y = f[0] as int;
        let ys = if y >= 0 && y <= 9999 {
            format!("{:04}", y)
        } else if y < 0 {
            format!("-{:06}", -y)
        } else {
            format!("+{:06}", y)
        };
        format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", ys, (f[1] as int) + 1, f[2] as int, f[4] as int, f[5] as int, f[6] as int, f[7] as int)
    }

    pub fn parse_date(s: &str) -> double {
        // ISO forms: YYYY, YYYY-MM, YYYY-MM-DD, with THH:mm(:ss(.sss))(Z|±hh:mm)
        let cs = s.trim().chars().collect::<Vec<char>>();
        let n = cs.len();
        let mut i: usize = 0;
        let mut nums: Vec<double> = vec![1970.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let mut sign: int = 1;
        let y: int;
        if i < n && (cs[i] == '+' || cs[i] == '-') {
            sign = if cs[i] == '-' { -1 } else { 1 };
            i += 1;
            y = date_digits(&cs, i, 6);
            i += 6;
        } else {
            y = date_digits(&cs, i, 4);
            i += 4;
        }
        if y < 0 {
            return nan();
        }
        nums[0] = (sign * y) as double;
        if i < n && cs[i] == '-' {
            i += 1;
            let m = date_digits(&cs, i, 2);
            i += 2;
            if m < 1 || m > 12 {
                return nan();
            }
            nums[1] = (m - 1) as double;
            if i < n && cs[i] == '-' {
                i += 1;
                let d = date_digits(&cs, i, 2);
                i += 2;
                if d < 1 || d > 31 {
                    return nan();
                }
                nums[2] = d as double;
            }
        }
        let mut offset: double = 0.0;
        if i < n && (cs[i] == 'T' || cs[i] == ' ') {
            i += 1;
            let h = date_digits(&cs, i, 2);
            i += 2;
            if h < 0 || i >= n || cs[i] != ':' {
                return nan();
            }
            i += 1;
            let mi = date_digits(&cs, i, 2);
            i += 2;
            if mi < 0 {
                return nan();
            }
            nums[3] = h as double;
            nums[4] = mi as double;
            if i < n && cs[i] == ':' {
                i += 1;
                let sec = date_digits(&cs, i, 2);
                i += 2;
                if sec < 0 {
                    return nan();
                }
                nums[5] = sec as double;
                if i < n && cs[i] == '.' {
                    i += 1;
                    let mut frac: double = 0.0;
                    let mut scale: double = 100.0;
                    while i < n && cs[i] >= '0' && cs[i] <= '9' {
                        frac += (((cs[i] as int) - 48) as double) * scale;
                        scale /= 10.0;
                        i += 1;
                    }
                    nums[6] = frac.floor();
                }
            }
            if i < n && cs[i] == 'Z' {
                i += 1;
            } else if i < n && (cs[i] == '+' || cs[i] == '-') {
                let sg: double = if cs[i] == '-' { -1.0 } else { 1.0 };
                i += 1;
                let oh = date_digits(&cs, i, 2);
                i += 2;
                if i < n && cs[i] == ':' {
                    i += 1;
                }
                let om = date_digits(&cs, i, 2);
                i += 2;
                if oh < 0 || om < 0 {
                    return nan();
                }
                offset = sg * ((oh * 60 + om) as double) * 60000.0;
            }
        }
        if i != n {
            return nan();
        }
        Vm::make_time(&nums) - offset
    }

    // ---- promises

    pub fn new_promise(&mut self) -> int {
        let pp = self.promise_proto;
        let p = self.alloc(C_PROMISE, pp);
        // pos: 0 pending, 1 fulfilled, 2 rejected; prim: the value;
        // elems: reactions [onFulfilled, onRejected, derived promise]*
        p
    }

    pub fn settle(&mut self, p: int, state: int, v: Val) {
        if self.objs[p as usize].pos != 0 {
            return;
        }
        if state == 1 {
            // resolving with a thenable adopts its state
            if let Val::Obj(o) = &v {
                if *o == p {
                    let e = self.new_error(self.type_error_proto, "Chaining cycle detected for promise");
                    self.settle(p, 2, Val::Obj(e));
                    return;
                }
                let then = self.get(&v, A_THEN);
                if self.throwing {
                    self.throwing = false;
                    let e = self.exc.clone();
                    self.settle(p, 2, e);
                    return;
                }
                if self.is_callable(&then) {
                    let (res, rej) = self.resolving_functions(p);
                    self.jobs.push(then);
                    self.jobs.push(v.clone());
                    self.jobs.push(Val::Obj(res));
                    self.jobs.push(Val::Obj(rej));
                    return;
                }
            }
        }
        self.objs[p as usize].pos = state as i32;
        self.put_prim(p, v.clone());
        let reactions = self.evecs[self.objs[p as usize].elems as usize].v.clone();
        self.put_elems(p, Vec::new());
        let mut i: usize = 0;
        while i + 2 < reactions.len() {
            self.queue_reaction(reactions[i].clone(), reactions[i + 1].clone(), reactions[i + 2].clone(), state, v.clone());
            i += 3;
        }
    }

    pub fn resolving_functions(&mut self, p: int) -> (int, int) {
        let res = self.native_fn("", NF_PR_RESOLVE_FN, 1);
        self.put_env(res, p);
        let rej = self.native_fn("", NF_PR_REJECT_FN, 1);
        self.put_env(rej, p);
        // shared "already resolved" flag: both point at one cell
        let cell = self.alloc(C_OBJECT, -1);
        self.put_home(res, cell);
        self.put_home(rej, cell);
        (res, rej)
    }

    pub fn queue_reaction(&mut self, on_ok: Val, on_err: Val, derived: Val, state: int, v: Val) {
        // a job: [handler, value, derived promise, state]
        let h = if state == 1 { on_ok } else { on_err };
        self.jobs.push(Val::Num(-(state as double)));
        self.jobs.push(h);
        self.jobs.push(v);
        self.jobs.push(derived);
    }

    /// The queued jobs not run yet, in order.
    pub fn pending_jobs(&self) -> Vec<Val> {
        let mut out: Vec<Val> = Vec::new();
        let mut i = self.job_head as usize;
        while i < self.jobs.len() {
            out.push(self.jobs[i]);
            i += 1;
        }
        out
    }

    /// Runs queued promise jobs until none are left. From Engine::run /
    /// resume with slices on (job_slices), a job's script stops where its
    /// slice runs out as the script itself does: its frames stay, the
    /// queue's head says how the job ends (JOB_CONT), and job_resume ends
    /// it once Engine::resume has run the frames out.
    pub fn run_jobs(&mut self) {
        // from the host, a call of its own for the time limit
        let began = if self.native_depth == 0 { self.begin_call(false) } else { false };
        let slicing = self.slicing;
        if !self.job_slices {
            self.slicing = self.prof_on;
        }
        self.run_jobs_now();
        self.slicing = slicing;
        self.end_call(began);
    }

    fn run_jobs_now(&mut self) {
        let mut guard: int = 0;
        while (self.job_head as usize) < self.jobs.len() && guard < 10000000 && !self.terminated {
            guard += 1;
            // from the front, each job's four values taken out in place:
            // a queue of n jobs costs n, not n * n moves
            let h = self.job_head as usize;
            let a = self.jobs[h];
            let b = self.jobs[h + 1];
            let c = self.jobs[h + 2];
            let d = self.jobs[h + 3];
            self.jobs[h] = Val::Undef;
            self.jobs[h + 1] = Val::Undef;
            self.jobs[h + 2] = Val::Undef;
            self.jobs[h + 3] = Val::Undef;
            self.job_head = (h + 4) as int;
            if (self.job_head as usize) >= self.jobs.len() {
                self.jobs.clear();
                self.job_head = 0;
            } else if self.job_head >= 4096 && (self.job_head as usize) * 2 >= self.jobs.len() {
                let rest = self.pending_jobs();
                self.jobs = rest;
                self.job_head = 0;
            }
            // from the host (native_depth 0) the jobs' frames are safe
            // points: what the loop still needs after a call is rooted
            let top = self.native_depth == 0;
            if top {
                self.temp_roots.push(d.clone());
                self.native_depth -= 1;
            }
            let mut kind: int = 0;
            let mut g: int = -1;
            let mut val = Val::Undef;
            let stale = match a {
                Val::Num(x) => x >= JOB_CONT,
                _ => false,
            };
            if stale {
                // a stopped job whose frames were dropped (the time limit,
                // a host that let a suspended script go): nothing to end
            } else if let Val::Num(neg) = a {
                // a reaction
                let state = (-neg) as int;
                let derived = obj_of(&d);
                let bo = obj_of(&b);
                let nf = if bo >= 0 && self.objs[bo as usize].class == C_NATIVE { self.objs[bo as usize].func as int } else { -1 };
                if top && self.slicing && (nf == crate::coroutine::NF_ASYNC_OK || nf == crate::coroutine::NF_ASYNC_ERR) {
                    // an await's continuation: the async body runs from
                    // here, where its slice may stop
                    kind = JK_ASYNC;
                    g = self.objs[bo as usize].env as int;
                    self.temp_roots.push(Val::Obj(g));
                    let mode = if nf == crate::coroutine::NF_ASYNC_OK { crate::coroutine::GM_NEXT } else { crate::coroutine::GM_THROW };
                    self.async_step_s(g, mode, c, false);
                    self.temp_roots.pop();
                } else if self.is_callable(&b) {
                    kind = JK_REACT;
                    val = self.call_fixed(b, Val::Undef, 1, c, Val::Undef, Val::Undef);
                } else if derived >= 0 {
                    self.settle(derived, state, c);
                }
            } else if matches!(b, Val::Undef) && matches!(c, Val::Undef) && matches!(d, Val::Undef) {
                // queueMicrotask
                kind = JK_MICRO;
                self.call_fixed(a, Val::Undef, 0, Val::Undef, Val::Undef, Val::Undef);
            } else {
                // thenable job: then.call(thenable, resolve, reject)
                kind = JK_THENABLE;
                self.call_fixed(a, b, 2, c, d.clone(), Val::Undef);
            }
            if self.suspended {
                // the slice ran out in the job's script: how it ends
                // goes to the queue's head, with the script's value
                let rv = self.run_result;
                self.jobs_push_front(Val::Num(JOB_CONT + kind as double), if g >= 0 { Val::Obj(g) } else { Val::Undef }, rv, d);
                if top {
                    self.native_depth += 1;
                    self.temp_roots.pop();
                }
                return;
            }
            if !self.terminated {
                let threw = self.throwing;
                if threw {
                    val = self.exc.clone();
                    self.throwing = false;
                    self.exc = Val::Undef;
                }
                self.job_end(kind, d, val, threw);
            }
            if top {
                self.native_depth += 1;
                self.temp_roots.pop();
            }
            if self.terminated {
                // past the time limit: the exception leaves the call
                if !self.throwing {
                    self.terminate();
                }
                return;
            }
        }
    }

    /// The end of a job of `kind` (JK_*) whose call gave `val`, or threw it.
    fn job_end(&mut self, kind: int, d: Val, val: Val, threw: bool) {
        if kind == JK_REACT {
            let derived = obj_of(&d);
            if derived >= 0 {
                self.settle(derived, if threw { 2 } else { 1 }, val);
            }
        } else if kind == JK_ASYNC {
            let derived = obj_of(&d);
            if derived >= 0 {
                self.settle(derived, 1, Val::Undef);
            }
        } else if kind == JK_THENABLE && threw {
            self.call_fixed(d, Val::Undef, 1, val, Val::Undef, Val::Undef);
            if !self.terminated {
                self.throwing = false;
            }
        }
    }

    fn jobs_push_front(&mut self, a: Val, b: Val, c: Val, d: Val) {
        if self.job_head >= 4 {
            let h = (self.job_head - 4) as usize;
            self.jobs[h] = a;
            self.jobs[h + 1] = b;
            self.jobs[h + 2] = c;
            self.jobs[h + 3] = d;
            self.job_head = h as int;
            return;
        }
        let rest = self.pending_jobs();
        let mut out: Vec<Val> = vec![a, b, c, d];
        for v in rest {
            out.push(v);
        }
        self.jobs = out;
        self.job_head = 0;
    }

    /// A job stopped by its slice heads the queue (run_jobs): its frames
    /// are the ones Engine::resume runs.
    pub fn job_stopped(&self) -> bool {
        let h = self.job_head as usize;
        if h >= self.jobs.len() {
            return false;
        }
        if let Val::Num(x) = self.jobs[h] {
            return x >= JOB_CONT;
        }
        false
    }

    /// Engine::resume ran a stopped job's frames out (job_stopped): the
    /// job ends as run_jobs would have ended it. Answers the script's own
    /// value, kept for after the jobs.
    pub fn job_resume(&mut self) -> Val {
        let h = self.job_head as usize;
        let a = self.jobs[h];
        let b = self.jobs[h + 1];
        let c = self.jobs[h + 2];
        let d = self.jobs[h + 3];
        self.jobs[h] = Val::Undef;
        self.jobs[h + 1] = Val::Undef;
        self.jobs[h + 2] = Val::Undef;
        self.jobs[h + 3] = Val::Undef;
        self.job_head = (h + 4) as int;
        if (self.job_head as usize) >= self.jobs.len() {
            self.jobs.clear();
            self.job_head = 0;
        }
        let kind = match a {
            Val::Num(x) => (x - JOB_CONT) as int,
            _ => 0,
        };
        self.temp_roots.push(c.clone());
        self.temp_roots.push(d.clone());
        if kind == JK_ASYNC {
            let g = obj_of(&b);
            if g >= 0 {
                let r = self.gen_after(g);
                self.async_after(g, r);
            }
            if !self.terminated {
                self.job_end(kind, d, Val::Undef, false);
            }
        } else if !self.terminated {
            let threw = self.throwing;
            let val = if threw {
                let e = self.exc.clone();
                self.throwing = false;
                self.exc = Val::Undef;
                e
            } else {
                match self.stack.pop() {
                    Some(v) => v,
                    None => Val::Undef,
                }
            };
            self.job_end(kind, d, val, threw);
        }
        self.temp_roots.pop();
        self.temp_roots.pop();
        c
    }

    pub fn promise_then(&mut self, p: int, on_ok: Val, on_err: Val) -> Val {
        let d = self.new_promise();
        let state = self.objs[p as usize].pos as int;
        if state == 0 {
            self.push_elem(p, on_ok);
            self.push_elem(p, on_err);
            self.push_elem(p, Val::Obj(d));
        } else {
            let v = self.colds[self.objs[p as usize].cold as usize].prim.clone();
            self.queue_reaction(on_ok, on_err, Val::Obj(d), state, v);
        }
        Val::Obj(d)
    }

    // ---- URI

    pub fn uri_encode(&mut self, s: &str, keep: &str) -> Val {
        let mut out = String::new();
        let cs = s.chars().collect::<Vec<char>>();
        let mut i: usize = 0;
        while i < cs.len() {
            let c = cs[i];
            i += 1;
            let mut buf = String::new();
            // a lone surrogate cannot be encoded; a 0xdbfe/0xdbff pair kept
            // as two stand-ins is its character
            let lu = jsstr::lone_unit(c as int);
            if lu >= 0 {
                let lo = if lu >= 0xdbfe && lu <= 0xdbff && i < cs.len() { jsstr::lone_unit(cs[i] as int) } else { -1 };
                if lo < 0xdc00 {
                    self.throw_uri();
                    return Val::Undef;
                }
                jsstr::push_cp(&mut buf, 0x10000 + ((lu - 0xd800) << 10) + (lo - 0xdc00));
                i += 1;
            } else {
                let plain = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || "-_.!~*'()".contains(c) || keep.contains(c);
                if plain {
                    out.push(c);
                    continue;
                }
                buf.push(c);
            }
            for b in buf.bytes() {
                let v = b as int;
                out.push('%');
                out.push(hexch(v >> 4));
                out.push(hexch(v & 15));
            }
        }
        self.string_val(out)
    }

    pub fn uri_decode(&mut self, s: &str, reserved: &str) -> Val {
        let cs = s.chars().collect::<Vec<char>>();
        let mut out = String::new();
        let n = cs.len();
        let mut i: usize = 0;
        while i < n {
            let c = cs[i];
            if c != '%' {
                out.push(c);
                i += 1;
                continue;
            }
            // one UTF-8 sequence of %XX escapes
            let mut bytes: Vec<int> = Vec::new();
            let mut j = i;
            let mut need: usize = 1;
            while j + 2 < n {
                if cs[j] != '%' {
                    break;
                }
                let h = hexval(cs[j + 1]);
                let l = hexval(cs[j + 2]);
                if h < 0 || l < 0 {
                    break;
                }
                bytes.push(h * 16 + l);
                j += 3;
                let lead = bytes[0];
                need = if lead < 128 {
                    1
                } else if lead >= 240 {
                    4
                } else if lead >= 224 {
                    3
                } else if lead >= 192 {
                    2
                } else {
                    0
                };
                if bytes.len() >= need {
                    break;
                }
            }
            if bytes.is_empty() || need == 0 || bytes.len() < need {
                self.throw_uri();
                return Val::Undef;
            }
            let cp = utf8_decode(&bytes);
            if cp < 0 {
                self.throw_uri();
                return Val::Undef;
            }
            let mut t = String::new();
            jsstr::push_cp(&mut t, cp);
            if need == 1 && reserved.contains(t.as_str()) {
                let mut k = i;
                while k < j {
                    out.push(cs[k]);
                    k += 1;
                }
            } else {
                out.push_str(t.as_str());
            }
            i = j;
        }
        self.string_val(out)
    }

    pub fn throw_uri(&mut self) {
        let p = self.error_ctor_proto(6);
        let e = self.new_error(p, "URI malformed");
        self.throw_val(Val::Obj(e));
    }

    // ---- the dispatch

    /// Whether an Array.prototype method takes the prelude's generic path:
    /// a receiver that is not a plain array (an array-like, a proxy, a
    /// frozen array or one with element properties, holes over indexed
    /// prototypes), or a species constructor for the methods that make an
    /// array.
    fn ap_generic(&mut self, id: int, this: &Val) -> bool {
        let o = obj_of(this);
        if o < 0 {
            return true;
        }
        let ob = &self.objs[o as usize];
        let class = ob.class;
        if class == C_ARGUMENTS || class == C_TYPED {
            return false;
        }
        if class != C_ARRAY {
            return true;
        }
        if ob.pos == 3 {
            return true;
        }
        if ob.pos == 2 {
            // frozen: the methods that write
            return id == NF_AP_PUSH || id == NF_AP_POP || id == NF_AP_SHIFT || id == NF_AP_UNSHIFT || id == NF_AP_SPLICE || id == NF_AP_REVERSE || id == NF_AP_SORT || id == NF_AP_FILL || id == NF_AP_COPYWITHIN;
        }
        if self.idx_protos && !self.colds[ob.cold as usize].elems2.is_empty() {
            return true;
        }
        if ob.shape != 0 && self.find(o, A_LENGTH) >= 0 {
            // a read-only length (defineProperty): the spec's steps
            return true;
        }
        if id == NF_AP_MAP || id == NF_AP_FILTER || id == NF_AP_SLICE || id == NF_AP_SPLICE || id == NF_AP_CONCAT || id == NF_AP_FLAT || id == NF_AP_FLATMAP {
            // ArraySpeciesCreate: a constructor other than Array
            if ob.proto as int != self.array_proto {
                return true;
            }
            if ob.shape != 0 && self.find(o, A_CONSTRUCTOR) >= 0 {
                return true;
            }
            let ap = self.array_proto;
            let sl = self.find(ap, A_CONSTRUCTOR);
            let g = self.array_generic;
            let arr_ctor = if self.evecs[self.objs[g as usize].elems as usize].v.len() > 36 { obj_of(&self.evecs[self.objs[g as usize].elems as usize].v[36]) } else { -1 };
            if sl < 0 || (self.attr_at(ap, sl) & P_ACCESSOR) != 0 || obj_of(&self.objs[ap as usize].get_slot(sl as usize)) != arr_ctor {
                return true;
            }
        }
        if id == NF_AP_CONCAT || id == NF_AP_FLAT || id == NF_AP_FLATMAP {
            // holes stay holes; Symbol.isConcatSpreadable on the receiver
            if !self.colds[self.objs[o as usize].cold as usize].elems2.is_empty() {
                return true;
            }
        }
        false
    }

    /// concat's arguments the native path cannot take: an object with
    /// Symbol.isConcatSpreadable, a proxy, an array with holes or its own
    /// properties.
    fn concat_generic(&mut self, this: &Val, args: &Vec<Val>) -> bool {
        let sa = self.atom_lookup("@@isConcatSpreadable");
        if sa < 0 {
            return false;
        }
        let o = obj_of(this);
        if o >= 0 && self.objs[o as usize].shape != 0 && self.find(o, sa) >= 0 {
            return true;
        }
        let ap = self.array_proto;
        if self.has_atom(ap, sa) {
            return true;
        }
        for a in args.iter() {
            let x = obj_of(a);
            if x < 0 {
                continue;
            }
            let class = self.objs[x as usize].class;
            if class == C_PROXY {
                return true;
            }
            if class == C_ARRAY {
                if !self.colds[self.objs[x as usize].cold as usize].elems2.is_empty() || self.objs[x as usize].pos == 3 {
                    return true;
                }
                if self.objs[x as usize].shape != 0 && self.find(x, sa) >= 0 {
                    return true;
                }
                if self.objs[x as usize].proto as int != ap {
                    return self.has_atom(x, sa);
                }
                continue;
            }
            if self.has_atom(x, sa) {
                return true;
            }
        }
        false
    }

    pub fn call_native(&mut self, id: int, fobj: int, this: Val, args: &Vec<Val>, construct: bool, new_target: Val) -> Val {
        self.nat_top = self.nat_ff && self.native_depth == self.gc_free_depth + 1;
        self.nat_ff = false;
        if id >= NF_AP_PUSH && id <= NF_AP_WITH && (matches!(this, Val::Undef) || matches!(this, Val::Null)) {
            self.throw_type("Array.prototype method called on null or undefined");
            return Val::Undef;
        }
        if id >= NF_AP_PUSH && id <= NF_AP_WITH && self.array_generic >= 0 && (self.ap_generic(id, &this) || (id == NF_AP_CONCAT && self.concat_generic(&this, args))) {
            let g = self.array_generic;
            let k = (id - NF_AP_PUSH) as usize;
            if k < self.evecs[self.objs[g as usize].elems as usize].v.len() {
                let f = self.evecs[self.objs[g as usize].elems as usize].v[k].clone();
                if !matches!(f, Val::Undef) {
                    return self.call_value(f, this, args.clone());
                }
            }
        }
        if id >= NF_AP_PUSH && id <= NF_AP_WITH && !(id == NF_AP_FOREACH || id == NF_AP_MAP || id == NF_AP_FILTER || id == NF_AP_SOME || id == NF_AP_EVERY || id == NF_AP_REDUCE || id == NF_AP_REDUCERIGHT || id == NF_AP_INDEXOF || id == NF_AP_LASTINDEXOF) {
            // the rest read and write `elems` directly: holes become undefined
            let t = obj_of(&this);
            if t >= 0 && self.objs[t as usize].class == C_ARRAY && !self.colds[self.objs[t as usize].cold as usize].elems2.is_empty() {
                let mutates = id == NF_AP_PUSH || id == NF_AP_POP || id == NF_AP_SHIFT || id == NF_AP_UNSHIFT || id == NF_AP_SPLICE || id == NF_AP_REVERSE || id == NF_AP_SORT || id == NF_AP_FILL || id == NF_AP_COPYWITHIN;
                if mutates {
                    // holes and element properties become plain elements
                    let n = self.evecs[self.objs[t as usize].elems as usize].v.len() as int;
                    if self.objs[t as usize].pos == 3 || self.idx_protos {
                        let mut k: int = 0;
                        while k < n {
                            if self.is_hole(t, k) {
                                let v = self.get_index(&this, k);
                                if self.throwing {
                                    return Val::Undef;
                                }
                                let a = self.index_atom(k);
                                let sl = self.find(t, a);
                                if sl >= 0 {
                                    self.remove_own(t, sl);
                                }
                                self.put_elem(t, k, v);
                            }
                            k += 1;
                        }
                        if self.objs[t as usize].pos == 3 {
                            self.objs[t as usize].pos = 0;
                        }
                    }
                    self.put_elems2(t, Vec::new());
                }
            }
        }
        if id == NF_HOST_SYNC {
            if !self.host_sync_set {
                self.throw_type("__hostSync: the host gives no synchronous answers");
                return Val::Undef;
            }
            let req = if args.is_empty() { String::new() } else { self.to_string(&args[0]) };
            let answer = self.host_sync.call(&req);
            return self.string_val(answer);
        }
        if id == crate::typed::NF_FAST_WRAP && fobj >= 0 && (self.objs[fobj as usize].pos as int) == 0 {
            // __fastWrap(native, slowFn, name, length): the method
            let nat = if args.is_empty() { -1 } else { obj_of(&args[0]) };
            let slow = if args.len() > 1 { obj_of(&args[1]) } else { -1 };
            let name = if args.len() > 2 { self.to_string(&args[2]) } else { String::new() };
            let len = if args.len() > 3 { self.to_number(&args[3]) as int } else { 0 };
            if nat < 0 || slow < 0 || self.objs[nat as usize].class != C_NATIVE {
                self.throw_type("__fastWrap: a native and a function");
                return Val::Undef;
            }
            let f = self.native_fn(name.as_str(), crate::typed::NF_FAST_WRAP, len);
            self.objs[f as usize].pos = self.objs[nat as usize].func;
            self.put_env(f, slow);
            return Val::Obj(f);
        }
        if id == crate::typed::NF_DVM_GET || id == crate::typed::NF_DVM_SET || id == crate::typed::NF_FAST_WRAP {
            // not done in place (native_fast): the prelude's own method
            let slow = if fobj >= 0 { self.objs[fobj as usize].env as int } else { -1 };
            if slow < 0 {
                self.throw_type("DataView method without its implementation");
                return Val::Undef;
            }
            return self.call_value(Val::Obj(slow), this, args.clone());
        }
        if id == crate::typed::NF_TA_LEN || id == crate::typed::NF_TA_SET || (id >= crate::typed::NF_DV_MAKE && id <= crate::typed::NF_DV_METHOD) {
            return self.call_native_ta(id, &this, args);
        }
        if id == crate::proxy::NF_PROMISE_OF {
            let p = self.new_promise();
            self.temp_roots.push(Val::Obj(p));
            let x = if args.is_empty() { Val::Undef } else { args[0].clone() };
            self.settle(p, 1, x);
            self.temp_roots.pop();
            return Val::Obj(p);
        }
        if id >= 920 && id < 930 {
            return self.call_native_typed(id, args);
        }
        if id >= 930 && id < 940 {
            return self.call_native_proxy(id, args, construct);
        }
        if id >= 960 && id < 970 {
            return self.call_native_intl(id, args);
        }
        if id == crate::dynamic::NF_ESM_COMPILE {
            return self.call_native_esm(args);
        }
        if id >= 950 && id < 960 {
            return self.call_native_bigint(id, this, args, construct);
        }
        if id >= 944 && id < 950 {
            return self.call_native_dynamic_this(id, this, args);
        }
        if id >= 940 && id < 950 {
            return self.call_native_dynamic(id, args, new_target);
        }
        if self.proxy_hooks >= 0 {
            let p = if id == NF_OP_HASOWN || id == NF_OP_PROPENUM { obj_of(&this) } else if args.is_empty() { -1 } else { obj_of(&args[0]) };
            if p >= 0 && self.objs[p as usize].class == C_PROXY {
                if let Some(v) = self.proxy_native(id, p, &args) {
                    return v;
                }
            }
        }
        if id >= 900 && id < 1000 {
            return self.call_native_co(id, fobj, this, args);
        }
        if id >= NF_MATH && id < NF_MATH + 40 {
            return self.math(id - NF_MATH, &args);
        }
        if id == NF_AP_POP || id == NF_AP_SHIFT || id == NF_AP_UNSHIFT || id == NF_AP_SPLICE || id == NF_AP_REVERSE || id == NF_AP_SORT || id == NF_AP_FILL || id == NF_AP_COPYWITHIN {
            // the in-place methods on a frozen array
            if let Val::Obj(o) = &this {
                let ob = &self.objs[*o as usize];
                if ob.class == C_ARRAY && ob.pos == 2 {
                    let n = self.evecs[ob.elems as usize].v.len();
                    let msg = if n == 0 && (id == NF_AP_POP || id == NF_AP_SHIFT) {
                        String::from("Cannot assign to read only property 'length' of object '[object Array]'")
                    } else if id == NF_AP_POP {
                        format!("Cannot delete property '{}' of [object Array]", n - 1)
                    } else if id == NF_AP_UNSHIFT && !args.is_empty() {
                        format!("Cannot add property {}, object is not extensible", n)
                    } else if n > 0 {
                        String::from("Cannot assign to read only property '0' of object '[object Array]'")
                    } else {
                        String::new()
                    };
                    if !msg.is_empty() {
                        self.throw_type(msg.as_str());
                        return Val::Undef;
                    }
                }
            }
        }
        if id >= NF_ERROR && id < NF_ERROR + 9 {
            let kind = id - NF_ERROR;
            let dflt = self.error_ctor_proto(kind);
            let proto = if construct { self.proto_from(&new_target, dflt) } else { dflt };
            let e = self.alloc(C_ERROR, proto);
            let mut mi: usize = 0;
            if kind == 7 {
                let errs = arg(&args, 0);
                let items = self.iterable_to_vec(&errs);
                let arr = self.new_array(items);
                self.add_own(e, A_ERRORS, Val::Obj(arr), P_HIDDEN);
                mi = 1;
            }
            if kind == 8 {
                mi = 2;
            }
            let m = arg(&args, mi);
            if !matches!(m, Val::Undef) {
                let ms = self.to_str(&m);
                if self.throwing {
                    return Val::Undef;
                }
                let sv4 = self.rc_val(ms);
                self.add_own(e, A_MESSAGE, sv4, P_HIDDEN);
            }
            if kind == 8 {
                // SuppressedError(error, suppressed, message)
                let a_err = self.intern("error");
                let a_sup = self.intern("suppressed");
                self.add_own(e, a_err, arg(&args, 0), P_HIDDEN);
                self.add_own(e, a_sup, arg(&args, 1), P_HIDDEN);
            }
            let opts = if kind == 8 { Val::Undef } else { arg(&args, mi + 1) };
            if let Val::Obj(oo) = &opts {
                if self.has_atom(*oo, A_CAUSE) {
                    let c = self.get(&opts, A_CAUSE);
                    self.add_own(e, A_CAUSE, c, P_HIDDEN);
                }
            }
            self.add_stack(e);
            return Val::Obj(e);
        }
        if id >= NF_DP_GET && id < NF_DP_GET + 20 {
            let t = self.date_value(&this);
            if self.throwing {
                return Val::Undef;
            }
            let field = id - NF_DP_GET;
            if field == 8 {
                return Val::Num(t);
            }
            if field == 9 {
                return Val::Num(0.0);
            }
            if is_nan(t) {
                return Val::Num(nan());
            }
            let f = Vm::time_fields(t);
            let v = match field {
                0 => f[0],
                1 => f[1],
                2 => f[2],
                3 => f[3],
                4 => f[4],
                5 => f[5],
                6 => f[6],
                7 => f[7],
                10 => f[0] - 1900.0,
                _ => nan(),
            };
            return Val::Num(v);
        }
        if id >= NF_DP_SET && id < NF_DP_SET + 20 {
            let t = self.date_value(&this);
            if self.throwing {
                return Val::Undef;
            }
            let field = id - NF_DP_SET;
            let o = obj_of(&this);
            if field == 8 {
                let n = self.to_number(&arg(&args, 0));
                // TimeClip: + 0 turns -0 into +0
                let v = if is_finite(n) && n.abs() <= 8.64e15 { to_integer(n) + 0.0 } else { nan() };
                self.put_prim(o, Val::Num(v));
                return Val::Num(v);
            }
            let f = if is_nan(t) { Vm::time_fields(0.0) } else { Vm::time_fields(t) };
            let mut parts = vec![f[0], f[1], f[2], f[4], f[5], f[6], f[7]];
            // which slot the first argument sets, and how many follow
            let (start, count) = match field {
                0 => (0, 3),
                1 => (1, 2),
                2 => (2, 1),
                4 => (3, 4),
                5 => (4, 3),
                6 => (5, 2),
                7 => (6, 1),
                10 => (0, 1),
                _ => (0, 0),
            };
            let mut k: usize = 0;
            while k < count && k < args.len() {
                let n = self.to_number(&args[k]);
                parts[start + k] = n;
                k += 1;
            }
            if field == 10 {
                let y = parts[0];
                if y >= 0.0 && y <= 99.0 {
                    parts[0] = y + 1900.0;
                }
            }
            if is_nan(t) && field != 0 && field != 10 {
                // an invalid date stays invalid, except through the year
                return Val::Num(nan());
            }
            let v = Vm::make_time(&parts);
            self.put_prim(o, Val::Num(v));
            return Val::Num(v);
        }
        let a0 = arg(&args, 0);
        match id {
            NF_NOOP => Val::Undef,
            NF_PRINT => {
                let mut line = String::new();
                let mut i: usize = 0;
                while i < args.len() {
                    if i > 0 {
                        line.push(' ');
                    }
                    let s = self.display(&args[i]);
                    line.push_str(s.as_str());
                    i += 1;
                }
                // a lone surrogate leaves as U+FFFD, as V8 writes it
                let line = jsstr::to_well_formed(line.as_str(), true);
                if self.echo {
                    println!("{}", line);
                }
                self.out.push(line);
                Val::Undef
            }
            NF_GC => {
                if self.native_depth <= 1 {
                    // collected at the next safe point
                    self.alloc_count = self.gc_threshold;
                    self.gc_flag = true;
                }
                Val::Undef
            }
            NF_PERF_NOW => Val::Num(now_ms()),
            NF_PARSEINT => {
                let s = self.to_string(&a0);
                let r = arg(&args, 1);
                let radix = if matches!(r, Val::Undef) { 0 } else { to_int32(self.to_number(&r)) };
                Val::Num(parse_int(s.as_str(), radix))
            }
            NF_PARSEFLOAT => {
                let s = self.to_string(&a0);
                Val::Num(parse_float(s.as_str()))
            }
            NF_ISNAN => {
                let n = self.to_number(&a0);
                Val::Bool(is_nan(n))
            }
            NF_ISFINITE => {
                let n = self.to_number(&a0);
                Val::Bool(is_finite(n))
            }
            NF_ENCODE_URI_COMPONENT => {
                let s = self.to_string(&a0);
                self.uri_encode(s.as_str(), "")
            }
            NF_ENCODE_URI => {
                let s = self.to_string(&a0);
                self.uri_encode(s.as_str(), ";/?:@&=+$,#")
            }
            NF_DECODE_URI_COMPONENT => {
                let s = self.to_string(&a0);
                self.uri_decode(s.as_str(), "")
            }
            NF_DECODE_URI => {
                let s = self.to_string(&a0);
                self.uri_decode(s.as_str(), ";/?:@&=+$,#")
            }
            NF_ESCAPE => {
                let s = self.to_string(&a0);
                let mut out = String::new();
                for u in jsstr::units(s.as_str()) {
                    let c = u;
                    let plain = (c >= 65 && c <= 90) || (c >= 97 && c <= 122) || (c >= 48 && c <= 57) || c == 64 || c == 42 || c == 95 || c == 43 || c == 45 || c == 46 || c == 47;
                    if plain {
                        jsstr::push_cp(&mut out, c);
                    } else if c < 256 {
                        out.push('%');
                        out.push_str(hex_digits(c, 2, true).as_str());
                    } else {
                        out.push_str("%u");
                        out.push_str(hex_digits(c, 4, true).as_str());
                    }
                }
                self.string_val(out)
            }
            NF_UNESCAPE => {
                let s = self.to_string(&a0);
                let u = jsstr::units(s.as_str());
                let mut out: Vec<int> = Vec::new();
                let n = u.len();
                let mut i: usize = 0;
                while i < n {
                    if u[i] == 37 && i + 5 < n + 0 && u[i + 1] == 117 {
                        let mut v: int = 0;
                        let mut ok = true;
                        let mut k = 2;
                        while k < 6 {
                            let h = hexval_unit(u[i + k]);
                            if h < 0 {
                                ok = false;
                            }
                            v = v * 16 + h;
                            k += 1;
                        }
                        if ok {
                            out.push(v);
                            i += 6;
                            continue;
                        }
                    }
                    if u[i] == 37 && i + 2 < n {
                        let h = hexval_unit(u[i + 1]);
                        let l = hexval_unit(u[i + 2]);
                        if h >= 0 && l >= 0 {
                            out.push(h * 16 + l);
                            i += 3;
                            continue;
                        }
                    }
                    out.push(u[i]);
                    i += 1;
                }
                let len = out.len() as int;
                self.string_val(jsstr::from_units(&out, 0, len))
            }
            NF_QUEUE_MICROTASK => {
                self.jobs.push(a0);
                self.jobs.push(Val::Undef);
                self.jobs.push(Val::Undef);
                self.jobs.push(Val::Undef);
                Val::Undef
            }
            NF_FUNCTION => self.call_native_dynamic(NF_FUNCTION, args, new_target),
            NF_STRING => {
                let s = if args.is_empty() {
                    Rc::new(String::new())
                } else if !construct && self.class_of(&a0) == C_SYMBOL {
                    Rc::new(self.display(&a0))
                } else {
                    self.to_str(&a0)
                };
                if construct {
                    let proto = self.proto_from(&new_target, self.string_proto);
                    let o = self.alloc(C_STRING, proto);
                    let sv3 = self.rc_val(s);
                    self.put_prim(o, sv3);
                    return Val::Obj(o);
                }
                self.rc_val(s)
            }
            NF_NUMBER => {
                let prim = if is_obj(&a0) { self.to_primitive(&a0, "number") } else { a0.clone() };
                let n = if args.is_empty() {
                    0.0
                } else if self.class_of(&prim) == C_BIGINT {
                    crate::bigint::to_double(self.big_text(&prim).as_str())
                } else {
                    self.to_number(&prim)
                };
                if construct {
                    let proto = self.proto_from(&new_target, self.number_proto);
                    let o = self.alloc(C_NUMBER, proto);
                    self.put_prim(o, Val::Num(n));
                    return Val::Obj(o);
                }
                Val::Num(n)
            }
            NF_BOOLEAN => {
                let b = self.truthy_v(&a0);
                if construct {
                    let proto = self.proto_from(&new_target, self.boolean_proto);
                    let o = self.alloc(C_BOOLEAN, proto);
                    self.put_prim(o, Val::Bool(b));
                    return Val::Obj(o);
                }
                Val::Bool(b)
            }
            NF_OBJECT => {
                if construct && !matches!(new_target, Val::Undef) {
                    let nt = obj_of(&new_target);
                    if nt != fobj {
                        let proto = self.proto_from(&new_target, self.object_proto);
                        let o = self.alloc(C_OBJECT, proto);
                        return Val::Obj(o);
                    }
                }
                match a0 {
                    Val::Undef | Val::Null => {
                        let o = self.new_object();
                        Val::Obj(o)
                    }
                    _ => {
                        let o = self.to_object(&a0);
                        Val::Obj(o)
                    }
                }
            }
            NF_ARRAY => {
                let proto = if construct { self.proto_from(&new_target, self.array_proto) } else { self.array_proto };
                let a = self.alloc(C_ARRAY, proto);
                if args.len() == 1 {
                    if let Val::Num(n) = a0 {
                        if n < 0.0 || n > 4294967295.0 || n != to_integer(n) {
                            self.throw_range("Invalid array length");
                            return Val::Undef;
                        }
                        if self.elems_too_many(n) {
                            return Val::Undef;
                        }
                        let len = n as int;
                        let mut v: Vec<Val> = Vec::with_capacity(len as usize);
                        let mut h: Vec<Val> = Vec::with_capacity(len as usize);
                        let mut i = 0;
                        while i < len {
                            v.push(Val::Undef);
                            h.push(Val::Bool(true));
                            if (i & 65535) == 65535 && self.poll(262144) {
                                return Val::Undef;
                            }
                            i += 1;
                        }
                        self.put_elems(a, v);
                        self.put_elems2(a, h);
                        return Val::Obj(a);
                    }
                }
                self.put_elems(a, args.clone());
                Val::Obj(a)
            }
            NF_SYMBOL => {
                if construct {
                    self.throw_type("Symbol is not a constructor");
                    return Val::Undef;
                }
                let desc = if matches!(a0, Val::Undef) { String::new() } else { self.to_string(&a0) };
                let n = self.symbols.len();
                let name = format!("@@sym:{}:{}", n, desc);
                let atom = self.intern(name.as_str());
                let sp = self.symbol_proto;
                let s = self.alloc(C_SYMBOL, sp);
                self.objs[s as usize].pos = atom as i32;
                let sv2 = self.string_val(desc);
                self.put_prim(s, if matches!(a0, Val::Undef) { Val::Undef } else { sv2 });
                self.symbol_atoms.insert(atom, s);
                if (atom as usize) < self.atom_kinds.len() {
                    self.atom_kinds[atom as usize] = 0;
                }
                self.symbols.push(s);
                Val::Obj(s)
            }
            NF_SYM_FOR => {
                let key = self.to_string(&a0);
                if let Some(s) = self.symbol_registry.get(&key) {
                    return Val::Obj(*s);
                }
                let name = format!("@@for:{}", key);
                let atom = self.intern(name.as_str());
                let sp = self.symbol_proto;
                let s = self.alloc(C_SYMBOL, sp);
                self.objs[s as usize].pos = atom as i32;
                let sv1 = self.string_val(key.clone());
                self.put_prim(s, sv1);
                self.symbol_atoms.insert(atom, s);
                if (atom as usize) < self.atom_kinds.len() {
                    self.atom_kinds[atom as usize] = 0;
                }
                self.symbols.push(s);
                self.symbol_registry.insert(key, s);
                Val::Obj(s)
            }
            NF_SYM_KEYFOR => {
                let o = obj_of(&a0);
                if o < 0 || self.objs[o as usize].class != C_SYMBOL {
                    self.throw_type("not a symbol");
                    return Val::Undef;
                }
                let name = self.atom_str(self.objs[o as usize].pos as int);
                if name.starts_with("@@for:") {
                    return self.colds[self.objs[o as usize].cold as usize].prim.clone();
                }
                Val::Undef
            }
            NF_SYMP_TOSTRING => {
                let t = self.this_symbol(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let s = self.display(&t);
                self.string_val(s)
            }
            NF_SYMP_DESCRIPTION => {
                let t = self.this_symbol(&this);
                let o = obj_of(&t);
                if o >= 0 && self.objs[o as usize].class == C_SYMBOL {
                    return self.colds[self.objs[o as usize].cold as usize].prim.clone();
                }
                Val::Undef
            }
            NF_EP_TOSTRING => {
                if !is_obj(&this) {
                    self.throw_type("Error.prototype.toString called on non-object");
                    return Val::Undef;
                }
                let n = self.get(&this, A_NAME);
                let m = self.get(&this, A_MESSAGE);
                let ns = if matches!(n, Val::Undef) { String::from("Error") } else { self.to_string(&n) };
                let ms = if matches!(m, Val::Undef) { String::new() } else { self.to_string(&m) };
                if ns.is_empty() {
                    return self.string_val(ms);
                }
                if ms.is_empty() {
                    return self.string_val(ns);
                }
                self.string_val(format!("{}: {}", ns, ms))
            }
            NF_CALL => {
                let mut rest: Vec<Val> = Vec::new();
                let mut i: usize = 1;
                while i < args.len() {
                    rest.push(args[i].clone());
                    i += 1;
                }
                self.call_value(this, a0, rest)
            }
            NF_APPLY => {
                let a1 = arg(&args, 1);
                let items = self.array_like_to_vec(&a1);
                if self.throwing {
                    return Val::Undef;
                }
                self.call_value(this, a0, items)
            }
            NF_BIND => {
                if !self.is_callable(&this) {
                    self.throw_type("Bind must be called on a function");
                    return Val::Undef;
                }
                let fp = self.objs[obj_of(&this) as usize].proto as int;
                let b = self.alloc(C_BOUND, fp);
                self.put_env(b, obj_of(&this));
                self.put_prim(b, a0);
                let mut rest: Vec<Val> = Vec::new();
                let mut i: usize = 1;
                while i < args.len() {
                    rest.push(args[i].clone());
                    i += 1;
                }
                self.put_elems(b, rest);
                Val::Obj(b)
            }
            NF_FN_TOSTRING => {
                let o = obj_of(&this);
                if o < 0 || !self.is_callable(&this) {
                    self.throw_type("Function.prototype.toString requires that 'this' be a Function");
                    return Val::Undef;
                }
                let n = self.get(&this, A_NAME);
                let ns = self.to_string(&n);
                if self.objs[o as usize].class == C_FUNCTION {
                    let pi = self.objs[o as usize].func as int;
                    if let Val::Str(src) = &self.protos[pi as usize].src {
                        let src = self.sget(src.clone());
                        let (a, b) = (self.protos[pi as usize].src_start, self.protos[pi as usize].src_end);
                        let mut text = String::new();
                        let mut i: int = 0;
                        for c in src.chars() {
                            if i >= b {
                                break;
                            }
                            if i >= a {
                                text.push(c);
                            }
                            i += 1;
                        }
                        return self.string_val(text);
                    }
                    if self.protos[pi as usize].class_ctor {
                        return self.string_val(format!("class {} {{ }}", ns));
                    }
                }
                self.string_val(format!("function {}() {{ [native code] }}", ns))
            }
            // ---- Object
            NF_O_KEYS | NF_O_VALUES | NF_O_ENTRIES => {
                let o = self.to_object(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let keys = self.own_keys(o, false, false);
                let mut out: Vec<Val> = Vec::new();
                for k in keys {
                    if id == NF_O_KEYS {
                        out.push(k);
                    } else {
                        let v = self.get_elem(&Val::Obj(o), &k);
                        if id == NF_O_VALUES {
                            out.push(v);
                        } else {
                            let p = self.new_array(vec![k, v]);
                            out.push(Val::Obj(p));
                        }
                    }
                }
                let a = self.new_array(out);
                Val::Obj(a)
            }
            NF_O_ASSIGN => {
                let t = self.to_object(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let mut i: usize = 1;
                while i < args.len() {
                    let src = args[i].clone();
                    if let Val::Obj(so) = &src {
                        let keys = self.own_keys(*so, false, true);
                        for k in keys {
                            let v = self.get_elem(&src, &k);
                            if self.throwing {
                                return Val::Undef;
                            }
                            // Set(to, key, value, true): a failed write throws
                            let saved = self.strict_at;
                            self.strict_at = self.frames.len() as int;
                            self.set_elem(&Val::Obj(t), &k, v);
                            self.strict_at = saved;
                            if self.throwing {
                                return Val::Undef;
                            }
                        }
                    } else if let Val::Str(s) = &src {
                        let s = self.sget(s.clone());
                        let mut k: int = 0;
                        for c in s.chars() {
                            let mut t2 = String::new();
                            t2.push(c);
                            let sv0 = self.string_val(t2);
                            self.set_index(t, k, sv0);
                            k += 1;
                        }
                    }
                    i += 1;
                }
                Val::Obj(t)
            }
            NF_O_CREATE => {
                let proto = match &a0 {
                    Val::Obj(p) => *p,
                    Val::Null => -1,
                    _ => {
                        self.throw_type("Object prototype may only be an Object or null");
                        return Val::Undef;
                    }
                };
                let o = self.alloc(C_OBJECT, proto);
                let props = arg(&args, 1);
                if !matches!(props, Val::Undef) {
                    self.define_props(o, &props);
                    if self.throwing {
                        return Val::Undef;
                    }
                }
                Val::Obj(o)
            }
            NF_O_GETPROTO | NF_REFLECT_GETPROTO => {
                let o = self.to_object(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let p = self.objs[o as usize].proto as int;
                if p >= 0 {
                    Val::Obj(p)
                } else {
                    Val::Null
                }
            }
            NF_O_SETPROTO => {
                let p = arg(&args, 1);
                if let Val::Obj(o) = &a0 {
                    match p {
                        Val::Obj(po) => {
                            // no cycles
                            let mut c = po;
                            while c >= 0 {
                                if c == *o {
                                    self.throw_type("Cyclic __proto__ value");
                                    return Val::Undef;
                                }
                                c = self.objs[c as usize].proto as int;
                            }
                            self.put_proto(*o, po);
                        }
                        Val::Null => self.put_proto(*o, -1),
                        _ => {
                            self.throw_type("Object prototype may only be an Object or null");
                            return Val::Undef;
                        }
                    }
                }
                a0
            }
            NF_O_DEFPROP | NF_REFLECT_DEFPROP => {
                let o = obj_of(&a0);
                if o < 0 {
                    self.throw_type("Object.defineProperty called on non-object");
                    return Val::Undef;
                }
                let k = arg(&args, 1);
                let d = arg(&args, 2);
                self.to_descriptor(o, &k, &d);
                if id == NF_REFLECT_DEFPROP {
                    if self.throwing && !self.terminated {
                        self.throwing = false;
                        self.exc = Val::Undef;
                        return Val::Bool(false);
                    }
                    return Val::Bool(true);
                }
                a0
            }
            NF_O_DEFPROPS => {
                let o = obj_of(&a0);
                if o < 0 {
                    self.throw_type("Object.defineProperties called on non-object");
                    return Val::Undef;
                }
                let props = arg(&args, 1);
                self.define_props(o, &props);
                if self.throwing {
                    return Val::Undef;
                }
                a0
            }
            NF_O_OWNNAMES | NF_O_OWNSYMBOLS | NF_REFLECT_OWNKEYS => {
                let o = self.to_object(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let mut keys = self.own_keys(o, true, id != NF_O_OWNNAMES);
                let class = self.objs[o as usize].class;
                if id != NF_O_OWNSYMBOLS && (class == C_ARRAY || class == C_STRING) && self.find(o, A_LENGTH) < 0 {
                    keys.push(self.str_val("length"));
                }
                if id != NF_O_OWNSYMBOLS && (class == C_FUNCTION || class == C_BOUND) {
                    let mut extra: Vec<Val> = Vec::new();
                    if self.find(o, A_LENGTH) < 0 && !self.vgone(o, A_LENGTH) {
                        extra.push(self.str_val("length"));
                    }
                    if self.find(o, A_NAME) < 0 && !self.vgone(o, A_NAME) {
                        extra.push(self.str_val("name"));
                    }
                    for k in keys {
                        extra.push(k);
                    }
                    keys = extra;
                }
                if id == NF_O_OWNSYMBOLS {
                    let mut only: Vec<Val> = Vec::new();
                    for k in keys {
                        if is_obj(&k) {
                            only.push(k);
                        }
                    }
                    keys = only;
                }
                let a = self.new_array(keys);
                Val::Obj(a)
            }
            NF_O_OWNDESC => {
                let o = self.to_object(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let k = arg(&args, 1);
                self.from_descriptor(o, &k)
            }
            NF_O_OWNDESCS => {
                let o = self.to_object(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let r = self.new_object();
                let keys = self.own_keys(o, true, true);
                for k in keys {
                    let d = self.from_descriptor(o, &k);
                    self.define_elem(r, &k, d, 0);
                }
                Val::Obj(r)
            }
            NF_O_FREEZE | NF_O_SEAL | NF_O_PREVENTEXT => {
                if let Val::Obj(o) = &a0 {
                    let o = *o;
                    if id != NF_O_PREVENTEXT && self.objs[o as usize].class == C_ARRAY && (id == NF_O_SEAL || self.objs[o as usize].pos == 3) {
                        // sealed elements (or frozen ones beside element
                        // properties) become properties with attributes
                        let n = self.evecs[self.objs[o as usize].elems as usize].v.len() as int;
                        let mut k: int = 0;
                        while k < n {
                            if !self.is_hole(o, k) {
                                let (v, _) = self.elem_to_prop(o, k);
                                let a = self.index_atom(k);
                                self.define(o, a, v, 0);
                            }
                            k += 1;
                        }
                    }
                    if id != NF_O_PREVENTEXT && self.objs[o as usize].class == C_ARGUMENTS {
                        self.slow_args(o);
                    }
                    self.objs[o as usize].extensible = false;
                    if id != NF_O_PREVENTEXT {
                        let n = self.nkeys(o);
                        let mut i: int = 0;
                        while i < n {
                            let mut at = self.attr_at(o, i);
                            let acc = (at & P_ACCESSOR) != 0;
                            if id == NF_O_FREEZE && (at & P_MAPPED) != 0 {
                                // a frozen element keeps the parameter's
                                // value and is no longer mapped
                                let ma = self.key_at(o, i);
                                self.mapped_sync(o, ma);
                                at = at - P_MAPPED;
                            }
                            if id == NF_O_FREEZE && !acc {
                                self.set_attr(o, i, at | P_FIXED | P_READONLY);
                                let a = self.key_at(o, i);
                                self.mark_setter(a);
                            } else {
                                self.set_attr(o, i, at | P_FIXED);
                            }
                            i += 1;
                        }
                        self.any_setter = true;
                        if id == NF_O_FREEZE {
                            // frozen elements: kept as read-only properties
                            let class = self.objs[o as usize].class;
                            if class == C_ARRAY && self.objs[o as usize].pos != 3 {
                                self.objs[o as usize].pos = 2;
                            }
                        }
                    }
                }
                a0
            }
            NF_O_ISFROZEN | NF_O_ISSEALED => {
                if let Val::Obj(o) = &a0 {
                    let o = *o;
                    if self.objs[o as usize].extensible {
                        return Val::Bool(false);
                    }
                    let class = self.objs[o as usize].class;
                    if class == C_ARRAY && self.objs[o as usize].pos == 3 && self.has_plain_elem(o) {
                        return Val::Bool(false);
                    }
                    if id == NF_O_ISFROZEN && class == C_ARRAY && self.objs[o as usize].pos < 2 && self.has_plain_elem(o) {
                        return Val::Bool(false);
                    }
                    let sh = self.objs[o as usize].shape as int;
                    for a in self.shape_attrs(sh).iter() {
                        if (*a & P_FIXED) == 0 {
                            return Val::Bool(false);
                        }
                        if id == NF_O_ISFROZEN && (*a & P_ACCESSOR) == 0 && (*a & P_READONLY) == 0 {
                            return Val::Bool(false);
                        }
                    }
                    return Val::Bool(true);
                }
                Val::Bool(true)
            }
            NF_O_ISEXT => {
                if let Val::Obj(o) = &a0 {
                    return Val::Bool(self.objs[*o as usize].extensible);
                }
                Val::Bool(false)
            }
            NF_O_IS => {
                let b = arg(&args, 1);
                Val::Bool(self.same_value(&a0, &b))
            }
            NF_O_FROMENTRIES => {
                let items = self.iterable_to_vec(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let o = self.new_object();
                for it in items {
                    let k = self.get_index(&it, 0);
                    let v = self.get_index(&it, 1);
                    self.define_elem(o, &k, v, 0);
                }
                Val::Obj(o)
            }
            NF_O_HASOWN => {
                let o = self.to_object(&a0);
                if self.throwing {
                    return Val::Undef;
                }
                let k = arg(&args, 1);
                Val::Bool(self.has_own(o, &k))
            }
            NF_O_GROUPBY => {
                let items = self.iterable_to_vec(&a0);
                let f = arg(&args, 1);
                let o = self.alloc(C_OBJECT, -1);
                let mut i: int = 0;
                for it in items {
                    let k = self.call_fixed(f.clone(), Val::Undef, 2, it.clone(), Val::Num(i as double), Val::Undef);
                    if self.throwing {
                        return Val::Undef;
                    }
                    let cur = self.get_elem(&Val::Obj(o), &k);
                    if let Val::Obj(a) = cur {
                        self.push_elem(a, it);
                    } else {
                        let a = self.new_array(vec![it]);
                        self.define_elem(o, &k, Val::Obj(a), 0);
                    }
                    i += 1;
                }
                Val::Obj(o)
            }
            NF_OP_HASOWN => {
                let o = self.to_object(&this);
                if self.throwing {
                    return Val::Undef;
                }
                Val::Bool(self.has_own(o, &a0))
            }
            NF_OP_ISPROTO => {
                let p = obj_of(&this);
                if let Val::Obj(o) = &a0 {
                    let mut c = self.objs[*o as usize].proto as int;
                    while c >= 0 {
                        if c == p {
                            return Val::Bool(true);
                        }
                        c = self.objs[c as usize].proto as int;
                    }
                }
                Val::Bool(false)
            }
            NF_OP_PROPENUM => {
                let o = self.to_object(&this);
                if self.throwing {
                    return Val::Undef;
                }
                let (i, a) = self.to_key(&a0);
                let class = self.objs[o as usize].class;
                if i >= 0 && (class == C_ARRAY || class == C_ARGUMENTS) {
                    return Val::Bool((i as usize) < self.evecs[self.objs[o as usize].elems as usize].v.len());
                }
                let atom = if i >= 0 { self.index_atom(i) } else { a };
                let slot = self.find(o, atom);
                if slot < 0 {
                    return Val::Bool(false);
                }
                Val::Bool((self.attr_at(o, slot) & P_HIDDEN) == 0)
            }
            NF_OP_TOSTRING => {
                let mut class: int = -1;
                let tag = match &this {
                    Val::Undef => String::from("Undefined"),
                    Val::Null => String::from("Null"),
                    _ => {
                        // a primitive reads through its prototype, as its
                        // wrapper would, without making one
                        let (o, wc) = match &this {
                            Val::Num(_) => (self.number_proto, C_NUMBER),
                            Val::Str(_) => (self.string_proto, C_STRING),
                            Val::Bool(_) => (self.boolean_proto, C_BOOLEAN),
                            _ => {
                                let w = self.to_object(&this);
                                (w, self.objs[w as usize].class)
                            }
                        };
                        if self.a_tostringtag < 0 {
                            self.a_tostringtag = self.intern("@@toStringTag");
                        }
                        let a_tag = self.a_tostringtag;
                        let t = self.get_obj(o, a_tag, &this);
                        if let Val::Str(s) = t {
                            let s = self.sget(s.clone());
                            s.as_ref().clone()
                        } else {
                            let c = wc;
                            class = c as int;
                            if (c as usize) < self.tostr_vals.len() && !matches!(self.tostr_vals[c as usize], Val::Undef) {
                                return self.tostr_vals[c as usize].clone();
                            }
                            String::from(match c {
                                C_ARRAY => "Array",
                                C_FUNCTION | C_NATIVE | C_BOUND => "Function",
                                C_ERROR => "Error",
                                C_REGEXP => "RegExp",
                                C_DATE => "Date",
                                C_ARGUMENTS | C_SARGS => "Arguments",
                                C_BOOLEAN => "Boolean",
                                C_NUMBER => "Number",
                                C_STRING => "String",
                                C_MAP => "Map",
                                C_SET => "Set",
                                C_SYMBOL => "Symbol",
                                C_PROMISE => "Promise",
                                _ => "Object",
                            })
                        }
                    }
                };
                if class >= 0 {
                    // the answer of a class is kept (a pinned string)
                    let v = self.pin_rc(Rc::new(format!("[object {}]", tag)));
                    while self.tostr_vals.len() <= class as usize {
                        self.tostr_vals.push(Val::Undef);
                    }
                    self.tostr_vals[class as usize] = v.clone();
                    return v;
                }
                self.string_val(format!("[object {}]", tag))
            }
            NF_OP_TOLOCALE => {
                let f = self.get(&this, A_TOSTRING);
                self.call_fixed(f, this, 0, Val::Undef, Val::Undef, Val::Undef)
            }
            NF_OP_VALUEOF => {
                let o = self.to_object(&this);
                if self.throwing {
                    return Val::Undef;
                }
                Val::Obj(o)
            }
            // ---- Reflect
            NF_REFLECT_APPLY => {
                let a2 = arg(&args, 2);
                let items = self.array_like_to_vec(&a2);
                if self.throwing {
                    return Val::Undef;
                }
                let t = arg(&args, 1);
                self.call_value(a0, t, items)
            }
            NF_REFLECT_CONSTRUCT => {
                let nt = if args.len() > 2 { args[2].clone() } else { a0.clone() };
                if !self.is_constructor(&a0) || !self.is_constructor(&nt) {
                    self.throw_type("Reflect.construct: target and newTarget must be constructors");
                    return Val::Undef;
                }
                let a1 = arg(&args, 1);
                let items = self.array_like_to_vec(&a1);
                if self.throwing {
                    return Val::Undef;
                }
                self.construct(a0, items, nt)
            }
            NF_REFLECT_HAS => {
                let o = obj_of(&a0);
                if o < 0 {
                    self.throw_type("Reflect.has called on non-object");
                    return Val::Undef;
                }
                let k = arg(&args, 1);
                Val::Bool(self.has_property(o, &k))
            }
            NF_REFLECT_GET => {
                let k = arg(&args, 1);
                let o = obj_of(&a0);
                if o < 0 {
                    self.throw_type("Reflect.get called on non-object");
                    return Val::Undef;
                }
                if args.len() > 2 {
                    // a getter sees the receiver
                    let (i, a) = self.to_key(&k);
                    let atom = if i >= 0 { self.index_atom(i) } else { a };
                    if i >= 0 && self.objs[o as usize].class != C_OBJECT {
                        return self.get_elem(&a0, &k);
                    }
                    return self.get_obj(o, atom, &args[2]);
                }
                self.get_elem(&a0, &k)
            }
            NF_REFLECT_SET => {
                let k = arg(&args, 1);
                let v = arg(&args, 2);
                self.set_elem(&a0, &k, v);
                Val::Bool(!self.throwing)
            }
            NF_REFLECT_DELETE => {
                let o = obj_of(&a0);
                if o < 0 {
                    return Val::Bool(false);
                }
                let k = arg(&args, 1);
                Val::Bool(self.delete(o, &k))
            }
            _ => self.call_native2(id, fobj, this, args, construct, new_target),
        }
    }

    pub fn math(&mut self, which: int, args: &Vec<Val>) -> Val {
        if which == 31 || which == 32 {
            // min / max
            let mut r = if which == 31 { infinity() } else { -infinity() };
            let mut nan_seen = false;
            for a in args.iter() {
                let n = self.to_number(a);
                if is_nan(n) {
                    nan_seen = true;
                    continue;
                }
                if which == 31 {
                    if n < r || (n == 0.0 && r == 0.0 && 1.0 / n < 0.0) {
                        r = n;
                    }
                } else if n > r || (n == 0.0 && r == 0.0 && 1.0 / n > 0.0) {
                    r = n;
                }
            }
            return Val::Num(if nan_seen { nan() } else { r });
        }
        if which == 33 {
            // xorshift, deterministic
            // Park-Miller: exact in doubles, the same sequence on every target
            let x = (((self.rng as double) * 16807.0) % 2147483647.0) as int;
            self.rng = x;
            let v = ((x - 1) as double) / 2147483646.0;
            return Val::Num(v);
        }
        if which == 34 {
            let mut s: double = 0.0;
            let mut inf = false;
            let mut nn = false;
            for a in args.iter() {
                let n = self.to_number(a);
                if !is_finite(n) && !is_nan(n) {
                    inf = true;
                }
                if is_nan(n) {
                    nn = true;
                }
                s += n * n;
            }
            if inf {
                return Val::Num(infinity());
            }
            if nn {
                return Val::Num(nan());
            }
            return Val::Num(s.sqrt());
        }
        let x = self.to_number(&arg(args, 0));
        if which >= 28 && which <= 30 {
            let y = self.to_number(&arg(args, 1));
            let r = match which {
                28 => crate::vm::arith_num(crate::ops::OP_EXP, x, y),
                29 => x.atan2(y),
                _ => crate::vm::imul(to_int32(x), to_int32(y)) as double,
            };
            return Val::Num(r);
        }
        Val::Num(math1(which, x))
    }
}

/// Math's one-argument functions (`which` 0..27) on a number.
pub fn math1(which: int, x: double) -> double {
    {
        match which {
            0 => x.abs(),
            1 => x.floor(),
            2 => x.ceil(),
            3 => {
                if !is_finite(x) || x == 0.0 {
                    x
                } else if x > 0.0 && x < 0.5 {
                    0.0
                } else if x < 0.0 && x >= -0.5 {
                    neg_zero()
                } else {
                    (x + 0.5).floor()
                }
            }
            4 => to_integer(x),
            5 => {
                if is_nan(x) || x == 0.0 {
                    x
                } else if x > 0.0 {
                    1.0
                } else {
                    -1.0
                }
            }
            6 => x.sqrt(),
            7 => x.cbrt(),
            8 => x.exp(),
            9 => x.exp_m1(),
            10 => x.ln(),
            11 => x.log2(),
            12 => x.log10(),
            13 => x.ln_1p(),
            14 => x.sin(),
            15 => x.cos(),
            16 => x.tan(),
            17 => x.asin(),
            18 => x.acos(),
            19 => x.atan(),
            20 => x.sinh(),
            21 => x.cosh(),
            22 => x.tanh(),
            23 => x.asinh(),
            24 => x.acosh(),
            25 => x.atanh(),
            26 => crate::typed::fround(x),
            27 => {
                let mut u = to_uint32(x) as int;
                let mut n: int = 32;
                while u > 0 {
                    u = u >> 1;
                    n -= 1;
                }
                n as double
            }
            _ => nan(),
        }
    }
}

/// `v` in hexadecimal, zero-padded to `width` digits.
fn hex_digits(v: int, width: int, upper: bool) -> String {
    let mut digits: Vec<char> = Vec::new();
    let mut n = v;
    while n > 0 {
        let d = n % 16;
        let code: u8 = if d < 10 {
            (48 + d) as u8
        } else if upper {
            (55 + d) as u8
        } else {
            (87 + d) as u8
        };
        digits.push(code as char);
        n = n / 16;
    }
    while (digits.len() as int) < width {
        digits.push('0');
    }
    let mut out = String::new();
    let mut i = (digits.len() as int) - 1;
    while i >= 0 {
        out.push(digits[i as usize]);
        i -= 1;
    }
    out
}

fn hexch(v: int) -> char {
    let c: u8 = if v < 10 { (48 + v) as u8 } else { (55 + v) as u8 };
    c as char
}

fn hexval_unit(c: int) -> int {
    if c >= 48 && c <= 57 {
        return c - 48;
    }
    if c >= 97 && c <= 102 {
        return c - 87;
    }
    if c >= 65 && c <= 70 {
        return c - 55;
    }
    -1
}

/// The code point of one UTF-8 sequence; -1 when it is not a valid one.
fn utf8_decode(b: &Vec<int>) -> int {
    let n = b.len();
    let lead = b[0];
    let mut i: usize = 1;
    let mut cp: int;
    let min: int;
    if n == 1 {
        return if lead < 128 { lead } else { -1 };
    }
    if n == 2 {
        cp = lead & 31;
        min = 128;
    } else if n == 3 {
        cp = lead & 15;
        min = 2048;
    } else {
        cp = lead & 7;
        min = 65536;
    }
    while i < n {
        if (b[i] & 192) != 128 {
            return -1;
        }
        cp = (cp << 6) | (b[i] & 63);
        i += 1;
    }
    if cp < min || cp > 1114111 || (cp >= 55296 && cp <= 57343) {
        return -1;
    }
    cp
}

fn hexval(c: char) -> int {
    if c >= '0' && c <= '9' {
        return (c as int) - 48;
    }
    if c >= 'a' && c <= 'f' {
        return (c as int) - 87;
    }
    if c >= 'A' && c <= 'F' {
        return (c as int) - 55;
    }
    -1
}

pub fn regex_cache_new() -> HashMap<String, int> {
    HashMap::new()
}

/// `count` digits at `i` of a date string as a number, -1 when they are not
/// all there (Date.parse; no closure, which the Go build did not write back)
fn date_digits(cs: &Vec<char>, i: usize, count: usize) -> int {
    let mut v: int = 0;
    let mut k: usize = 0;
    while k < count {
        let j = i + k;
        if j >= cs.len() || cs[j] < '0' || cs[j] > '9' {
            return -1;
        }
        v = v * 10 + ((cs[j] as int) - 48);
        k += 1;
    }
    v
}

/// A match's named groups in order, each name once: a duplicate name
/// (in another alternative) takes the value of the group that matched.
fn group_pairs(names: &Vec<String>, nidx: &Vec<int>, vals: &Vec<Val>) -> Vec<(String, Val)> {
    let mut out: Vec<(String, Val)> = Vec::new();
    let mut i: usize = 0;
    while i < names.len() {
        let mut seen = false;
        let mut j: usize = 0;
        while j < i {
            if names[j].as_str() == names[i].as_str() {
                seen = true;
            }
            j += 1;
        }
        if !seen {
            let mut v = vals[nidx[i] as usize].clone();
            let mut k: usize = i + 1;
            while k < names.len() {
                if names[k].as_str() == names[i].as_str() && matches!(v, Val::Undef) {
                    v = vals[nidx[k] as usize].clone();
                }
                k += 1;
            }
            out.push((names[i].clone(), v));
        }
        i += 1;
    }
    out
}

/// Whether `s.replace(pat, rep)` / replaceAll is replace_plain's: a
/// non-empty ASCII `pat` and a `rep` without `$` (Rust; the other targets
/// take the general path).
#[ranger::target(rust)]
pub fn replace_plain_ok(pat: &str, rep: &str) -> bool {
    !pat.is_empty() && pat.is_ascii() && !rep.as_bytes().contains(&b'$')
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn replace_plain_ok(_pat: &str, _rep: &str) -> bool {
    false
}

/// `s.replace(pat, rep)` / replaceAll when replace_plain_ok: an ASCII byte
/// is never part of a longer UTF-8 sequence, so a byte match is the UTF-16
/// match.
#[ranger::target(rust)]
pub fn replace_plain(s: &str, pat: &str, rep: &str, all: bool) -> String {
    let mut out = String::new();
    let mut last = 0usize;
    while let Some(p) = s[last..].find(pat) {
        let at = last + p;
        jsstr::push_js(&mut out, &s[last..at]);
        jsstr::push_js(&mut out, rep);
        last = at + pat.len();
        if !all {
            break;
        }
    }
    jsstr::push_js(&mut out, &s[last..]);
    out
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn replace_plain(_s: &str, _pat: &str, _rep: &str, _all: bool) -> String {
    String::new()
}

/// Whether every value is a number.
pub fn all_nums(v: &Vec<Val>) -> bool {
    let n = v.len();
    let mut i: usize = 0;
    while i < n {
        if !matches!(v[i], Val::Num(_)) {
            return false;
        }
        i += 1;
    }
    true
}

/// merge_sort over numbers with `(a, b) => a - b` (desc: `b - a`) as the
/// comparator: the same merges and so the same order as calling it.
pub fn num_merge_sort(v: &mut Vec<Val>, desc: bool) {
    let n = v.len();
    let mut src: Vec<double> = Vec::with_capacity(n);
    let mut i0: usize = 0;
    while i0 < n {
        if let Val::Num(x) = v[i0] {
            src.push(x);
        }
        i0 += 1;
    }
    let mut dst: Vec<double> = src.clone();
    let mut width: usize = 1;
    while width < n {
        let mut lo: usize = 0;
        while lo < n {
            let mid = if lo + width < n { lo + width } else { n };
            let hi = if lo + 2 * width < n { lo + 2 * width } else { n };
            let mut i = lo;
            let mut j = mid;
            let mut k = lo;
            while i < mid && j < hi {
                // cmp(src[j], src[i]) < 0; NaN compares as 0
                let c = if desc { src[i] - src[j] } else { src[j] - src[i] };
                if c < 0.0 {
                    dst[k] = src[j];
                    j += 1;
                } else {
                    dst[k] = src[i];
                    i += 1;
                }
                k += 1;
            }
            while i < mid {
                dst[k] = src[i];
                i += 1;
                k += 1;
            }
            while j < hi {
                dst[k] = src[j];
                j += 1;
                k += 1;
            }
            lo += 2 * width;
        }
        let t = src;
        src = dst;
        dst = t;
        width *= 2;
        if crate::builtins::time_up(n as int) {
            // past the deadline: v as it was (Vm::check_expired)
            return;
        }
    }
    let mut i1: usize = 0;
    while i1 < n {
        v[i1] = Val::Num(src[i1]);
        i1 += 1;
    }
}

/// An identifier's length at the start of `s` (ASCII names only), 0 if none.
#[ranger::target(rust)]
fn ident_len(s: &[u8]) -> usize {
    let mut i: usize = 0;
    while i < s.len() {
        let c = s[i];
        let ok = c == b'_' || c == b'$' || (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z') || (i > 0 && c >= b'0' && c <= b'9');
        if !ok {
            break;
        }
        i += 1;
    }
    i
}

#[ranger::target(rust)]
fn word_byte(c: u8) -> bool {
    c == b'_' || c == b'$' || (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z') || (c >= b'0' && c <= b'9') || c >= 128
}

/// `t` with white space dropped, except one space between two word
/// characters (`return a` stays apart from `returna`). Rust only, like
/// sub_cmp_kind: elsewhere comparators are always called.
#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn squeeze_ws(_t: &str) -> String {
    String::new()
}

#[ranger::target(rust)]
pub fn squeeze_ws(t: &str) -> String {
    let s = t.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i: usize = 0;
    while i < s.len() {
        let c = s[i];
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            let mut j = i;
            while j < s.len() && (s[j] == b' ' || s[j] == b'\t' || s[j] == b'\n' || s[j] == b'\r') {
                j += 1;
            }
            if !out.is_empty() && j < s.len() && word_byte(out[out.len() - 1]) && word_byte(s[j]) {
                out.push(b' ');
            }
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8(out).unwrap_or(String::new())
}

/// sort_kind of a function's text after squeeze_ws:
/// `(a,b)=>a-b`, `(a,b)=>{return a-b;}`, `function f?(a,b){return a-b;}`
/// give 2, the same with `b-a` 3, anything else 1.
#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
pub fn sub_cmp_kind(_t: &str) -> i32 {
    1
}

#[ranger::target(rust)]
pub fn sub_cmp_kind(t: &str) -> i32 {
    let s = t.as_bytes();
    let mut p: usize = 0;
    let arrow = !t.starts_with("function");
    if !arrow {
        p = 8;
        if p < s.len() && s[p] == b' ' {
            p += 1;
            let ln = ident_len(&s[p..]);
            if ln == 0 {
                return 1;
            }
            p += ln;
        }
    }
    if p >= s.len() || s[p] != b'(' {
        return 1;
    }
    p += 1;
    let la = ident_len(&s[p..]);
    if la == 0 {
        return 1;
    }
    let a = &t[p..p + la];
    p += la;
    if p >= s.len() || s[p] != b',' {
        return 1;
    }
    p += 1;
    let lb = ident_len(&s[p..]);
    if lb == 0 {
        return 1;
    }
    let b = &t[p..p + lb];
    p += lb;
    if p >= s.len() || s[p] != b')' || a == b || a == "arguments" || b == "arguments" || a == "eval" || b == "eval" {
        return 1;
    }
    p += 1;
    let rest = &t[p..];
    let ab = format!("{}-{}", a, b);
    let ba = format!("{}-{}", b, a);
    let mut forms: Vec<String> = Vec::new();
    if arrow {
        forms.push(format!("=>{}", ab));
        forms.push(format!("=>{{return {};}}", ab));
        forms.push(format!("=>{{return {}}}", ab));
        forms.push(format!("=>{}", ba));
        forms.push(format!("=>{{return {};}}", ba));
        forms.push(format!("=>{{return {}}}", ba));
    } else {
        forms.push(format!("{{return {};}}", ab));
        forms.push(format!("{{return {}}}", ab));
        forms.push(format!("{{return {};}}", ba));
        forms.push(format!("{{return {}}}", ba));
    }
    let half = forms.len() / 2;
    let mut i: usize = 0;
    while i < forms.len() {
        if rest == forms[i].as_str() {
            return if i < half { 2 } else { 3 };
        }
        i += 1;
    }
    1
}

/// merge_sort of numbers with no comparator: each number's string made
/// once, not at every comparison (the same merges, the same order).
pub fn num_str_merge_sort(v: &mut Vec<Val>) {
    let n = v.len();
    let mut keys: Vec<String> = Vec::with_capacity(n);
    let mut i0: usize = 0;
    while i0 < n {
        if let Val::Num(x) = v[i0] {
            keys.push(number_to_string(x));
        }
        i0 += 1;
    }
    let mut src: Vec<usize> = Vec::with_capacity(n);
    let mut i1: usize = 0;
    while i1 < n {
        src.push(i1);
        i1 += 1;
    }
    let mut dst: Vec<usize> = src.clone();
    let mut width: usize = 1;
    while width < n {
        let mut lo: usize = 0;
        while lo < n {
            let mid = if lo + width < n { lo + width } else { n };
            let hi = if lo + 2 * width < n { lo + 2 * width } else { n };
            let mut i = lo;
            let mut j = mid;
            let mut k = lo;
            while i < mid && j < hi {
                let c = jsstr::compare(keys[src[j]].as_str(), keys[src[i]].as_str());
                if c < 0 {
                    dst[k] = src[j];
                    j += 1;
                } else {
                    dst[k] = src[i];
                    i += 1;
                }
                k += 1;
            }
            while i < mid {
                dst[k] = src[i];
                i += 1;
                k += 1;
            }
            while j < hi {
                dst[k] = src[j];
                j += 1;
                k += 1;
            }
            lo += 2 * width;
        }
        let t = src;
        src = dst;
        dst = t;
        width *= 2;
        if crate::builtins::time_up(n as int) {
            return;
        }
    }
    let old: Vec<Val> = v.clone();
    let mut i2: usize = 0;
    while i2 < n {
        v[i2] = old[src[i2]];
        i2 += 1;
    }
}

/// A regex's source as `RegExp.prototype.source` shows it: `/` and line
/// terminators escaped, "(?:)" for an empty one.
pub fn shown_source(s: &str) -> String {
    if s.is_empty() {
        String::from("(?:)")
    } else {
        s.replace("/", "\\/").replace("\\\\/", "\\/").replace("\n", "\\n").replace("\r", "\\r").replace("\u{2028}", "\\u2028").replace("\u{2029}", "\\u2029")
    }
}

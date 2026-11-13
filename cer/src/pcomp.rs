// SPDX-License-Identifier: AGPL-3.0-or-later
//! From tokens to bytecode in one pass, with no syntax tree.
//!
//! The parser writes code as it reads. What it cannot know yet is left as
//! a placeholder op (`PH_*`): a name's place (a stack slot, a slot of a
//! scope object or a property of the global object depends on whether a
//! closure read further on captures it), the scope objects a block needs,
//! the functions hoisted to the top of a block. When the whole script has
//! been read, `finish` resolves every name, places every binding and
//! writes each function's code again with the placeholders expanded, the
//! ops fused as `compiler.rs` fuses them and the jumps moved to match.
//!
//! Code that runs in another order than it is written is read again from
//! its tokens: a `for` loop's update after its body, the cases of a
//! `switch` before their statements, a `finally` block at every exit of
//! its `try`, a destructuring target after the value it receives.

use ranger::prelude::*;
use std::rc::Rc;
use std::collections::HashMap;

use crate::ast::{F_ARROW, F_ASYNC, F_CTOR, F_DECL, F_DERIVED, F_GENERATOR, F_GETTER, F_METHOD, F_SETTER, F_STATIC, F_STRICT};
use crate::lexer::*;
use crate::num::number_to_string;
use crate::ops::*;
use crate::parser::{is_reserved, is_strict_reserved};
use crate::value::*;

/// PComp.ctx: what the code being read may contain (early errors)
const CX_SUPER_CALL: int = 1;
const CX_SUPER_PROP: int = 2;
/// a field initializer or static block: no `arguments`
const CX_NO_ARGS: int = 4;
/// a static block: `await` is reserved
const CX_AWAIT: int = 8;

const K_VAR: int = 0;
const K_LET: int = 1;
const K_CONST: int = 2;
const K_FUNC: int = 3;
const K_PARAM: int = 4;
const K_CLASS: int = 5;
const K_CATCH: int = 6;
const K_ARGS: int = 7;
const K_CALLEE: int = 8;

/// a name: `a` the reference, `b` the mode (M_*), `c` the mode's extra
const PH_REF: int = 1000;
/// a scope entered: `a` the scope, `b` 1 for a function's own scope
const PH_ENTER: int = 1001;
/// a block scope left (also by a jump out of it)
const PH_LEAVE: int = 1002;
/// a `for` loop's scope object copied for the next iteration
const PH_COPY_ENV: int = 1003;
/// the function declarations of a scope, made where the scope starts
const PH_HOIST: int = 1004;
/// `var` and function names of the top level declared on the global object
const PH_DECL_GLOBALS: int = 1005;
/// a function's captured parameters, `arguments` and its own name
const PH_PROLOGUE: int = 1006;
/// body vars named like a parameter start with its value: `a` the body's
/// scope, `b` the parameters'
const PH_BODY_VARS: int = 1007;
/// Annex B: a block's function copied to the function's var
const PH_ANNEX: int = 1008;
/// nothing
const PH_NOP: int = 1009;
/// a direct eval's call: `a` its scope, `b` the argument count
const PH_EVAL: int = 1010;
/// a known function's call compiled two ways (`a` the site, KSite): the
/// inlined body from PH_KALT to PH_KMID, the call from there to PH_KEND;
/// `finish` keeps one
const PH_KALT: int = 1011;
const PH_KMID: int = 1012;
const PH_KEND: int = 1013;

const M_LOAD: int = 0;
const M_STORE: int = 1;
const M_INIT: int = 2;
const M_TYPEOF: int = 3;
const M_DELETE: int = 4;
/// `x++`: c holds U_* flags
const M_UPD: int = 5;
/// `x += v;` / `x = x + v;`: the read of x (c the store's reference) and
/// the add and store (c 1 when v assigns nothing)
const M_ADDT_LOAD: int = 6;
const M_ADDT_STORE: int = 7;
/// inside `with`, a reference taken before the right side: M_WREF pushes
/// the object that has the name (undefined: the binding), M_WLOAD reads
/// through it ([base] → [base, v]), M_WSTORE writes ([base, v] → [v])
const M_WREF: int = 8;
const M_WLOAD: int = 9;
const M_WSTORE: int = 10;

const U_PREFIX: int = 1;
const U_INC: int = 2;
/// a statement of its own: the value is not kept
const U_DISCARD: int = 4;
/// ... at the top of the script: it is the completion value
const U_TOP: int = 8;

// control-stack entries
const CT_LOOP: int = 0;
const CT_SWITCH: int = 1;
const CT_BLOCK: int = 2;
const CT_TRY: int = 3;
const CT_ENV: int = 4;
const CT_ITEM: int = 5;
const CT_ITER: int = 6;
/// an inlined function body (iife_here): `return` jumps to its end
const CT_INLINE: int = 7;

// what an expression was, for what follows it (an assignment, a call,
// a condition)
const E_OTHER: int = 0;
const E_IDENT: int = 1;
const E_MEMBER: int = 2;
const E_INDEX: int = 3;
const E_SUPER_MEMBER: int = 4;
const E_ANON: int = 5;
const E_CMP: int = 6;
const E_NOT: int = 7;
/// `x + v` with x a name: a the reference, b its op, c the assignments
/// counted before v
const E_ADD: int = 8;
const E_NUM: int = 9;
const E_ADDASSIGN: int = 10;
const E_MEMBER_ADDASSIGN: int = 11;
const E_UPDATE_IDENT: int = 12;
const E_CHAIN: int = 13;
const E_PRIVATE: int = 14;
const E_LITERAL: int = 15;
/// `#x` before `in`: a its key's atom, b its brand's (-1: none)
const E_PRIVNAME: int = 16;

/// a private name's kind: a field, a method, an accessor; static added
const PK_FIELD: int = 0;
const PK_METHOD: int = 1;
const PK_ACCESSOR: int = 2;
const PK_STATIC: int = 4;

/// before every sequence number
const SEQ_NEVER: int = 2000000000;

#[derive(Clone, Copy)]
pub struct Ex {
    pub k: int,
    pub a: int,
    pub b: int,
    pub c: int,
}

fn ex(k: int) -> Ex {
    Ex { k: k, a: -1, b: -1, c: -1 }
}

/// `s`'s parts between the bytes `sep` (none for an empty `s`).
fn split_on(s: &str, sep: int) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let n = s.as_bytes().len();
    if n == 0 {
        return out;
    }
    let mut start: usize = 0;
    let mut i: usize = 0;
    while i <= n {
        if i == n || (s.as_bytes()[i] as int) == sep {
            out.push(String::from(&s[start..i]));
            start = i + 1;
        }
        i += 1;
    }
    out
}

/// The decimal digits of `s` as a number.
fn digits_int(s: &str) -> int {
    let mut v: int = 0;
    let n = s.as_bytes().len();
    let mut i: usize = 0;
    while i < n {
        let c = s.as_bytes()[i] as int;
        if c >= 48 && c <= 57 {
            v = v * 10 + (c - 48);
        }
        i += 1;
    }
    v
}

pub struct PBind {
    pub name: String,
    pub kind: int,
    pub captured: bool,
    pub slot: int,
    pub in_env: bool,
    pub scope: int,
    pub global: bool,
    pub placed: bool,
    pub tdz: bool,
    /// the sequence number from which the binding is initialized (reads
    /// before it check for the uninitialized marker)
    pub init_seq: int,
}

pub struct PScope {
    pub parent: int,
    pub is_func: bool,
    pub is_arrow: bool,
    pub is_program: bool,
    /// the scope of a function declaration, which may run before the
    /// declarations of the scopes around it
    pub decl_fn: bool,
    pub is_switch: bool,
    pub names: HashMap<String, int>,
    pub binds: Vec<int>,
    pub env_size: int,
    pub has_env: bool,
    /// the function (PFn) whose code enters it
    pub fnid: int,
    /// a `with` statement's scope: its object's binding
    pub with_bind: int,
    pub hoists: Vec<int>,
    /// the `var` names declared in it or in the blocks inside it (early
    /// errors: a lexical declaration of the same name)
    pub vthru: HashMap<String, bool>,
    /// a function body's scope (parameters with expressions): the scope of
    /// its parameters
    pub params_scope: int,
    /// the async functions and generators declared in it (a block's plain
    /// functions alone may share a name, in sloppy code)
    pub odd_fns: HashMap<String, bool>,
    /// a direct eval inside may name its bindings: all go in its object
    pub has_eval: bool,
}

pub struct PRef {
    /// the name's index in `names`
    pub nid: i32,
    pub scope: i32,
    pub seq: i32,
    /// the name a let / const / parameter declares (not a reference)
    pub decl: bool,
    /// a binding known when the reference was made
    pub direct: i32,
    pub bind: i32,
    pub with_list: i32,
    pub tdz: bool,
    pub param_tdz: bool,
}

pub struct PHoist {
    pub proto: int,
    pub name: String,
}

pub struct PAnnex {
    pub scope: int,
    pub name: String,
    pub proto: int,
    pub var_bind: int,
}

/// a function, from its start to `finish`
pub struct PFn {
    pub fscope: int,
    pub scopes: Vec<int>,
    pub param_binds: Vec<int>,
    pub base: int,
    pub is_program: bool,
    /// its index in `protos`
    pub proto: int,
    pub str_consts: HashMap<String, int>,
    pub num_consts: HashMap<String, int>,
    /// sloppy with simple parameters: its arguments object's elements
    /// are the parameters (mapped)
    pub mapped: bool,
}

struct PCtl {
    kind: int,
    labels: Vec<String>,
    breaks: Vec<int>,
    conts: Vec<int>,
    /// a try's finally block: the token of its `{`, -1 for none
    fin: int,
    installed: bool,
    scope: int,
    /// a `using` region (fin -2): its resource list's binding, and
    /// whether disposing awaits
    uname: String,
    uasync: bool,
}

/// The statements after a `using` declaration up to the end of its list:
/// a try whose exits dispose of the list's resources
/// A function declared once and only called (never written), whose calls
/// with a closure argument may be inlined (`kcall_here`)
struct KFn {
    decl: int,
    params: Vec<String>,
    /// its body's braces
    bo: int,
    bc: int,
}

/// An inlined call of a KFn: which of its two codes `finish` keeps
struct KSite {
    name: String,
    scope: int,
    strict: bool,
    /// the scopes made from here on are the inlined body's
    s0: int,
    /// refs: the inlined code's from r0, the call's from ra1 to rb1;
    /// scopes the same from s0, sa1, sb1
    r0: int,
    sa1: int,
    sb1: int,
    ra1: int,
    rb1: int,
    /// the refs of the body's own tokens: from cb0 to cb1, but for the
    /// closure's body (pairs in `ex`)
    cb0: int,
    cb1: int,
    ex: Vec<int>,
    /// 1 the inlined code, 0 the call
    pick: int,
}

/// The closure parameter of an inlined body: its call `name()` compiles
/// the closure's body (tokens `body` to `body_end`) in `scope`, the
/// call site's
struct KSub {
    name: String,
    bo: int,
    bc: int,
    body: int,
    body_end: int,
    scope: int,
    active: bool,
    hits: int,
    site: int,
}

struct URegion {
    list: String,
    err: String,
    at: int,
    is_async: bool,
}

struct PState {
    proto: Proto,
    fnid: int,
    ctl: Vec<PCtl>,
    chain: Vec<Vec<int>>,
    pending_labels: Vec<String>,
    is_program: bool,
    /// where its `var`s go
    var_scope: int,
    /// Annex B candidates: function declarations in its blocks
    annex: Vec<int>,
    /// parser state of the code around it
    saved_in_function: bool,
    saved_in_generator: bool,
    saved_in_async: bool,
    saved_strict: bool,
    saved_scope: int,
    saved_ctx: int,
}

fn new_ctl(kind: int) -> PCtl {
    PCtl { kind: kind, labels: Vec::new(), breaks: Vec::new(), conts: Vec::new(), fin: -1, installed: false, scope: -1, uname: String::new(), uasync: false }
}

fn binary_prec(op: &str) -> int {
    if op == "??" {
        return 1;
    }
    if op == "||" {
        return 2;
    }
    if op == "&&" {
        return 3;
    }
    if op == "|" {
        return 4;
    }
    if op == "^" {
        return 5;
    }
    if op == "&" {
        return 6;
    }
    if op == "==" || op == "!=" || op == "===" || op == "!==" {
        return 7;
    }
    if op == "<" || op == ">" || op == "<=" || op == ">=" || op == "instanceof" || op == "in" {
        return 8;
    }
    if op == "<<" || op == ">>" || op == ">>>" {
        return 9;
    }
    if op == "+" || op == "-" {
        return 10;
    }
    if op == "*" || op == "/" || op == "%" {
        return 11;
    }
    if op == "**" {
        return 12;
    }
    -1
}

fn is_assign_op(op: &str) -> bool {
    op == "="
        || op == "+="
        || op == "-="
        || op == "*="
        || op == "/="
        || op == "%="
        || op == "**="
        || op == "<<="
        || op == ">>="
        || op == ">>>="
        || op == "&="
        || op == "|="
        || op == "^="
        || op == "&&="
        || op == "||="
        || op == "??="
}

fn binary_code(op: &str) -> int {
    match op {
        "+" => OP_ADD,
        "-" => OP_SUB,
        "*" => OP_MUL,
        "/" => OP_DIV,
        "%" => OP_MOD,
        "**" => OP_EXP,
        "&" => OP_BAND,
        "|" => OP_BOR,
        "^" => OP_BXOR,
        "<<" => OP_SHL,
        ">>" => OP_SHR,
        ">>>" => OP_USHR,
        "==" => OP_EQ,
        "!=" => OP_NE,
        "===" => OP_SEQ,
        "!==" => OP_SNE,
        "<" => OP_LT,
        ">" => OP_GT,
        "<=" => OP_LE,
        ">=" => OP_GE,
        "instanceof" => OP_INSTANCEOF,
        "in" => OP_IN,
        _ => OP_NOP,
    }
}

fn is_compare(code: int) -> bool {
    code == OP_LT || code == OP_GT || code == OP_LE || code == OP_GE || code == OP_SEQ || code == OP_SNE || code == OP_EQ || code == OP_NE
}

/// Whether the op also jumps to `b`.
fn jumps_b(c: int) -> bool {
    c == OP_YIELD_STAR || c == OP_ASTAR_CALL || c == OP_ASTAR_DONE
}

/// Whether the op jumps to `a` (and, for YIELD_STAR, to `b`).
fn is_jump(op: &Op) -> bool {
    let c = op.code;
    (c as int) == OP_JUMP
        || (c as int) == OP_JF
        || (c as int) == OP_JT
        || (c as int) == OP_JNOT_UNDEF
        || (c as int) == OP_JF_KEEP
        || (c as int) == OP_JT_KEEP
        || (c as int) == OP_JNN_KEEP
        || (c as int) == OP_JNULLISH
        || (c as int) == OP_CMP_JF
        || (c as int) == OP_LOCAL_NULL_JF
        || (c as int) == OP_CASE_INT_JT
        || (c as int) == OP_CASE_JT
        || (c as int) == OP_ITER_NEXT
        || ((c as int) == OP_TEMPLATE_OBJ && op.b >= 0)
        || (c as int) == OP_GEN_RESUME
        || (c as int) == OP_YIELD_STAR
        || (c as int) == OP_ASTAR_CALL
        || (c as int) == OP_ASTAR_DONE
        || (c as int) == OP_TRY
}

/// A token as pcomp keeps it: its text (or number) by index into
/// PComp.names (PComp.nums), so that a script's tokens stay small.
pub struct PTok {
    pub kind: i32,
    /// where the token starts and ends in the source (chars)
    pub start: i32,
    pub end: i32,
    /// the text's index in `names`
    pub v: i32,
    /// a template's or a regular expression's extra (xs), a number's
    /// value in `nums`, -1 for none
    pub x: i32,
    /// PT_NL, PT_ESCAPED
    pub flags: i32,
}

/// a line break before the token
pub const PT_NL: i32 = 1;
/// an identifier written with an escape, or a string with an octal escape
pub const PT_ESCAPED: i32 = 2;

pub struct PComp {
    toks: Vec<PTok>,
    /// the tokens' texts, each once (`name_ids` finds one)
    names: Vec<String>,
    name_ids: HashMap<String, int>,
    /// name_id's cache: a text's cheap hash to its index, -1 for none
    name_cache: Vec<int>,
    /// the number tokens' values
    nums: Vec<double>,
    /// the token closing each bracket (and the one opening each closing
    /// one), -1 for others
    mate: Vec<i32>,
    xs: Vec<TokX>,
    pos: int,
    /// counts tokens read, re-reads included: what came first
    seq: int,
    /// the source the tokens come from, shared by the functions' protos
    src_rc: Rc<String>,
    pub error: String,

    pub atoms: HashMap<String, int>,
    pub atom_names: Vec<String>,
    pub protos: Vec<Proto>,
    pub proto_base: int,
    pub local_program: bool,
    /// strict eval code: a "use strict" of its own also keeps its
    /// declarations local
    pub local_if_strict: bool,
    keep_completion: bool,
    /// TypeScript: types are read and skipped (`type_end`)
    pub ts: bool,
    /// JSX: elements are the factory's calls (the lexer writes them)
    jsx: bool,
    jsx_factory: String,
    jsx_fragment: String,
    /// a module's file (`compile_module`): its imports name it, and its
    /// exports (names, the expressions that read them, `export *` modules)
    /// are what it returns
    pub module_path: String,
    /// a host's script (CErXes `Engine::compile`) may `import` the host's
    /// modules and keep its `export`ed declarations, as the parser reads it
    pub script_imports: bool,
    /// direct eval code: the caller's environments (`x_eval_desc`)
    pub outer_desc: String,
    /// the scopes of direct eval calls
    eval_sites: Vec<int>,
    /// per binding: some code stores to it after it is declared (a
    /// mapped parameter that never changes needs no mapping until the
    /// arguments object itself is written)
    bind_stored: Vec<bool>,
    /// a direct eval site's private environment ("p|…" desc parts)
    eval_privs: HashMap<int, String>,
    /// `using` regions open, the ones of the list being read from
    /// `using_mark`; whether the statement being read may be a `using`
    /// declaration, and the token ending its list
    usings: Vec<URegion>,
    using_mark: int,
    using_here: bool,
    using_end: int,
    using_count: int,
    /// sloppy direct eval code in a function: the caller's environments
    /// from the innermost to the function's, where its vars may already be
    eval_var_scopes: Vec<int>,
    /// sloppy direct eval code in a function: the binding of the object
    /// that keeps the vars it adds (the function's `%ev`), -1 for none
    eval_ev: int,
    /// direct eval code in a function's parameters / where new.target is
    /// allowed
    eval_in_params: bool,
    eval_new_target: bool,
    /// direct eval code: the caller's CX_* bits; its program scope
    eval_ctx: int,
    eval_prog: int,
    /// strict code (a strict caller's direct eval)
    pub force_strict: bool,
    /// the next statement is the body of an if or a loop (labels and
    /// all): no labelled function there
    sub_label: bool,
    mex_names: Vec<String>,
    mex_values: Vec<String>,
    mex_stars: Vec<String>,
    /// the type reader's token and how many `>` of a `>>` it has read
    tp: int,
    tg: int,
    tbad: bool,

    scopes: Vec<PScope>,
    binds: Vec<PBind>,
    refs: Vec<PRef>,
    hoists: Vec<PHoist>,
    annexes: Vec<PAnnex>,
    fns: Vec<PFn>,
    with_lists: Vec<Vec<int>>,
    has_with: bool,
    with_count: int,
    fs: Vec<PState>,
    cur_scope: int,

    // parser state
    no_in: bool,
    /// compiling a module's body (esm.rs: `function* __cerxesModuleBody`):
    /// its scope, its depth in fs, and whether its pause (`yield;`) was read
    module_code: bool,
    module_scope: int,
    module_depth: usize,
    module_yielded: bool,
    in_function: bool,
    /// the depth of fs inside a class static block (no `return`), else -1
    static_depth: int,
    in_class: bool,
    in_generator: bool,
    in_async: bool,
    strict: bool,
    /// CX_* bits for the function being read (arrows inherit them)
    ctx: int,
    /// the binary expression read last is a `&&` / `||` one
    logic_top: bool,
    /// per class being read: its private names ("g"/"s"/"o" + static
    /// "1"/"0" + name) and the ones its code refers to
    priv_decl: Vec<Vec<String>>,
    priv_refs: Vec<Vec<String>>,
    /// per class being read: its id in the unit, its private names
    /// ("\u{1}#x") and their kinds (PK_*, + PK_STATIC); a name's key is
    /// "\u{1}#x\u{2}<id>" (unit::link renumbers the ids per engine)
    priv_cls: Vec<int>,
    priv_nm: Vec<Vec<String>>,
    priv_kd: Vec<Vec<int>>,
    pub priv_next: int,
    /// catch scopes whose parameter is a pattern (Annex B: a block
    /// function of a name it binds is not copied to the var)
    catch_patterns: Vec<int>,
    /// reading a (non-arrow) function's parameters
    in_params: bool,
    /// computed field keys so far (their bindings' names)
    fk_count: int,
    member_start: int,
    /// the statement being read stands directly in a statement list
    list_stmt: bool,
    /// assignments and updates read so far
    assigns: int,
    /// the value of the number literal read last
    last_num: double,
    /// bindings a declaration made, initialized once it is read
    pending_inits: Vec<int>,
    /// the names of the parameters being read
    param_names: Vec<String>,
    /// the name of the function read last
    last_fn_name: String,
    /// the field initializers of classes being read, set aside while
    /// their other members are read (by id)
    parked: Vec<PState>,
    parked_ids: Vec<int>,
    park_count: int,

    // the code being expanded (finish)
    xo: Vec<Op>,
    xfixed: Vec<bool>,
    xlabel: int,
    xpi: usize,
    xf: int,

    // known functions inlined at calls with a closure argument
    /// the program's own tokens (template substitutions' come after)
    main_toks: int,
    /// 0 not looked at, 1 may inline, -1 not (`with`, a direct eval)
    kstate: int,
    kcache: HashMap<String, int>,
    kfns: Vec<KFn>,
    ksites: Vec<KSite>,
    ksubs: Vec<KSub>,
}

impl PComp {
    pub fn new(src: &str, atoms: HashMap<String, int>, atom_names: Vec<String>, proto_base: int) -> PComp {
        PComp::new_syntax(src, atoms, atom_names, proto_base, false, "", "")
    }

    /// `new`, its JSX read as the factory's calls (crate::lexer::lexer_for).
    pub fn new_syntax(src: &str, atoms: HashMap<String, int>, atom_names: Vec<String>, proto_base: int, jsx: bool, factory: &str, fragment: &str) -> PComp {
        let mut lx = lexer_for(src, jsx, factory, fragment);
        let mut c = PComp {
            toks: Vec::new(),
            names: Vec::new(),
            name_ids: HashMap::new(),
            name_cache: Vec::new(),
            nums: Vec::new(),
            mate: Vec::new(),
            xs: Vec::new(),
            pos: 0,
            seq: 0,
            src_rc: Rc::new(String::from(src)),
            error: String::new(),
            atoms: atoms,
            atom_names: atom_names,
            protos: Vec::new(),
            proto_base: proto_base,
            local_program: false,
            local_if_strict: false,
            keep_completion: true,
            ts: false,
            jsx: jsx,
            jsx_factory: String::from(factory),
            jsx_fragment: String::from(fragment),
            module_path: String::new(),
            script_imports: false,
            outer_desc: String::new(),
            eval_sites: Vec::new(),
            bind_stored: Vec::new(),
            eval_privs: HashMap::new(),
            usings: Vec::new(),
            using_mark: 0,
            using_here: false,
            using_end: -1,
            using_count: 0,
            eval_var_scopes: Vec::new(),
            eval_ev: -1,
            eval_in_params: false,
            eval_new_target: false,
            eval_ctx: 0,
            eval_prog: -1,
            force_strict: false,
            sub_label: false,
            mex_names: Vec::new(),
            mex_values: Vec::new(),
            mex_stars: Vec::new(),
            tp: 0,
            tg: 0,
            tbad: false,
            scopes: Vec::new(),
            binds: Vec::new(),
            refs: Vec::new(),
            hoists: Vec::new(),
            annexes: Vec::new(),
            fns: Vec::new(),
            with_lists: Vec::new(),
            has_with: false,
            with_count: 0,
            fs: Vec::new(),
            cur_scope: -1,
            no_in: false,
            module_code: false,
            module_scope: -1,
            module_depth: 0,
            module_yielded: false,
            in_function: false,
            static_depth: -1,
            in_class: false,
            ctx: 0,
            logic_top: false,
            priv_decl: Vec::new(),
            priv_refs: Vec::new(),
            priv_cls: Vec::new(),
            priv_nm: Vec::new(),
            priv_kd: Vec::new(),
            priv_next: 0,
            catch_patterns: Vec::new(),
            in_params: false,
            fk_count: 0,
            in_generator: false,
            in_async: false,
            strict: false,
            member_start: 0,
            list_stmt: false,
            assigns: 0,
            last_num: 0.0,
            pending_inits: Vec::new(),
            param_names: Vec::new(),
            last_fn_name: String::new(),
            parked: Vec::new(),
            parked_ids: Vec::new(),
            park_count: 0,
            xo: Vec::new(),
            xfixed: Vec::new(),
            xlabel: -1,
            xpi: 0,
            xf: 0,
            main_toks: 0,
            kstate: 0,
            kcache: HashMap::new(),
            kfns: Vec::new(),
            ksites: Vec::new(),
            ksubs: Vec::new(),
        };
        c.lex_tokens(&mut lx, 0);
        trim_toks(&mut c.toks);
        if !lx.error.is_empty() {
            c.error = lx.error.clone();
        }
        c.xs = lx.xs;
        c.pos = 0;
        c.main_toks = c.toks.len() as int;
        c
    }

    /// The lexer's tokens after the ones there are, each kept small (PTok)
    /// as it is read, with their brackets matched; `xbase` is where their
    /// extras start in `xs`.
    fn lex_tokens(&mut self, lx: &mut Lexer, xbase: int) {
        let base = self.toks.len();
        loop {
            let t = next_tok(lx);
            let k = t.kind;
            let pt = self.small_tok(&t, xbase);
            self.toks.push(pt);
            if k == T_EOF {
                break;
            }
        }
        let n = self.toks.len();
        let mut stack: Vec<int> = Vec::new();
        let mut i: usize = base;
        while i < n {
            self.mate.push(-1);
            if (self.toks[i].kind as int) as int == T_PUNCT {
                let s = self.names[self.toks[i].v as usize].as_str();
                if s == "(" || s == "[" || s == "{" {
                    stack.push(i as int);
                } else if s == ")" || s == "]" || s == "}" {
                    if !stack.is_empty() {
                        let o = stack.pop().unwrap();
                        self.mate[o as usize] = i as i32;
                        self.mate[i] = o as i32;
                    }
                }
            }
            i += 1;
        }
    }

    fn small_tok(&mut self, t: &Tok, xbase: int) -> PTok {
        let mut flags: i32 = 0;
        if t.nl {
            flags = flags | PT_NL;
        }
        if t.escaped {
            flags = flags | PT_ESCAPED;
        }
        let v = self.name_id(t.text.as_str());
        // a number's value is in `nums`, at its `x`
        let x = if t.kind == T_NUM {
            self.nums.push(t.num);
            (self.nums.len() - 1) as int
        } else if t.x >= 0 {
            t.x + xbase
        } else {
            -1
        };
        PTok { kind: t.kind as i32, start: t.start as i32, end: t.end as i32, v: v as i32, x: x as i32, flags: flags }
    }

    fn name_id(&mut self, s: &str) -> int {
        // most texts repeat: a direct-mapped cache by a cheap hash first
        let n = s.as_bytes().len();
        let mut h: int = n as int;
        let mut i: usize = 0;
        while i < n && i < 8 {
            h = (h * 131 + (s.as_bytes()[i] as int)) & 16777215;
            i += 1;
        }
        if n > 8 {
            h = (h * 131 + (s.as_bytes()[n - 1] as int)) & 16777215;
        }
        let slot = ((h ^ (h >> 14)) & 16383) as usize;
        if self.name_cache.is_empty() {
            let mut k = 0;
            while k < 16384 {
                self.name_cache.push(-1);
                k += 1;
            }
        }
        let c = self.name_cache[slot];
        if c >= 0 && self.names[c as usize].as_str() == s {
            return c;
        }
        let id = self.name_id_slow(s);
        self.name_cache[slot] = id;
        id
    }

    fn name_id_slow(&mut self, s: &str) -> int {
        match self.name_ids.get(s) {
            Some(i) => *i,
            None => {
                let i = self.names.len() as int;
                self.names.push(String::from(s));
                self.name_ids.insert(String::from(s), i);
                i
            }
        }
    }

    /// Token `i`'s text.
    fn tok_text(&self, i: int) -> String {
        self.names[self.toks[i as usize].v as usize].clone()
    }

    /// Token `i`'s text is `s`.
    fn text_is(&self, i: int, s: &str) -> bool {
        self.names[self.toks[i as usize].v as usize].as_str() == s
    }

    fn tok_num(&self, i: int) -> double {
        self.nums[self.toks[i as usize].x as usize]
    }

    fn nl_at(&self, i: int) -> bool {
        (self.toks[i as usize].flags & PT_NL) != 0
    }

    fn tok_escaped(&self, i: int) -> bool {
        (self.toks[i as usize].flags & PT_ESCAPED) != 0
    }

    pub fn atom(&mut self, s: &str) -> int {
        match self.atoms.get(s) {
            Some(a) => *a,
            None => {
                let a = self.atom_names.len() as int;
                self.atom_names.push(String::from(s));
                self.atoms.insert(String::from(s), a);
                a
            }
        }
    }

    // =====================================================================
    // tokens

    fn kind(&self) -> int {
        self.toks[self.pos as usize].kind as int
    }

    fn text(&self) -> String {
        self.tok_text(self.pos)
    }

    /// The current token's line (for an error: counted from the source,
    /// so tokens need not carry it).
    fn line(&self) -> int {
        let start = if (self.pos as usize) < self.toks.len() { self.toks[self.pos as usize].start as int } else { 0 };
        let mut l: int = 1;
        let mut i: int = 0;
        for c in self.src_rc.chars() {
            if i >= start {
                break;
            }
            if c == '\n' {
                l += 1;
            }
            i += 1;
        }
        l
    }

    fn tok_count(&self) -> int {
        self.toks.len() as int
    }

    fn peek_kind(&self, k: int) -> int {
        let i = self.pos + k;
        if i >= self.tok_count() {
            return T_EOF;
        }
        self.toks[i as usize].kind as int
    }

    fn tok_is(&self, i: int, s: &str) -> bool {
        if i < 0 || i >= self.tok_count() {
            return false;
        }
        let t = &self.toks[i as usize];
        ((t.kind as int) == T_PUNCT || ((t.kind as int) == T_IDENT && (t.flags & PT_ESCAPED) == 0)) && self.names[t.v as usize].as_str() == s
    }

    fn peek_is(&self, k: int, s: &str) -> bool {
        self.tok_is(self.pos + k, s)
    }

    fn is(&self, s: &str) -> bool {
        self.peek_is(0, s)
    }

    fn nl_before(&self) -> bool {
        self.nl_at(self.pos)
    }

    fn tok_nl(&self, i: int) -> bool {
        if i < 0 || i >= self.tok_count() {
            return false;
        }
        self.nl_at(i)
    }

    fn escaped(&self) -> bool {
        self.tok_escaped(self.pos)
    }

    fn next(&mut self) {
        if self.pos < self.tok_count() - 1 {
            self.pos += 1;
        }
        self.seq += 1;
    }

    /// Moves to token `p` (not after an error, which stays at the end).
    fn goto(&mut self, p: int) {
        if self.error.is_empty() {
            self.pos = p;
        }
    }

    fn fail(&mut self, msg: &str) {
        if self.error.is_empty() {
            let l = self.line();
            if msg.starts_with("SyntaxError") {
                self.error = String::from(msg);
            } else {
                self.error = format!("SyntaxError: {} (line {})", msg, l);
            }
        }
        self.pos = self.tok_count() - 1;
    }

    fn expect(&mut self, s: &str) {
        if self.is(s) {
            self.next();
        } else {
            let t = self.text();
            self.fail(format!("expected '{}' but found '{}'", s, t).as_str());
        }
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.is(s) {
            self.next();
            return true;
        }
        false
    }

    fn semicolon(&mut self) {
        if self.eat(";") {
            return;
        }
        if self.is("}") || self.kind() == T_EOF || self.nl_before() {
            return;
        }
        let t = self.text();
        self.fail(format!("unexpected token '{}'", t).as_str());
    }

    fn ident_name(&mut self) -> String {
        if self.kind() == T_IDENT {
            let s = self.text();
            self.next();
            return s;
        }
        let t = self.text();
        self.fail(format!("expected a name but found '{}'", t).as_str());
        String::new()
    }

    /// `yield` / `await` / a strict mode reserved word where it cannot
    /// name anything.
    fn bad_ident(&self, w: &str) -> bool {
        if w == "yield" {
            return self.in_generator || self.strict;
        }
        if w == "await" {
            return self.in_async || self.module_code || (self.ctx & CX_AWAIT) != 0;
        }
        self.strict && !self.ts && w != "eval" && w != "arguments" && is_strict_reserved(w)
    }

    fn binding_ident(&mut self) -> String {
        if self.kind() == T_IDENT {
            let s = self.text();
            if is_reserved(s.as_str()) || (self.strict && is_strict_reserved(s.as_str())) || self.bad_ident(s.as_str()) {
                self.fail(format!("unexpected reserved word '{}'", s).as_str());
                return s;
            }
            self.next();
            return s;
        }
        let t = self.text();
        self.fail(format!("expected an identifier but found '{}'", t).as_str());
        String::new()
    }

    /// The token after the bracket at `i` closes, or -1.
    fn after_mate(&self, i: int) -> int {
        if i < 0 || i >= self.tok_count() {
            return -1;
        }
        let m = self.mate[i as usize] as int;
        if m < 0 {
            return -1;
        }
        m + 1
    }

    /// The token closing the bracket at `i`, or the last token.
    fn mate_of(&self, i: int) -> int {
        let m = if i >= 0 && i < self.tok_count() { self.mate[i as usize] as int } else { -1 };
        if m < 0 {
            return self.tok_count() - 1;
        }
        m
    }

    /// From `i`, the first token at bracket depth 0 that is one of the
    /// punctuators `stops` (or `in` / `of` with `words`), or a closing
    /// bracket of the depth around.
    fn scan_to(&self, i: int, stops: &str, words: bool) -> int {
        let n = self.tok_count();
        let mut k = i;
        while k < n {
            let t = &self.toks[k as usize];
            if (t.kind as int) == T_EOF {
                return k;
            }
            if (t.kind as int) == T_PUNCT {
                let s = self.names[t.v as usize].as_str();
                if s == "(" || s == "[" || s == "{" {
                    let m = self.mate[k as usize] as int;
                    if m < 0 {
                        return n - 1;
                    }
                    k = m + 1;
                    continue;
                }
                if s == ")" || s == "]" || s == "}" {
                    return k;
                }
                let mut j: usize = 0;
                let mut hit = false;
                for p in stops.split(' ') {
                    if p == s {
                        hit = true;
                    }
                    j += 1;
                }
                let _ = j;
                if hit {
                    return k;
                }
            } else if words && (t.kind as int) == T_IDENT && (t.flags & PT_ESCAPED) == 0 && (self.names[t.v as usize].as_str() == "in" || self.names[t.v as usize].as_str() == "of") {
                return k;
            }
            k += 1;
        }
        n - 1
    }

    // =====================================================================
    // scopes and bindings

    fn new_scope(&mut self, parent: int, is_func: bool, is_arrow: bool) -> int {
        let fnid = if self.fs.is_empty() { -1 } else { self.fs[self.fs.len() - 1].fnid };
        self.scopes.push(PScope {
            parent: parent,
            is_func: is_func,
            is_arrow: is_arrow,
            is_program: false,
            decl_fn: false,
            is_switch: false,
            names: HashMap::new(),
            binds: Vec::new(),
            env_size: 0,
            has_env: false,
            fnid: fnid,
            with_bind: -1,
            hoists: Vec::new(),
            vthru: HashMap::new(),
            params_scope: -1,
            odd_fns: HashMap::new(),
            has_eval: false,
        });
        let id = (self.scopes.len() as int) - 1;
        if fnid >= 0 {
            self.fns[fnid as usize].scopes.push(id);
        }
        id
    }

    fn declare(&mut self, scope: int, name: &str, kind: int) -> int {
        let existing = match self.scopes[scope as usize].names.get(name) {
            Some(b) => *b,
            None => -1,
        };
        if existing >= 0 {
            if kind == K_FUNC {
                self.binds[existing as usize].kind = K_FUNC;
            }
            return existing;
        }
        if (kind == K_VAR || kind == K_FUNC) && !self.eval_var_scopes.is_empty() && self.scopes[scope as usize].is_program {
            // sloppy direct eval: the caller's var of the name
            if self.eval_in_params && name == "arguments" {
                self.fail("'arguments' may not be declared by eval code in parameters");
                return -1;
            }
            let vs = self.eval_var_scopes.clone();
            for es in vs.iter() {
                let b = match self.scopes[*es as usize].names.get(name) {
                    Some(b) => *b,
                    None => -1,
                };
                if b >= 0 {
                    let bk = self.binds[b as usize].kind;
                    if bk == K_VAR || bk == K_FUNC || bk == K_PARAM || bk == K_ARGS {
                        self.scopes[scope as usize].names.insert(String::from(name), b);
                        return b;
                    }
                    if bk == K_LET || bk == K_CONST || bk == K_CLASS {
                        self.fail(format!("Identifier '{}' has already been declared", name).as_str());
                        return -1;
                    }
                }
            }
        }
        let lexical = kind == K_LET || kind == K_CONST || kind == K_CLASS;
        // eval code's lexical declarations are its own
        let global = self.scopes[scope as usize].is_program && !self.local_program && !(self.local_if_strict && lexical);
        self.binds.push(PBind {
            name: String::from(name),
            kind: kind,
            captured: false,
            slot: -1,
            in_env: false,
            scope: scope,
            global: global,
            placed: false,
            tdz: false,
            init_seq: SEQ_NEVER,
        });
        let id = (self.binds.len() as int) - 1;
        self.scopes[scope as usize].names.insert(String::from(name), id);
        self.scopes[scope as usize].binds.push(id);
        id
    }

    /// A reference to private name `s` ("\u{1}#x").
    fn priv_ref(&mut self, s: &str) {
        let n = self.priv_refs.len();
        if n == 0 {
            self.fail("Private field must be declared in an enclosing class");
            return;
        }
        self.priv_refs[n - 1].push(String::from(s));
    }

    /// The private names a class body declares (from its `{` at open),
    /// read ahead on the tokens: references may come first.
    fn priv_scan(&self, open: int) -> (Vec<String>, Vec<int>) {
        let mut names: Vec<String> = Vec::new();
        let mut kinds: Vec<int> = Vec::new();
        let close = self.mate_of(open);
        let mut k = open + 1;
        while k < close {
            if self.tok_is(k, "(") || self.tok_is(k, "[") || self.tok_is(k, "{") {
                let m = self.mate_of(k);
                if m <= k {
                    break;
                }
                k = m + 1;
                continue;
            }
            let before = k - 1;
            let after = k + 1;
            let member = self.tok_is(before, ".") || self.tok_is(before, "?.");
            let in_test = self.tok_is(after, "in");
            if (self.toks[k as usize].kind as int) == T_PRIVATE && !member && !in_test {
                let mut kind = if self.tok_is(k + 1, "(") { PK_METHOD } else { PK_FIELD };
                let mut j = k - 1;
                while self.tok_is(j, "get") || self.tok_is(j, "set") || self.tok_is(j, "async") || self.tok_is(j, "*") {
                    if self.tok_is(j, "get") || self.tok_is(j, "set") {
                        kind = PK_ACCESSOR;
                    }
                    j -= 1;
                }
                if self.tok_is(j, "static") {
                    kind += PK_STATIC;
                }
                names.push(format!("\u{1}#{}", self.tok_text(k)));
                kinds.push(kind);
            }
            k += 1;
        }
        (names, kinds)
    }

    /// Private name `s` ("\u{1}#x"): its key, its brand ("" for none: a
    /// field, a static member) and its kind; `s` itself when no class
    /// around declares it (an early error then).
    fn priv_lookup(&self, s: &str) -> (String, String, int) {
        let mut d = self.priv_cls.len();
        while d > 0 {
            d -= 1;
            let mut i: usize = 0;
            while i < self.priv_nm[d].len() {
                if self.priv_nm[d][i].as_str() == s {
                    let kind = self.priv_kd[d][i];
                    let id = self.priv_cls[d];
                    let brand = if kind == PK_METHOD || kind == PK_ACCESSOR { format!("\u{1}brand\u{2}{}", id) } else { String::new() };
                    return (format!("{}\u{2}{}", s, id), brand, kind & 3);
                }
                i += 1;
            }
        }
        (String::from(s), String::new(), PK_FIELD)
    }

    /// The private names in scope here, for a direct eval's desc: one
    /// "p|" part per class, outermost first, each name "\u{1}#x\u{2}<id>~kind".
    fn priv_env_desc(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut d: usize = 0;
        while d < self.priv_cls.len() {
            let mut items: Vec<String> = Vec::new();
            let mut i: usize = 0;
            while i < self.priv_nm[d].len() {
                items.push(format!("{}\u{2}{}~{}", self.priv_nm[d][i], self.priv_cls[d], self.priv_kd[d][i]));
                i += 1;
            }
            parts.push(format!("p|{}", items.join(",")));
            d += 1;
        }
        parts.join(";")
    }

    /// Direct eval code: the caller's private names (desc "p|" parts) as
    /// classes around the program, so that `#x` there is the caller's.
    fn priv_seed(&mut self, parts: &Vec<String>) {
        for lv in parts.iter() {
            let n = lv.as_bytes().len();
            let items = if n > 2 { String::from(&lv[2..n]) } else { String::new() };
            let mut names: Vec<String> = Vec::new();
            let mut kinds: Vec<int> = Vec::new();
            let mut cid: int = -1;
            for it in split_on(items.as_str(), 44).iter() {
                let cut = crate::jsstr::index_of(it.as_str(), "\u{2}", 0);
                let til = crate::jsstr::index_of(it.as_str(), "~", 0);
                if cut <= 0 || til <= cut {
                    continue;
                }
                names.push(crate::jsstr::slice(it.as_str(), 0, cut));
                cid = digits_int(crate::jsstr::slice(it.as_str(), cut + 1, til).as_str());
                kinds.push(digits_int(crate::jsstr::slice(it.as_str(), til + 1, crate::jsstr::len(it.as_str())).as_str()));
            }
            self.priv_cls.push(cid);
            self.priv_nm.push(names);
            self.priv_kd.push(kinds);
            self.priv_refs.push(Vec::new());
        }
    }

    /// At the end of direct eval code: a `#x` no class declares is an
    /// early error.
    fn priv_check_seeded(&mut self) {
        let mut d: usize = 0;
        while d < self.priv_refs.len() {
            let refs = self.priv_refs[d].clone();
            for r in refs.iter() {
                let (key, _b, _k) = self.priv_lookup(r.as_str());
                if key.as_str() == r.as_str() {
                    self.fail("Private field must be declared in an enclosing class");
                    return;
                }
            }
            d += 1;
        }
    }

    /// `obj.#x` read: OP_GET_PRIVATE key, brand atom (-1: none), kind.
    fn emit_priv_get(&mut self, s: &str) -> Ex {
        self.priv_ref(s);
        let (key, brand, kind) = self.priv_lookup(s);
        let at = self.atom(key.as_str());
        let bt = if brand.is_empty() { -1 } else { self.atom(brand.as_str()) };
        self.emit3(OP_GET_PRIVATE, at, bt, kind);
        Ex { k: E_PRIVATE, a: at, b: -1, c: -1 }
    }

    /// The get (or set) of the member read last as `last`: a private
    /// one's or OP_GET_PROP's (OP_SET_PROP's).
    fn emit_member_get(&mut self, last: Op) {
        if (last.code as int) == OP_GET_PRIVATE {
            self.emit3(OP_GET_PRIVATE, last.a as int, last.b as int, last.c as int);
        } else {
            self.emit(OP_GET_PROP, last.a as int, 0);
        }
    }

    fn emit_member_set(&mut self, last: Op) {
        if (last.code as int) == OP_GET_PRIVATE {
            self.emit3(OP_SET_PRIVATE, last.a as int, last.b as int, last.c as int);
        } else {
            self.emit(OP_SET_PROP, last.a as int, 0);
        }
    }

    /// A class member's private name `s`; kind 'g', 's' or 'o'.
    fn priv_declare(&mut self, s: &str, kind: &str, is_static: bool) {
        let n = self.priv_decl.len();
        if n == 0 {
            return;
        }
        let st = if is_static { "1" } else { "0" };
        let mut bad = false;
        for d in self.priv_decl[n - 1].iter() {
            if crate::jsstr::slice(d.as_str(), 2, crate::jsstr::len(d.as_str())).as_str() == s {
                let dk = crate::jsstr::slice(d.as_str(), 0, 1);
                let ds = crate::jsstr::slice(d.as_str(), 1, 2);
                let pair = ((dk.as_str() == "g" && kind == "s") || (dk.as_str() == "s" && kind == "g")) && ds.as_str() == st;
                if !pair {
                    bad = true;
                }
            }
        }
        if bad {
            self.fail("Identifier has already been declared");
            return;
        }
        self.priv_decl[n - 1].push(format!("{}{}{}", kind, st, s));
    }

    /// The class's private references checked at its end: an undeclared
    /// one goes to the class around it, or is an early error.
    fn priv_end(&mut self) {
        let nd = self.priv_decl.len() - 1;
        let decl = self.priv_decl[nd].clone();
        self.priv_decl.remove(nd);
        let nr = self.priv_refs.len() - 1;
        let refs = self.priv_refs[nr].clone();
        self.priv_refs.remove(nr);
        for r in refs.iter() {
            let mut found = false;
            for d in decl.iter() {
                if crate::jsstr::slice(d.as_str(), 2, crate::jsstr::len(d.as_str())).as_str() == r.as_str() {
                    found = true;
                    break;
                }
            }
            if !found {
                self.priv_ref(r.as_str());
            }
        }
    }

    /// Early error for a lexical declaration (let, const, class, a
    /// function in a block) of `name` in `scope`: true when it is allowed.
    fn check_lex(&mut self, scope: int, name: &str, kind: int) -> bool {
        let existing = match self.scopes[scope as usize].names.get(name) {
            Some(b) => *b,
            None => -1,
        };
        let mut bad = false;
        if existing >= 0 {
            let ek = self.binds[existing as usize].kind;
            if ek == K_ARGS || ek == K_CALLEE {
                bad = false;
            } else if kind == K_FUNC && ek == K_FUNC && !self.strict {
                bad = false;
            } else {
                bad = true;
            }
        }
        if !bad && self.scopes[scope as usize].vthru.contains_key(name) {
            bad = true;
        }
        let ps = self.scopes[scope as usize].params_scope;
        if !bad && ps >= 0 {
            if let Some(b) = self.scopes[ps as usize].names.get(name) {
                if self.binds[*b as usize].kind == K_PARAM {
                    bad = true;
                }
            }
        }
        if bad {
            self.fail(format!("Identifier '{}' has already been declared", name).as_str());
            return false;
        }
        true
    }

    /// Early error for a `var` (or a function at a body's top) of `name`
    /// declared from the current scope: true when it is allowed.
    fn check_var(&mut self, name: &str) -> bool {
        let vs = self.f().var_scope;
        let mut s = self.cur_scope;
        while s >= 0 {
            let existing = match self.scopes[s as usize].names.get(name) {
                Some(b) => *b,
                None => -1,
            };
            if existing >= 0 {
                let ek = self.binds[existing as usize].kind;
                if ek == K_LET || ek == K_CONST || ek == K_CLASS || (ek == K_FUNC && s != vs) {
                    self.fail(format!("Identifier '{}' has already been declared", name).as_str());
                    return false;
                }
            }
            if s == vs {
                // the var scope's own names say it (no vthru entry)
                break;
            }
            if !self.scopes[s as usize].vthru.contains_key(name) {
                self.scopes[s as usize].vthru.insert(String::from(name), true);
            }
            s = self.scopes[s as usize].parent;
        }
        true
    }

    /// The binding is initialized from here on (none in a switch, whose
    /// cases can be jumped into past it).
    fn mark_inited(&mut self, b: int) {
        let s = self.binds[b as usize].scope;
        if self.scopes[s as usize].is_switch {
            return;
        }
        if self.binds[b as usize].init_seq == SEQ_NEVER {
            self.binds[b as usize].init_seq = self.seq + 1;
        }
    }

    fn mark_pending(&mut self) {
        let list = self.pending_inits.clone();
        self.pending_inits.clear();
        for b in list.iter() {
            self.mark_inited(*b);
        }
    }

    fn new_ref(&mut self, name: &str, decl: bool, direct: int) -> int {
        let nid = self.name_id(name) as i32;
        self.refs.push(PRef {
            nid: nid,
            scope: self.cur_scope as i32,
            seq: self.seq as i32,
            decl: decl,
            direct: direct as i32,
            bind: -1,
            with_list: -1,
            tdz: false,
            param_tdz: false,
        });
        (self.refs.len() as int) - 1
    }

    /// The name reference r is to.
    fn ref_name(&self, r: usize) -> String {
        self.names[self.refs[r].nid as usize].clone()
    }

    /// A placeholder for the name: its op's index.
    fn emit_ref(&mut self, name: &str, mode: int, decl: bool) -> int {
        let r = self.new_ref(name, decl, -1);
        self.emit3(PH_REF, r, mode, 0)
    }

    /// A store to a binding known now.
    fn emit_bind_init(&mut self, b: int) {
        let name = self.binds[b as usize].name.clone();
        let r = self.new_ref(name.as_str(), true, b);
        self.emit3(PH_REF, r, M_INIT, 0);
    }

    fn lookup_here(&self, scope: int, name: &str) -> int {
        let mut s = scope;
        while s >= 0 {
            if let Some(b) = self.scopes[s as usize].names.get(name) {
                return *b;
            }
            s = self.scopes[s as usize].parent;
        }
        -1
    }

    fn lookup(&mut self, scope: int, name: &str) -> int {
        let mut s = scope;
        let mut crossed = false;
        loop {
            let found = match self.scopes[s as usize].names.get(name) {
                Some(b) => *b,
                None => -1,
            };
            if found >= 0 {
                if crossed {
                    self.binds[found as usize].captured = true;
                }
                return found;
            }
            let is_fn = self.scopes[s as usize].is_func;
            if is_fn && !self.scopes[s as usize].is_arrow && !self.scopes[s as usize].is_program && name == "arguments" {
                let b = self.declare(s, "arguments", K_ARGS);
                if crossed {
                    self.binds[b as usize].captured = true;
                }
                return b;
            }
            if is_fn {
                crossed = true;
            }
            let p = self.scopes[s as usize].parent;
            if p < 0 {
                return -1;
            }
            s = p;
        }
    }

    fn resolve_ref(&mut self, r: usize) {
        let direct = (self.refs[r].direct as int);
        if direct >= 0 {
            self.refs[r].bind = (direct) as i32;
            return;
        }
        let name = self.ref_name(r);
        let scope = (self.refs[r].scope as int);
        let decl = self.refs[r].decl;
        let seq = (self.refs[r].seq as int);
        let b = self.lookup(scope, name.as_str());
        let global = b >= 0 && self.binds[b as usize].global;
        if self.has_with && !decl {
            let target = if b >= 0 && !global { self.binds[b as usize].scope } else { -1 };
            let mut list: Vec<int> = Vec::new();
            let mut s = scope;
            while s >= 0 && s != target {
                let wb = self.scopes[s as usize].with_bind;
                if wb >= 0 {
                    let wn = self.binds[wb as usize].name.clone();
                    // a direct eval's vars (`%ev`): that function's own
                    let x = if wn.as_str() == "%ev" { wb } else { self.lookup(scope, wn.as_str()) };
                    if x >= 0 {
                        list.push(x);
                    }
                }
                s = self.scopes[s as usize].parent;
            }
            if !list.is_empty() {
                self.with_lists.push(list);
                self.refs[r].with_list = ((self.with_lists.len() as int) - 1) as i32;
            }
        }
        if b < 0 || global {
            self.refs[r].bind = (-1) as i32;
            return;
        }
        self.refs[r].bind = (b) as i32;
        let kind = self.binds[b as usize].kind;
        let target = self.binds[b as usize].scope;
        let inited = seq >= self.binds[b as usize].init_seq;
        if kind == K_PARAM && !inited && !decl {
            let mut s = scope;
            let mut crossed = false;
            while s >= 0 && s != target {
                if self.scopes[s as usize].is_func {
                    crossed = true;
                }
                s = self.scopes[s as usize].parent;
            }
            if !crossed {
                self.refs[r].param_tdz = true;
            }
        }
        if (kind == K_LET || kind == K_CONST || kind == K_CLASS) && !decl {
            let mut early = !inited;
            let mut s = scope;
            while !early && s >= 0 && s != target {
                if self.scopes[s as usize].is_func && self.scopes[s as usize].decl_fn {
                    early = true;
                }
                s = self.scopes[s as usize].parent;
            }
            if early {
                self.refs[r].tdz = true;
                self.binds[b as usize].tdz = true;
            }
        }
    }

    // =====================================================================
    // writing code (placeholders stay until `finish`)

    fn f(&mut self) -> &mut PState {
        let i = self.fs.len() - 1;
        &mut self.fs[i]
    }

    fn pc(&self) -> int {
        self.fs[self.fs.len() - 1].proto.code.len() as int
    }

    fn emit3(&mut self, code: int, a: int, b: int, c: int) -> int {
        let i = self.fs.len() - 1;
        self.fs[i].proto.code.push(Op { code: (code as i32), a: (a as i32), b: (b as i32), c: (c as i32) });
        (self.fs[i].proto.code.len() as int) - 1
    }

    fn emit(&mut self, code: int, a: int, b: int) -> int {
        self.emit3(code, a, b, -1)
    }

    fn op(&mut self, code: int) {
        self.emit3(code, 0, 0, -1);
    }

    fn last_op(&self) -> Op {
        let f = &self.fs[self.fs.len() - 1];
        let n = f.proto.code.len();
        if n == 0 {
            return Op { code: -1, a: 0, b: 0, c: -1 };
        }
        f.proto.code[n - 1]
    }

    fn pop_op(&mut self) {
        let i = self.fs.len() - 1;
        self.fs[i].proto.code.pop();
    }

    fn set_op(&mut self, at: int, code: int, a: int, b: int) {
        let i = self.fs.len() - 1;
        self.fs[i].proto.code[at as usize] = Op { code: (code as i32), a: (a as i32), b: (b as i32), c: -1 };
    }

    fn patch(&mut self, at: int) {
        let here = self.pc();
        let i = self.fs.len() - 1;
        self.fs[i].proto.code[at as usize].a = (here as i32);
    }

    fn patch_to(&mut self, at: int, target: int) {
        let i = self.fs.len() - 1;
        self.fs[i].proto.code[at as usize].a = (target as i32);
    }

    fn fn_str_const(&mut self, f: int, s: &str) -> int {
        let found = match self.fns[f as usize].str_consts.get(s) {
            Some(i) => *i,
            None => -1,
        };
        if found >= 0 {
            return found;
        }
        let pi = self.fns[f as usize].proto;
        let i: int;
        if pi >= 0 {
            let p = &mut self.protos[pi as usize];
            i = p.consts.len() as int;
            let k = p.cstrs.len() as int;
            p.cstrs.push(Rc::new(String::from(s)));
            p.consts.push(Val::Str(k));
        } else {
            let top = self.fs.len() - 1;
            let p = &mut self.fs[top].proto;
            i = p.consts.len() as int;
            let k = p.cstrs.len() as int;
            p.cstrs.push(Rc::new(String::from(s)));
            p.consts.push(Val::Str(k));
        }
        self.fns[f as usize].str_consts.insert(String::from(s), i);
        i
    }

    fn str_const(&mut self, s: &str) -> int {
        let f = self.fs[self.fs.len() - 1].fnid;
        self.fn_str_const(f, s)
    }

    fn num_const(&mut self, v: double) -> int {
        let key = if v == 0.0 && 1.0 / v < 0.0 { String::from("-0") } else { number_to_string(v) };
        let f = self.fs[self.fs.len() - 1].fnid as usize;
        let found = match self.fns[f].num_consts.get(&key) {
            Some(i) => *i,
            None => -1,
        };
        if found >= 0 {
            return found;
        }
        let top = self.fs.len() - 1;
        let p = &mut self.fs[top].proto;
        let i = p.consts.len() as int;
        p.consts.push(Val::Num(v));
        self.fns[f].num_consts.insert(key, i);
        i
    }

    fn push_num(&mut self, v: double) {
        if v == ((v as int) as double) && v > -1073741824.0 && v < 1073741824.0 && !(v == 0.0 && 1.0 / v < 0.0) {
            self.emit(OP_INT, v as int, 0);
        } else {
            let c = self.num_const(v);
            self.emit(OP_CONST, c, 0);
        }
    }

    fn push_str(&mut self, s: &str) {
        let c = self.str_const(s);
        self.emit(OP_CONST, c, 0);
    }

    /// A BigInt literal (its decimal digits): a small one is a constant
    /// Val::Big, a larger one its text and OP_BIGINT.
    fn push_bigint(&mut self, digits: &str) {
        let n = crate::bigint::small_big_of(digits);
        if n >= crate::bigint::small_big_limit() {
            self.push_str(digits);
            self.op(OP_BIGINT);
            return;
        }
        let key = format!("big:{}", digits);
        let f = self.fs[self.fs.len() - 1].fnid as usize;
        let mut c = match self.fns[f].num_consts.get(&key) {
            Some(i) => *i,
            None => -1,
        };
        if c < 0 {
            let top = self.fs.len() - 1;
            let p = &mut self.fs[top].proto;
            c = p.consts.len() as int;
            p.consts.push(Val::Big(n + 0.0));
            self.fns[f].num_consts.insert(key, c);
        }
        self.emit(OP_CONST, c, 0);
    }

    /// new.target here: inside a function (not an arrow's alone), or in a
    /// direct eval's code where its caller may name it.
    fn new_target_ok(&self) -> bool {
        let mut s = self.cur_scope;
        while s >= 0 {
            let sc = &self.scopes[s as usize];
            if sc.is_program {
                return self.eval_new_target;
            }
            if sc.is_func && !sc.is_arrow {
                return !(self.module_code && s == self.module_scope);
            }
            s = sc.parent;
        }
        self.eval_new_target
    }

    /// In a derived class's constructor (or an arrow in one): `this` is
    /// its binding `%this`, uninitialized until `super(…)` returns.
    fn in_derived_ctor(&self) -> bool {
        let mut i = self.fs.len();
        while i > 0 && self.fs[i - 1].proto.arrow {
            i -= 1;
        }
        i > 0 && self.fs[i - 1].proto.derived && self.fs[i - 1].proto.class_ctor
    }

    /// `this` here: OP_THIS, or a derived constructor's checked `%this`.
    fn emit_this(&mut self) {
        if self.in_derived_ctor() {
            self.emit_ref("%this", M_LOAD, false);
        } else {
            self.op(OP_THIS);
        }
    }

    /// A derived constructor returning undefined: its `this` must be bound.
    fn ctor_this_check(&mut self) {
        let i = self.fs.len();
        if i > 0 && self.fs[i - 1].proto.derived && self.fs[i - 1].proto.class_ctor {
            self.emit_ref("%this", M_LOAD, false);
            self.op(OP_POP);
        }
    }

    fn mark_lexical_ctor(&mut self) {
        let mut i = self.fs.len();
        while i > 0 && self.fs[i - 1].proto.arrow {
            self.fs[i - 1].proto.lexical_ctor = true;
            i -= 1;
        }
    }

    /// A block-like scope: entered here, left by `leave_block`.
    fn enter_block(&mut self, scope: int) {
        self.cur_scope = scope;
        self.emit(PH_ENTER, scope, 0);
        let mut c = new_ctl(CT_ENV);
        c.scope = scope;
        self.f().ctl.push(c);
    }

    fn leave_block(&mut self, scope: int, saved: int) {
        self.emit(PH_LEAVE, scope, 0);
        self.f().ctl.pop();
        self.cur_scope = saved;
    }

    // =====================================================================
    // the end: names, places, code

    fn finish(&mut self) {
        let n = self.refs.len();
        let dead = self.kpick();
        let mut i: usize = 0;
        while i < n {
            if !dead.is_empty() && dead[2 * i] >= 0 {
                // code that is not kept (its functions are still made):
                // the names outside it are global ones, so nothing outside
                // goes in a scope object for it
                let lo = dead[2 * i];
                let hi = dead[2 * i + 1];
                let name = self.ref_name(i);
                let x = self.lookup_here((self.refs[i].scope as int), name.as_str());
                if (self.refs[i].direct as int) < 0 && (x < 0 || self.binds[x as usize].scope < lo || self.binds[x as usize].scope >= hi) {
                    self.refs[i].bind = (-1) as i32;
                } else {
                    self.resolve_ref(i);
                }
            } else {
                self.resolve_ref(i);
            }
            i += 1;
        }
        if !self.eval_sites.is_empty() {
            self.eval_captures();
        }
        let nf = self.fns.len() as int;
        self.find_stored();
        let mut f: int = 0;
        while f < nf {
            self.place(f);
            f += 1;
        }
        f = 0;
        while f < nf {
            self.expand(f);
            f += 1;
        }
    }

    /// bind_stored: the bindings some name reference stores to (anything
    /// but a read, typeof or delete; a declaration's own first store
    /// aside), and those a hoisted function declaration sets.
    fn find_stored(&mut self) {
        let mut st: Vec<bool> = Vec::new();
        let mut i: usize = 0;
        while i < self.binds.len() {
            st.push(false);
            i += 1;
        }
        let nf = self.fns.len();
        let mut f: usize = 0;
        while f < nf {
            let pi = self.fns[f].proto as usize;
            let n = self.protos[pi].code.len();
            let mut k: usize = 0;
            while k < n {
                let o = self.protos[pi].code[k];
                if (o.code as int) == PH_REF {
                    let m = o.b as int;
                    let r = o.a as usize;
                    let stores = m == M_STORE || m == M_UPD || m == M_ADDT_STORE || m == M_WSTORE || m == M_WREF || (m == M_INIT && !self.refs[r].decl);
                    let b = (self.refs[r].bind as int);
                    if stores && b >= 0 {
                        st[b as usize] = true;
                    }
                }
                k += 1;
            }
            f += 1;
        }
        let ns = self.scopes.len();
        let mut s: usize = 0;
        while s < ns {
            let hs = self.scopes[s].hoists.clone();
            for h in hs.iter() {
                let name = self.hoists[*h as usize].name.clone();
                let b = self.lookup_here(s as int, name.as_str());
                if b >= 0 {
                    st[b as usize] = true;
                }
            }
            s += 1;
        }
        self.bind_stored = st;
    }

    /// Direct evals: the bindings of the scopes around them go in the
    /// scopes' objects, lexical ones start uninitialized, and the
    /// function's `arguments` is made.
    fn eval_captures(&mut self) {
        let sites = self.eval_sites.clone();
        for site in sites.iter() {
            self.lookup(*site, "arguments");
        }
        let ns = self.scopes.len();
        let mut s: usize = 0;
        while s < ns {
            if self.scopes[s].has_eval && self.scopes[s].fnid >= 0 {
                let binds = self.scopes[s].binds.clone();
                for b in binds.iter() {
                    let bi = *b as usize;
                    if self.binds[bi].global || self.binds[bi].placed {
                        continue;
                    }
                    self.binds[bi].captured = true;
                    let k = self.binds[bi].kind;
                    if k == K_LET || k == K_CONST || k == K_CLASS {
                        self.binds[bi].tdz = true;
                    }
                }
            }
            s += 1;
        }
    }

    /// A direct eval's view of the environments from scope `site` out:
    /// "F" (in a function) or "G", then per environment, innermost first,
    /// ";" + "f" (a function's) or "b" + "|" + its bindings as
    /// "name:kind:slot", joined by ",".
    fn x_eval_desc(&self, site: int, flags: int) -> String {
        let mut levels: Vec<String> = Vec::new();
        let mut in_fn = false;
        let mut nt = false;
        let mut nt_known = false;
        let mut s = site;
        while s >= 0 {
            let si = s as usize;
            if self.scopes[si].is_func && !self.scopes[si].is_program {
                in_fn = true;
            }
            if !nt_known && self.scopes[si].is_func && (!self.scopes[si].is_arrow || self.scopes[si].is_program) {
                nt_known = true;
                nt = !self.scopes[si].is_program;
            }
            if self.scopes[si].has_env {
                let mut items: Vec<String> = Vec::new();
                for b in self.scopes[si].binds.iter() {
                    let bi = *b as usize;
                    if !self.binds[bi].in_env || self.binds[bi].global {
                        continue;
                    }
                    let nm = self.binds[bi].name.as_str();
                    if nm.is_empty() || nm.contains(",") || nm.contains(":") || nm.contains(";") || nm.contains("|") || (nm.contains("%") && nm != "%ev") || nm.contains("\u{1}") {
                        continue;
                    }
                    items.push(format!("{}:{}:{}", nm, self.binds[bi].kind, self.binds[bi].slot));
                }
                let flag = if self.scopes[si].is_func && !self.scopes[si].is_program { "f" } else { "b" };
                levels.push(format!("{}|{}", flag, items.join(",")));
            }
            s = self.scopes[si].parent;
        }
        if self.eval_new_target && !nt_known {
            nt = true;
        }
        let head = format!("{}{}{}{}", if in_fn { "F" } else { "G" }, if (flags & 1) != 0 { "P" } else { "" }, if nt { "N" } else { "" }, flags / 2);
        let pv = match self.eval_privs.get(&site) {
            Some(v) => v.clone(),
            None => String::new(),
        };
        if !pv.is_empty() {
            levels.push(pv);
        }
        if levels.is_empty() {
            return head;
        }
        format!("{};{}", head, levels.join(";"))
    }

    /// Slots for the function's bindings: parameters first, then each
    /// scope's in turn; a captured one goes in its scope's object.
    fn place(&mut self, f: int) {
        let fi = f as usize;
        let mut next_slot = self.fns[fi].base;
        let pb = self.fns[fi].param_binds.clone();
        if self.fns[fi].mapped && !pb.is_empty() && self.args_bind(self.fns[fi].fscope) >= 0 {
            // mapped parameters live in the env, where the arguments
            // object reads and writes them
            for b in pb.iter() {
                if *b >= 0 {
                    self.binds[*b as usize].captured = true;
                }
            }
        }
        let mut i: int = 0;
        for b in pb.iter() {
            let bi = *b;
            if bi >= 0 && !self.binds[bi as usize].captured && !self.binds[bi as usize].placed {
                self.binds[bi as usize].slot = i;
                self.binds[bi as usize].placed = true;
            }
            i += 1;
        }
        let scopes = self.fns[fi].scopes.clone();
        for s in scopes.iter() {
            let binds = self.scopes[*s as usize].binds.clone();
            let mut env_size: int = 0;
            for b in binds.iter() {
                let bi = *b as usize;
                if self.binds[bi].global || self.binds[bi].placed {
                    continue;
                }
                if self.binds[bi].captured {
                    self.binds[bi].in_env = true;
                    self.binds[bi].slot = env_size;
                    env_size += 1;
                } else {
                    self.binds[bi].slot = next_slot;
                    next_slot += 1;
                }
                self.binds[bi].placed = true;
            }
            self.scopes[*s as usize].env_size = env_size;
            self.scopes[*s as usize].has_env = env_size > 0;
        }
        let pi = self.fns[fi].proto as usize;
        self.protos[pi].nslots = next_slot;
        let fs = self.fns[fi].fscope;
        if !self.fns[fi].is_program || self.local_program || self.local_if_strict {
            self.protos[pi].env_size = self.scopes[fs as usize].env_size;
        }
    }

    /// The function scope's `arguments` binding, or -1.
    fn args_bind(&self, fscope: int) -> int {
        for b in self.scopes[fscope as usize].binds.iter() {
            if self.binds[*b as usize].kind == K_ARGS {
                return *b;
            }
        }
        -1
    }

    fn env_depth(&self, from: int, target: int) -> int {
        let mut d: int = 0;
        let mut s = from;
        while s >= 0 && s != target {
            if self.scopes[s as usize].has_env {
                d += 1;
            }
            s = self.scopes[s as usize].parent;
        }
        d
    }

    fn xemit(&mut self, code: int, a: int, b: int) -> int {
        let n = self.xo.len();
        if n > 0 && self.xlabel < (n as int) {
            let last = self.xo[n - 1];
            let mut fused = -1;
            let mut fa = 0;
            let mut fb = 0;
            if code == OP_GET_LOCAL && (last.code as int) == OP_GET_LOCAL {
                fused = OP_GET_LOCAL2;
                fa = last.a;
                fb = a;
            } else if code == OP_UNDEF && (last.code as int) == OP_GET_ENV {
                fused = OP_GET_ENV_UNDEF;
                fa = last.a;
                fb = (last.b as int);
            } else if code == OP_CASE_JT && (last.code as int) == OP_INT {
                fused = OP_CASE_INT_JT;
                fa = (a as i32);
                fb = (last.a as int);
            } else if code == OP_CMP_JF && (b == OP_SEQ || b == OP_SNE || b == OP_EQ || b == OP_NE) && n > 1 && self.xlabel < (n as int) - 1 {
                let prev = self.xo[n - 2];
                let mut slot = -1;
                if (prev.code as int) == OP_NULL && (last.code as int) == OP_GET_LOCAL {
                    slot = last.a;
                } else if (prev.code as int) == OP_GET_LOCAL && (last.code as int) == OP_NULL {
                    slot = prev.a;
                }
                if slot >= 0 {
                    self.xo.pop();
                    self.xfixed.pop();
                    self.xo[n - 2] = Op { code: (OP_LOCAL_NULL_JF as i32), a: (a as i32), b: slot * 256 + (b as i32), c: -1 };
                    self.xfixed[n - 2] = false;
                    return (n as int) - 2;
                }
            }
            if fused >= 0 {
                self.xo[n - 1] = Op { code: (fused as i32), a: fa, b: (fb as i32), c: -1 };
                self.xfixed[n - 1] = false;
                return (n as int) - 1;
            }
        }
        self.xo.push(Op { code: (code as i32), a: (a as i32), b: (b as i32), c: -1 });
        self.xfixed.push(false);
        (self.xo.len() as int) - 1
    }

    fn xop(&mut self, code: int) {
        if code == OP_POP {
            let n = self.xo.len();
            if n > 0 {
                let last = self.xo[n - 1].code;
                if (last as int) == OP_SET_LOCAL {
                    self.xo[n - 1].code = (OP_SET_LOCAL_POP as i32);
                } else if (last as int) == OP_SET_PROP {
                    self.xo[n - 1].code = (OP_SET_PROP_POP as i32);
                } else if (last as int) == OP_SET_ELEM {
                    self.xo[n - 1].code = (OP_SET_ELEM_POP as i32);
                }
            }
        }
        self.xemit(code, 0, 0);
    }

    /// A jump inside an expansion: points it here, for good.
    fn xpatch(&mut self, at: int) {
        let here = self.xo.len() as int;
        self.xlabel = here;
        self.xo[at as usize].a = (here as i32);
        self.xfixed[at as usize] = true;
    }

    fn xstr(&mut self, s: &str) -> int {
        let f = self.xf;
        self.fn_str_const(f, s)
    }

    fn xpush_str(&mut self, s: &str) {
        let c = self.xstr(s);
        self.xemit(OP_CONST, c, 0);
    }

    fn xload_bind(&mut self, b: int, scope: int) {
        let in_env = self.binds[b as usize].in_env;
        let slot = self.binds[b as usize].slot;
        if in_env {
            let depth = self.env_depth(scope, self.binds[b as usize].scope);
            self.xemit(OP_GET_ENV, depth, slot);
        } else {
            self.xemit(OP_GET_LOCAL, slot, 0);
        }
    }

    fn xstore_bind(&mut self, b: int, init: bool, scope: int) {
        let kind = self.binds[b as usize].kind;
        if kind == K_CALLEE && !init {
            if self.protos[self.xpi].strict {
                let nm = self.binds[b as usize].name.clone();
                let c = self.xstr(nm.as_str());
                self.xemit(OP_CONST_ERROR, c, 0);
            }
            return;
        }
        if kind == K_CONST && !init {
            let nm = self.binds[b as usize].name.clone();
            let c = self.xstr(nm.as_str());
            self.xemit(OP_CONST_ERROR, c, 0);
            return;
        }
        if self.binds[b as usize].global {
            let nm = self.binds[b as usize].name.clone();
            let a = self.atom(nm.as_str());
            if self.eval_ev >= 0 && self.binds[b as usize].scope == self.eval_prog {
                // sloppy direct eval code's var: on the caller's `%ev`
                let ev = self.eval_ev;
                self.xload_bind(ev, scope);
                self.xop(OP_SWAP);
                self.xemit(OP_SET_PROP, a, 0);
                return;
            }
            self.xemit(OP_SET_GLOBAL, a, if init { 1 } else { 0 });
            return;
        }
        let slot = self.binds[b as usize].slot;
        if self.binds[b as usize].in_env {
            let depth = self.env_depth(scope, self.binds[b as usize].scope);
            self.xemit(OP_SET_ENV, depth, slot);
        } else {
            self.xemit(OP_SET_LOCAL, slot, 0);
        }
    }

    fn xwith_lookup(&mut self, r: usize, store: bool) -> Vec<int> {
        let mut ends: Vec<int> = Vec::new();
        let li = (self.refs[r].with_list as int);
        if li < 0 {
            return ends;
        }
        let name = self.ref_name(r);
        let scope = (self.refs[r].scope as int);
        let at = self.atom(name.as_str());
        let list = self.with_lists[li as usize].clone();
        for wb in list.iter() {
            self.xpush_str(name.as_str());
            self.xload_bind(*wb, scope);
            self.xop(OP_IN);
            let jf = self.xemit(OP_JF, 0, 0);
            self.xfixed[jf as usize] = true;
            self.xload_bind(*wb, scope);
            if store {
                self.xop(OP_SWAP);
                self.xemit(OP_SET_PROP, at, 0);
            } else {
                self.xemit(OP_GET_PROP, at, 0);
            }
            let j = self.xemit(OP_JUMP, 0, 0);
            self.xfixed[j as usize] = true;
            ends.push(j);
            self.xpatch(jf);
        }
        ends
    }

    fn xload_name_here(&mut self, r: usize) {
        let name = self.ref_name(r);
        let b = (self.refs[r].bind as int);
        if b < 0 {
            if name.as_str() == "undefined" {
                self.xop(OP_UNDEF);
                return;
            }
            let a = self.atom(name.as_str());
            self.xemit(OP_GET_GLOBAL, a, 0);
            return;
        }
        if self.refs[r].param_tdz {
            self.xop(OP_TDZ);
            let c = self.xstr(name.as_str());
            self.xemit(OP_CHECK_TDZ, c, 0);
            return;
        }
        let scope = (self.refs[r].scope as int);
        self.xload_bind(b, scope);
        if self.refs[r].tdz {
            let c = self.xstr(name.as_str());
            self.xemit(OP_CHECK_TDZ, c, 0);
        }
    }

    fn xload_name(&mut self, r: usize) {
        let ends = self.xwith_lookup(r, false);
        self.xload_name_here(r);
        for j in ends.iter() {
            self.xpatch(*j);
        }
    }

    fn xstore_name(&mut self, r: usize, init: bool) {
        let ends: Vec<int> = if init { Vec::new() } else { self.xwith_lookup(r, true) };
        self.xstore_name_here(r, init);
        for j in ends.iter() {
            self.xpatch(*j);
        }
    }

    fn xstore_name_here(&mut self, r: usize, init: bool) {
        let name = self.ref_name(r);
        let b = (self.refs[r].bind as int);
        let scope = (self.refs[r].scope as int);
        if (self.refs[r].direct as int) >= 0 {
            self.xstore_bind(b, init, scope);
            return;
        }
        if b < 0 {
            let ps = if self.eval_prog >= 0 { self.eval_prog } else { 0 };
            let gb = if self.scopes.is_empty() {
                -1
            } else {
                match self.scopes[ps as usize].names.get(&name) {
                    Some(x) => *x,
                    None => -1,
                }
            };
            if gb >= 0 && self.eval_ev >= 0 && self.binds[gb as usize].global && self.binds[gb as usize].scope == ps {
                self.xstore_bind(gb, init, scope);
                return;
            }
            if gb >= 0 && self.binds[gb as usize].kind == K_CONST && !init {
                let c = self.xstr(name.as_str());
                self.xemit(OP_CONST_ERROR, c, 0);
                return;
            }
            let a = self.atom(name.as_str());
            if gb >= 0 && !init && (self.binds[gb as usize].kind == K_LET || self.binds[gb as usize].kind == K_CLASS) {
                // the script's own `let` before it runs: not there yet
                self.xemit(OP_GET_GLOBAL, a, 0);
                self.xop(OP_POP);
            }
            self.xemit(OP_SET_GLOBAL, a, if init { 1 } else { 0 });
            return;
        }
        if !init && self.refs[r].tdz {
            self.xload_bind(b, scope);
            let c = self.xstr(name.as_str());
            self.xemit(OP_CHECK_TDZ, c, 0);
            self.xop(OP_POP);
        }
        self.xstore_bind(b, init, scope);
    }

    /// A plain slot of this frame: no scope object, no check, no `with`.
    fn ref_local(&self, r: usize) -> bool {
        let b = (self.refs[r].bind as int);
        b >= 0 && !self.binds[b as usize].in_env && !self.binds[b as usize].global && !self.refs[r].tdz && (self.refs[r].with_list as int) < 0 && !self.refs[r].param_tdz
    }

    fn expand_ref(&mut self, op: Op, top_completion: bool) {
        let r = op.a as usize;
        let mode = op.b;
        if (mode as int) == M_LOAD {
            self.xload_name(r);
            return;
        }
        if (mode as int) == M_STORE {
            self.xstore_name(r, false);
            return;
        }
        if (mode as int) == M_INIT {
            self.xstore_name(r, true);
            return;
        }
        if (mode as int) == M_WREF {
            let li = (self.refs[r].with_list as int);
            if li < 0 {
                return;
            }
            let name = self.ref_name(r);
            let scope = (self.refs[r].scope as int);
            let list = self.with_lists[li as usize].clone();
            let mut ends: Vec<int> = Vec::new();
            for wb in list.iter() {
                self.xpush_str(name.as_str());
                self.xload_bind(*wb, scope);
                self.xop(OP_IN);
                let jf = self.xemit(OP_JF, 0, 0);
                self.xfixed[jf as usize] = true;
                self.xload_bind(*wb, scope);
                let j = self.xemit(OP_JUMP, 0, 0);
                self.xfixed[j as usize] = true;
                ends.push(j);
                self.xpatch(jf);
            }
            self.xop(OP_UNDEF);
            for j in ends.iter() {
                self.xpatch(*j);
            }
            return;
        }
        if (mode as int) == M_WLOAD {
            if (self.refs[r].with_list as int) < 0 {
                self.xload_name_here(r);
                return;
            }
            let at = self.atom(self.ref_name(r).as_str());
            self.xop(OP_DUP);
            let jf = self.xemit(OP_JF, 0, 0);
            self.xfixed[jf as usize] = true;
            self.xop(OP_DUP);
            self.xemit(OP_GET_PROP, at, 0);
            let j = self.xemit(OP_JUMP, 0, 0);
            self.xfixed[j as usize] = true;
            self.xpatch(jf);
            self.xload_name_here(r);
            self.xpatch(j);
            return;
        }
        if (mode as int) == M_WSTORE {
            if (self.refs[r].with_list as int) < 0 {
                self.xstore_name_here(r, false);
                return;
            }
            let at = self.atom(self.ref_name(r).as_str());
            self.xop(OP_OVER);
            let jf = self.xemit(OP_JF, 0, 0);
            self.xfixed[jf as usize] = true;
            if op.c == 1 {
                // strict: the binding must still be there
                let name = self.ref_name(r);
                self.xop(OP_OVER);
                self.xpush_str(name.as_str());
                self.xop(OP_SWAP);
                self.xop(OP_IN);
                let jt = self.xemit(OP_JT, 0, 0);
                self.xfixed[jt as usize] = true;
                let c = self.xstr(name.as_str());
                self.xemit(OP_CONST_ERROR, c, 1);
                self.xpatch(jt);
            }
            self.xemit(OP_SET_PROP, at, 0);
            let j = self.xemit(OP_JUMP, 0, 0);
            self.xfixed[j as usize] = true;
            self.xpatch(jf);
            self.xop(OP_SWAP);
            self.xop(OP_POP);
            self.xstore_name_here(r, false);
            self.xpatch(j);
            return;
        }
        if (mode as int) == M_TYPEOF {
            let b = (self.refs[r].bind as int);
            let name = self.ref_name(r);
            if (self.refs[r].with_list as int) >= 0 {
                let ends = self.xwith_lookup(r, false);
                if b >= 0 {
                    self.xload_name_here(r);
                } else {
                    let at = self.atom(name.as_str());
                    self.xemit(OP_TYPEOF_GLOBAL, at, 0);
                    let j = self.xemit(OP_JUMP, 0, 0);
                    self.xfixed[j as usize] = true;
                    for e in ends.iter() {
                        self.xpatch(*e);
                    }
                    self.xop(OP_TYPEOF);
                    self.xpatch(j);
                    return;
                }
                for e in ends.iter() {
                    self.xpatch(*e);
                }
                self.xop(OP_TYPEOF);
                return;
            }
            if b < 0 {
                let at = self.atom(name.as_str());
                self.xemit(OP_TYPEOF_GLOBAL, at, 0);
                return;
            }
            self.xload_name(r);
            self.xop(OP_TYPEOF);
            return;
        }
        if (mode as int) == M_DELETE {
            // inside `with`: the object that has the name loses it
            let mut ends: Vec<int> = Vec::new();
            let li = (self.refs[r].with_list as int);
            if li >= 0 {
                let name = self.ref_name(r);
                let scope = (self.refs[r].scope as int);
                let at = self.atom(name.as_str());
                let list = self.with_lists[li as usize].clone();
                for wb in list.iter() {
                    self.xpush_str(name.as_str());
                    self.xload_bind(*wb, scope);
                    self.xop(OP_IN);
                    let jf = self.xemit(OP_JF, 0, 0);
                    self.xfixed[jf as usize] = true;
                    self.xload_bind(*wb, scope);
                    self.xemit(OP_DEL_PROP, at, 0);
                    let j = self.xemit(OP_JUMP, 0, 0);
                    self.xfixed[j as usize] = true;
                    ends.push(j);
                    self.xpatch(jf);
                }
            }
            if (self.refs[r].bind as int) >= 0 {
                self.xop(OP_FALSE);
            } else {
                let name = self.ref_name(r);
                let g = self.atom("globalThis");
                self.xemit(OP_GET_GLOBAL, g, 0);
                let at = self.atom(name.as_str());
                self.xemit(OP_DEL_PROP, at, 0);
            }
            for e in ends.iter() {
                self.xpatch(*e);
            }
            return;
        }
        if (mode as int) == M_UPD {
            let flags = op.c;
            let prefix = (flags & (U_PREFIX as i32)) != 0;
            let inc = (flags & (U_INC as i32)) != 0;
            let discard = (flags & (U_DISCARD as i32)) != 0;
            let b = (self.refs[r].bind as int);
            if self.ref_local(r) && self.binds[b as usize].kind != K_CONST {
                let slot = self.binds[b as usize].slot;
                let delta = if inc { 1 } else { -1 };
                if discard {
                    self.xemit(OP_INC_LOCAL, slot, delta);
                } else {
                    self.xemit(if prefix { OP_PREINC_LOCAL } else { OP_POSTINC_LOCAL }, slot, delta);
                }
                return;
            }
            let code = if inc { OP_INC } else { OP_DEC };
            self.xload_name(r);
            self.xop(OP_TONUM);
            if prefix {
                self.xop(code);
                self.xstore_name(r, false);
            } else {
                self.xop(OP_DUP);
                self.xop(code);
                self.xstore_name(r, false);
                self.xop(OP_POP);
            }
            if discard {
                if (flags & (U_TOP as i32)) != 0 {
                    self.xemit(OP_SET_LOCAL, 0, 0);
                }
                self.xop(OP_POP);
            }
            return;
        }
        if (mode as int) == M_ADDT_LOAD || (mode as int) == M_ADDT_STORE {
            let sr = if (mode as int) == M_ADDT_LOAD { (op.c / 2) as usize } else { r };
            let b = (self.refs[sr].bind as int);
            let ok = b >= 0
                && !self.binds[b as usize].global
                && self.binds[b as usize].kind != K_CONST
                && self.binds[b as usize].kind != K_CALLEE
                && !self.refs[sr].tdz
                && (self.refs[sr].with_list as int) < 0;
            let in_env = ok && self.binds[b as usize].in_env;
            let pure = if (mode as int) == M_ADDT_LOAD { (op.c % 2) == 1 } else { op.c == 1 };
            if (mode as int) == M_ADDT_LOAD {
                if ok && in_env {
                    let slot = self.binds[b as usize].slot;
                    let depth = self.env_depth((self.refs[sr].scope as int), self.binds[b as usize].scope);
                    self.xemit(OP_GET_ENV, depth, slot);
                } else if ok && pure {
                    // nothing: ADD_LOCAL_POP reads x
                } else {
                    self.xload_name(r);
                }
                return;
            }
            let slot = if b >= 0 { self.binds[b as usize].slot } else { 0 };
            if ok && in_env {
                let depth = self.env_depth((self.refs[sr].scope as int), self.binds[b as usize].scope);
                self.xemit(OP_ADD_ENV_POP, depth, slot);
            } else if ok && pure {
                self.xemit(OP_ADD_LOCAL_POP, slot, 0);
            } else {
                self.xop(OP_ADD);
                self.xstore_name(r, false);
                if top_completion {
                    self.xemit(OP_SET_LOCAL, 0, 0);
                }
                self.xop(OP_POP);
            }
            return;
        }
    }

    fn expand(&mut self, f: int) {
        let pi = self.fns[f as usize].proto as usize;
        self.xpi = pi;
        self.xf = f;
        let code = self.protos[pi].code.clone();
        let n = code.len();
        let mut target: Vec<bool> = Vec::new();
        let mut i: usize = 0;
        while i <= n {
            target.push(false);
            i += 1;
        }
        for o in code.iter() {
            if is_jump(o) {
                if o.a >= 0 && (o.a as usize) <= n {
                    target[o.a as usize] = true;
                }
                if jumps_b((o.code as int)) && o.b >= 0 && (o.b as usize) <= n {
                    target[o.b as usize] = true;
                }
            }
        }
        let mut map: Vec<int> = Vec::new();
        i = 0;
        while i <= n {
            map.push(0);
            i += 1;
        }
        self.xo = Vec::new();
        self.xfixed = Vec::new();
        self.xlabel = -1;
        i = 0;
        while i < n {
            if target[i] {
                self.xlabel = self.xo.len() as int;
            }
            map[i] = self.xo.len() as int;
            let o = code[i];
            let c = o.code;
            if (c as int) == PH_REF {
                // a local's property: fused
                if (o.b as int) == M_LOAD && i + 1 < n && (code[i + 1].code as int) == OP_GET_PROP && !target[i + 1] && self.ref_local(o.a as usize) {
                    let b = (self.refs[o.a as usize].bind as int);
                    let slot = self.binds[b as usize].slot;
                    self.xemit(OP_GET_LOCAL_PROP, slot, (code[i + 1].a as int));
                    map[i + 1] = map[i];
                    i += 2;
                    continue;
                }
                let top = (o.b as int) == M_ADDT_STORE && (o.c & 2) != 0;
                let mut o2 = o;
                if (o.b as int) == M_ADDT_STORE {
                    o2.c = o.c & 1;
                }
                self.expand_ref(o2, top);
            } else if (c as int) == OP_THIS && i + 1 < n && (code[i + 1].code as int) == OP_GET_PROP && !target[i + 1] {
                self.xemit(OP_GET_THIS_PROP, (code[i + 1].a as int), 0);
                map[i + 1] = map[i];
                i += 2;
                continue;
            } else if (c as int) == PH_ENTER {
                self.x_enter((o.a as int), o.b == 1);
            } else if (c as int) == PH_LEAVE {
                if self.scopes[o.a as usize].has_env {
                    self.xop(OP_POP_ENV);
                }
            } else if (c as int) == PH_COPY_ENV {
                if self.scopes[o.a as usize].has_env {
                    self.xop(OP_COPY_ENV);
                }
            } else if (c as int) == PH_HOIST {
                self.x_hoist((o.a as int));
            } else if (c as int) == PH_DECL_GLOBALS {
                if !self.local_program && self.eval_ev >= 0 {
                    self.x_ev_decls((o.a as int));
                } else if !self.local_program {
                    let binds = self.scopes[o.a as usize].binds.clone();
                    for b in binds.iter() {
                        let k = self.binds[*b as usize].kind;
                        if k == K_VAR || k == K_FUNC {
                            let nm = self.binds[*b as usize].name.clone();
                            let a = self.atom(nm.as_str());
                            // eval code's vars and functions can be deleted
                            self.xemit(OP_DECL_GLOBAL, a, if self.local_if_strict { 1 } else { 0 });
                        }
                    }
                }
            } else if (c as int) == PH_PROLOGUE {
                self.x_prologue((o.a as int), f);
            } else if (c as int) == PH_BODY_VARS {
                self.x_body_vars((o.a as int), (o.b as int));
            } else if (c as int) == PH_ANNEX {
                self.x_annex((o.a as int));
            } else if (c as int) == PH_NOP {
                // nothing
            } else if (c as int) == PH_KALT || (c as int) == PH_KMID {
                let k = o.a as usize;
                let keep = if (c as int) == PH_KALT { 1 } else { 0 };
                if self.ksites[k].pick != keep {
                    // the code not kept, to its end mark
                    let stop = if (c as int) == PH_KALT { PH_KMID } else { PH_KEND };
                    let mut j = i;
                    while j < n && !((code[j].code as int) == stop && (code[j].a as usize) == k) {
                        if target[j] {
                            self.xlabel = self.xo.len() as int;
                        }
                        map[j] = self.xo.len() as int;
                        j += 1;
                    }
                    i = j;
                    continue;
                }
            } else if (c as int) == PH_KEND {
                // nothing
            } else if (c as int) == PH_EVAL {
                let desc = self.x_eval_desc((o.a as int), (o.c as int));
                let k = self.xstr(desc.as_str());
                self.xemit(OP_CALL_EVAL, (o.b as int), k);
            } else if (c as int) == OP_POP {
                self.xop(OP_POP);
            } else {
                let at = self.xemit((c as int), (o.a as int), (o.b as int));
                if (c as int) == OP_TEMPLATE_OBJ || jumps_b((c as int)) {
                    // not fused: keep both operands
                    self.xo[at as usize] = Op { code: c, a: o.a, b: o.b, c: -1 };
                }
            }
            i += 1;
        }
        map[n] = self.xo.len() as int;
        let m = self.xo.len();
        let mut k: usize = 0;
        while k < m {
            if !self.xfixed[k] && is_jump(&self.xo[k]) {
                let a = self.xo[k].a;
                if a >= 0 && (a as usize) <= n {
                    self.xo[k].a = (map[a as usize] as i32);
                }
                if jumps_b((self.xo[k].code as int)) {
                    let b = self.xo[k].b;
                    if b >= 0 && (b as usize) <= n {
                        self.xo[k].b = (map[b as usize] as i32);
                    }
                }
            }
            k += 1;
        }
        self.protos[pi].code = self.xo.clone();
        self.xo = Vec::new();
        self.xfixed = Vec::new();
    }

    fn x_enter(&mut self, scope: int, is_fn: bool) {
        let si = scope as usize;
        if !is_fn && self.scopes[si].has_env {
            let es = self.scopes[si].env_size;
            self.xemit(OP_PUSH_ENV, es, 0);
        }
        let binds = self.scopes[si].binds.clone();
        for b in binds.iter() {
            if self.binds[*b as usize].tdz && !self.binds[*b as usize].global {
                self.xop(OP_TDZ);
                self.xstore_bind(*b, true, scope);
                self.xop(OP_POP);
            }
        }
    }

    fn x_hoist(&mut self, scope: int) {
        let hs = self.scopes[scope as usize].hoists.clone();
        for h in hs.iter() {
            let p = self.hoists[*h as usize].proto;
            let name = self.hoists[*h as usize].name.clone();
            self.xemit(OP_CLOSURE, p, 0);
            let b = self.lookup_here(scope, name.as_str());
            if b >= 0 {
                self.xstore_bind(b, true, scope);
            } else {
                let a = self.atom(name.as_str());
                self.xemit(OP_SET_GLOBAL, a, 0);
            }
            self.xop(OP_POP);
        }
    }

    fn x_prologue(&mut self, fscope: int, f: int) {
        let pb = self.fns[f as usize].param_binds.clone();
        let mut j: int = 0;
        for b in pb.iter() {
            if *b >= 0 && self.binds[*b as usize].in_env {
                self.xemit(OP_GET_LOCAL, j, 0);
                self.xstore_bind(*b, true, fscope);
                self.xop(OP_POP);
            }
            j += 1;
        }
        let binds = self.scopes[fscope as usize].binds.clone();
        for b in binds.iter() {
            let k = self.binds[*b as usize].kind;
            if k == K_ARGS {
                self.protos[self.xpi].uses_args = true;
                self.xop(OP_ARGUMENTS);
                self.xstore_bind(*b, true, fscope);
                self.xop(OP_POP);
                if self.fns[f as usize].mapped {
                    // parameters nothing assigns (and no direct eval could)
                    // hold what the arguments object holds until it is
                    // written itself: the mapping is made then (b's 1 << 30)
                    let mut lazy = self.eval_sites.is_empty();
                    for q in pb.iter() {
                        if *q >= 0 && (*q as usize) < self.bind_stored.len() && self.bind_stored[*q as usize] {
                            lazy = false;
                        }
                    }
                    let lazy_bit: int = if lazy { 1 << 30 } else { 0 };
                    // the last parameter of a name is the mapped one
                    let mut j = (pb.len() as int) - 1;
                    while j >= 0 {
                        let pbj = pb[j as usize];
                        let mut later = false;
                        let mut q = j + 1;
                        while q < (pb.len() as int) {
                            if pb[q as usize] == pbj {
                                later = true;
                            }
                            q += 1;
                        }
                        if pbj >= 0 && !later && self.binds[pbj as usize].in_env {
                            let depth = self.env_depth(fscope, self.binds[pbj as usize].scope);
                            let slot = self.binds[pbj as usize].slot;
                            self.xemit(OP_ARGUMENTS, 2 + j, slot * 256 + depth + lazy_bit);
                            self.xop(OP_POP);
                        }
                        j -= 1;
                    }
                }
            } else if k == K_CALLEE {
                self.xop(OP_CALLEE);
                self.xstore_bind(*b, true, fscope);
                self.xop(OP_POP);
            }
        }
        let ev = self.scopes[fscope as usize].with_bind;
        if ev >= 0 && self.binds[ev as usize].name.as_str() == "%ev" {
            // a direct eval's vars: an object without a prototype
            self.xop(OP_NEW_OBJECT);
            self.xop(OP_NULL);
            let a = self.atom("__proto__");
            self.xemit(OP_INIT_PROP, a, 1);
            self.xstore_bind(ev, true, fscope);
            self.xop(OP_POP);
        }
    }

    /// Sloppy direct eval code's new vars: on the caller's `%ev` object
    /// (undefined unless there).
    fn x_ev_decls(&mut self, scope: int) {
        let ev = self.eval_ev;
        let binds = self.scopes[scope as usize].binds.clone();
        for b in binds.iter() {
            let k = self.binds[*b as usize].kind;
            if (k == K_VAR || k == K_FUNC) && self.binds[*b as usize].global {
                let nm = self.binds[*b as usize].name.clone();
                let a = self.atom(nm.as_str());
                self.xpush_str(nm.as_str());
                self.xload_bind(ev, scope);
                self.xop(OP_IN);
                let jt = self.xemit(OP_JT, 0, 0);
                self.xfixed[jt as usize] = true;
                self.xload_bind(ev, scope);
                self.xop(OP_UNDEF);
                self.xemit(OP_SET_PROP, a, 0);
                self.xop(OP_POP);
                self.xpatch(jt);
            }
        }
    }

    fn x_body_vars(&mut self, bscope: int, fscope: int) {
        let binds = self.scopes[bscope as usize].binds.clone();
        for b in binds.iter() {
            if self.binds[*b as usize].kind != K_VAR {
                continue;
            }
            let nm = self.binds[*b as usize].name.clone();
            let pb = match self.scopes[fscope as usize].names.get(&nm) {
                Some(x) => *x,
                None => -1,
            };
            if pb >= 0 && self.binds[pb as usize].kind == K_PARAM {
                self.xload_bind(pb, bscope);
                self.xstore_bind(*b, true, bscope);
                self.xop(OP_POP);
            }
        }
    }

    fn x_annex(&mut self, k: int) {
        let vb = self.annexes[k as usize].var_bind;
        if vb < 0 {
            return;
        }
        let scope = self.annexes[k as usize].scope;
        let name = self.annexes[k as usize].name.clone();
        let here = match self.scopes[scope as usize].names.get(&name) {
            Some(b) => *b,
            None => -1,
        };
        if here >= 0 && here != vb && self.binds[here as usize].kind == K_FUNC {
            self.xload_bind(here, scope);
        } else {
            let p = self.annexes[k as usize].proto;
            self.xemit(OP_CLOSURE, p, 0);
        }
        self.xstore_bind(vb, true, scope);
        self.xop(OP_POP);
    }
}

impl PComp {
    // =====================================================================
    // functions being written

    fn push_fn(&mut self, proto: Proto, is_program: bool, base: int) -> int {
        self.fns.push(PFn {
            fscope: -1,
            scopes: Vec::new(),
            param_binds: Vec::new(),
            base: base,
            is_program: is_program,
            proto: -1,
            str_consts: HashMap::new(),
            num_consts: HashMap::new(),
            mapped: false,
        });
        let fnid = (self.fns.len() as int) - 1;
        self.fs.push(PState {
            proto: proto,
            fnid: fnid,
            ctl: Vec::new(),
            chain: Vec::new(),
            pending_labels: Vec::new(),
            is_program: is_program,
            var_scope: -1,
            annex: Vec::new(),
            saved_in_function: self.in_function,
            saved_in_generator: self.in_generator,
            saved_in_async: self.in_async,
            saved_strict: self.strict,
            saved_scope: self.cur_scope,
            saved_ctx: self.ctx,
        });
        fnid
    }

    /// Ends the function on top: its proto, as CLOSURE names it.
    fn pop_fn(&mut self) -> int {
        let st = self.fs.pop().unwrap();
        self.cur_scope = st.saved_scope;
        self.in_function = st.saved_in_function;
        self.in_generator = st.saved_in_generator;
        self.in_async = st.saved_in_async;
        self.strict = st.saved_strict;
        self.ctx = st.saved_ctx;
        let fnid = st.fnid;
        self.protos.push(st.proto);
        let idx = (self.protos.len() as int) - 1;
        self.fns[fnid as usize].proto = idx;
        self.proto_base + idx
    }

    /// Annex B: a function declared in a block of sloppy code also sets
    /// the function's var of that name, unless a lexical or parameter
    /// binding there has the name.
    fn end_annex(&mut self) {
        if self.f().proto.strict {
            return;
        }
        let list = self.f().annex.clone();
        let vs = self.f().var_scope;
        for k in list.iter() {
            let nm = self.annexes[*k as usize].name.clone();
            let existing = match self.scopes[vs as usize].names.get(&nm) {
                Some(b) => *b,
                None => -1,
            };
            if existing >= 0 {
                let ek = self.binds[existing as usize].kind;
                if ek != K_VAR && ek != K_FUNC {
                    continue;
                }
            }
            // a parameter's name (parameters with expressions have a
            // scope of their own), or a lexical binding of a block between
            // the function's block and the var scope: `var f` there would
            // be an early error, so no copy
            let fnid = self.f().fnid;
            let fs = self.fns[fnid as usize].fscope;
            if fs >= 0 && fs != vs {
                let pb = match self.scopes[fs as usize].names.get(&nm) {
                    Some(b) => *b,
                    None => -1,
                };
                if pb >= 0 && self.binds[pb as usize].kind == K_PARAM {
                    continue;
                }
            }
            let mut blocked = false;
            let ascope = self.annexes[*k as usize].scope;
            let mut sc = self.scopes[ascope as usize].parent;
            while sc >= 0 && sc != vs && sc != fs {
                let lb = match self.scopes[sc as usize].names.get(&nm) {
                    Some(b) => *b,
                    None => -1,
                };
                if lb >= 0 {
                    let lk = self.binds[lb as usize].kind;
                    if lk == K_LET || lk == K_CONST || lk == K_CLASS || lk == K_FUNC || (lk == K_CATCH && self.catch_patterns.contains(&sc)) {
                        blocked = true;
                    }
                }
                sc = self.scopes[sc as usize].parent;
            }
            if blocked {
                continue;
            }
            let b = self.declare(vs, nm.as_str(), K_VAR);
            self.annexes[*k as usize].var_bind = b;
        }
    }

    /// The function's source text: tokens `first` to the last read.
    fn set_span(&mut self, p: int, first: int) {
        if first < 0 || self.pos < 1 || first >= self.pos {
            return;
        }
        let st = self.toks[first as usize].start as int;
        let en = self.toks[(self.pos - 1) as usize].end as int;
        if st < 0 || en < st {
            return;
        }
        let pi = (p - self.proto_base) as usize;
        let pr = &mut self.protos[pi];
        pr.src = Val::Str(pr.cstrs.len() as int);
        pr.cstrs.push(self.src_rc.clone());
        pr.src_start = st;
        pr.src_end = en;
    }

    // ---- tokens of other text (template substitutions, made-up code)

    /// Reads `src` as tokens after the script's own; `pop_tokens` drops
    /// them. Answers what pop_tokens needs back.
    fn push_tokens(&mut self, src: &str) -> Vec<int> {
        let mut lx = lexer_for(src, self.jsx, self.jsx_factory.as_str(), self.jsx_fragment.as_str());
        let base = self.toks.len() as int;
        let xbase = self.xs.len() as int;
        let mut saved: Vec<int> = Vec::new();
        saved.push(base);
        saved.push(xbase);
        saved.push(self.pos);
        self.lex_tokens(&mut lx, xbase);
        if !lx.error.is_empty() {
            let e = lx.error.clone();
            self.fail(e.as_str());
        }
        for x in lx.xs {
            self.xs.push(x);
        }
        if self.error.is_empty() {
            self.pos = base;
        }
        saved
    }

    fn pop_tokens(&mut self, saved: &Vec<int>) {
        let base = saved[0] as usize;
        let xbase = saved[1] as usize;
        self.toks.truncate(base);
        self.mate.truncate(base);
        self.xs.truncate(xbase);
        if self.error.is_empty() {
            self.pos = saved[2];
        } else {
            self.pos = (base as int) - 1;
        }
    }

    // =====================================================================
    // the program

    /// `"use strict"` at the top of a body: whether there is one.
    fn directives(&mut self) -> bool {
        let mut found = false;
        let mut k = self.pos;
        let n = self.tok_count();
        while k < n && (self.toks[k as usize].kind as int) == T_STR {
            if self.text_is(k, "use strict") && !self.tok_escaped(k) {
                found = true;
                self.strict = true;
            }
            k += 1;
            if k < n && (self.toks[k as usize].kind as int) == T_PUNCT && self.text_is(k, ";") {
                k += 1;
            } else if !(k < n && self.tok_nl(k)) {
                break;
            }
        }
        found
    }

    /// Compiles the script; answers its proto, -1 when it is not one.
    pub fn compile_program(&mut self) -> int {
        if !self.error.is_empty() {
            return -1;
        }
        let mut strict = self.directives();
        if self.force_strict {
            strict = true;
            self.strict = true;
        }
        if strict && self.local_if_strict {
            self.local_program = true;
        }
        let mut proto = Proto::new("<script>");
        proto.strict = strict;
        let outer = if self.outer_desc.is_empty() { -1 } else { self.outer_scopes(strict) };
        // direct eval code takes its caller's context (eval_ctx); indirect
        // eval code is global code, where super is an early error
        self.ctx = if !self.outer_desc.is_empty() { self.eval_ctx & (CX_SUPER_CALL | CX_SUPER_PROP | CX_NO_ARGS) } else { 0 };
        let fnid = self.push_fn(proto, true, 1);
        let s = self.new_scope(outer, true, false);
        self.scopes[s as usize].is_program = true;
        self.fns[fnid as usize].fscope = s;
        self.f().var_scope = s;
        self.cur_scope = s;
        self.eval_prog = s;
        if self.local_program || self.local_if_strict {
            self.emit(PH_ENTER, s, 1);
        }
        self.emit(PH_DECL_GLOBALS, s, 0);
        self.emit(PH_HOIST, s, 0);
        self.op(OP_UNDEF);
        self.emit(OP_SET_LOCAL, 0, 0);
        self.op(OP_POP);
        while self.kind() != T_EOF && self.error.is_empty() {
            self.list_stmt = true;
            self.statement();
        }
        self.emit(OP_GET_LOCAL, 0, 0);
        self.op(OP_RETURN);
        self.end_annex();
        if !self.outer_desc.is_empty() && self.error.is_empty() {
            self.priv_check_seeded();
        }
        let entry = self.pop_fn();
        if !self.error.is_empty() {
            return -1;
        }
        // the parse is over: its tokens are not read again (finish works
        // on the code, the references and the scopes)
        self.toks = Vec::new();
        self.finish();
        if !self.error.is_empty() {
            return -1;
        }
        entry
    }

    /// Direct eval code: the caller's environments (`outer_desc`) as
    /// scopes outside the program's, their bindings placed; the innermost.
    fn outer_scopes(&mut self, strict: bool) -> int {
        let desc = self.outer_desc.clone();
        let parts = split_on(desc.as_str(), 59);
        let head = if parts.is_empty() { String::new() } else { parts[0].clone() };
        let in_fn = head.starts_with("F");
        self.eval_in_params = head.contains("P");
        self.eval_new_target = head.contains("N");
        self.eval_ctx = digits_int(head.as_str());
        let mut ids: Vec<int> = Vec::new();
        let mut fn_level: Vec<bool> = Vec::new();
        let mut parent: int = -1;
        let mut i = parts.len();
        let mut privs: Vec<String> = Vec::new();
        while i > 1 {
            i -= 1;
            let lv = parts[i].clone();
            if lv.starts_with("p|") {
                privs.insert(0, lv);
                continue;
            }
            let s = self.new_scope(parent, false, false);
            self.scopes[s as usize].has_env = true;
            fn_level.insert(0, lv.starts_with("f"));
            let n = lv.as_bytes().len();
            let items = if n > 2 { String::from(&lv[2..n]) } else { String::new() };
            let mut size: int = 0;
            let entries = split_on(items.as_str(), 44);
            for it in entries.iter() {
                let f = split_on(it.as_str(), 58);
                if f.len() != 3 {
                    continue;
                }
                let kind = digits_int(f[1].as_str());
                let slot = digits_int(f[2].as_str());
                let lexical = kind == K_LET || kind == K_CONST || kind == K_CLASS;
                self.binds.push(PBind {
                    name: f[0].clone(),
                    kind: kind,
                    captured: true,
                    slot: slot,
                    in_env: true,
                    scope: s,
                    global: false,
                    placed: true,
                    tdz: lexical,
                    init_seq: if lexical { SEQ_NEVER } else { 0 },
                });
                let b = (self.binds.len() as int) - 1;
                self.scopes[s as usize].names.insert(f[0].clone(), b);
                self.scopes[s as usize].binds.push(b);
                if f[0].as_str() == "%ev" {
                    self.scopes[s as usize].with_bind = b;
                    self.has_with = true;
                    if in_fn && !strict && self.eval_ev < 0 {
                        self.eval_ev = b;
                    }
                }
                if slot + 1 > size {
                    size = slot + 1;
                }
            }
            self.scopes[s as usize].env_size = size;
            ids.insert(0, s);
            parent = s;
        }
        if !privs.is_empty() {
            self.priv_seed(&privs);
        }
        if in_fn && !strict {
            let mut j: usize = 0;
            while j < ids.len() {
                self.eval_var_scopes.push(ids[j]);
                if fn_level[j] {
                    break;
                }
                j += 1;
            }
        }
        parent
    }

    // =====================================================================
    // statements

    /// Whether an enclosing statement already has label `w`.
    fn label_taken(&mut self, w: &str) -> bool {
        let pl = self.f().pending_labels.clone();
        for l in pl.iter() {
            if l.as_str() == w {
                return true;
            }
        }
        let n = self.f().ctl.len();
        let mut i: usize = 0;
        while i < n {
            let ls = self.f().ctl[i].labels.clone();
            for l in ls.iter() {
                if l.as_str() == w {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn take_labels(&mut self) -> Vec<String> {
        let l = self.f().pending_labels.clone();
        self.f().pending_labels.clear();
        l
    }

    fn push_loop(&mut self, labels: Vec<String>) {
        let mut c = new_ctl(CT_LOOP);
        c.labels = labels;
        self.f().ctl.push(c);
    }

    fn find_target(&self, label: &str, cont: bool) -> int {
        let f = &self.fs[self.fs.len() - 1];
        let mut i = (f.ctl.len() as int) - 1;
        while i >= 0 {
            let c = &f.ctl[i as usize];
            if c.kind == CT_INLINE {
                // a function body's break may not leave it
                return -1;
            }
            if label.is_empty() {
                if c.kind == CT_LOOP || (!cont && c.kind == CT_SWITCH) {
                    return i;
                }
            } else {
                let mut has = false;
                for l in c.labels.iter() {
                    if l.as_str() == label {
                        has = true;
                    }
                }
                if has {
                    if cont && c.kind != CT_LOOP {
                        return -1;
                    }
                    return i;
                }
            }
            i -= 1;
        }
        -1
    }

    fn split_ctl(&mut self, at: int) -> Vec<PCtl> {
        let mut rest: Vec<PCtl> = Vec::new();
        while (self.f().ctl.len() as int) > at {
            let c = self.f().ctl.pop().unwrap();
            rest.push(c);
        }
        rest
    }

    fn join_ctl(&mut self, mut saved: Vec<PCtl>) {
        while !saved.is_empty() {
            let c = saved.pop().unwrap();
            self.f().ctl.push(c);
        }
    }

    /// The block statement at token `at`, read again here.
    fn block_at(&mut self, at: int) {
        let back = self.pos;
        self.goto(at);
        self.block_statement();
        self.goto(back);
    }

    /// Leaves every control entry above `target` (-1: all, for `return`).
    fn unwind_to(&mut self, target: int) {
        let top = (self.f().ctl.len() as int) - 1;
        let mut i = top;
        let ret = target < 0;
        let mut above: int = 1;
        while i > target {
            let kind = self.f().ctl[i as usize].kind;
            if kind == CT_ITER {
                self.op(OP_END_TRY);
                if ret {
                    self.emit(OP_ITER_CLOSE_AT, above, 0);
                    above += 1;
                } else {
                    self.emit(OP_ITER_CLOSE, 0, 0);
                }
                i -= 1;
                continue;
            }
            if kind == CT_ITEM && ret {
                above += 1;
            }
            if kind == CT_TRY {
                if self.f().ctl[i as usize].installed {
                    self.op(OP_END_TRY);
                }
                let fin = self.f().ctl[i as usize].fin;
                if fin >= 0 {
                    let saved = self.split_ctl(i);
                    self.block_at(fin);
                    self.join_ctl(saved);
                } else if fin == -2 {
                    let un = self.f().ctl[i as usize].uname.clone();
                    let ua = self.f().ctl[i as usize].uasync;
                    let saved = self.split_ctl(i);
                    self.emit_dispose(un.as_str(), "", ua);
                    self.join_ctl(saved);
                }
            } else if kind == CT_ENV && !ret {
                let s = self.f().ctl[i as usize].scope;
                self.emit(PH_LEAVE, s, 0);
            } else if kind == CT_ITEM && !ret {
                self.op(OP_POP);
            }
            i -= 1;
        }
    }

    fn is_program_top(&self) -> bool {
        if !(self.fs.len() == 1 && self.fs[0].is_program && self.keep_completion) {
            return false;
        }
        for c in self.fs[0].ctl.iter() {
            if c.kind != CT_ENV {
                return false;
            }
        }
        true
    }

    fn is_let_decl(&self) -> bool {
        self.is("let") && (self.peek_kind(1) == T_IDENT || self.peek_is(1, "[") || self.peek_is(1, "{"))
    }

    fn statement(&mut self) {
        let in_list = self.list_stmt;
        self.list_stmt = false;
        let using_ok = self.using_here;
        self.using_here = false;
        let sub = self.sub_label;
        self.sub_label = false;
        if !self.error.is_empty() {
            return;
        }
        let word = self.kind() == T_IDENT && !self.escaped();
        let label = word && self.peek_is(1, ":") && !is_reserved(self.text().as_str());
        if self.kind() == T_IDENT && self.peek_is(1, ":") && (self.bad_ident(self.text().as_str()) || (self.escaped() && is_reserved(self.text().as_str()))) {
            let t = self.text();
            self.fail(format!("unexpected reserved word '{}'", t).as_str());
            return;
        }
        let looping = word && (self.is("for") || self.is("while") || self.is("do"));
        if !label && !looping {
            self.f().pending_labels.clear();
        }
        if self.kind() == T_PUNCT {
            if self.is("{") {
                self.block_statement();
                return;
            }
            if self.is(";") {
                self.next();
                return;
            }
        }
        if word && self.ts && self.ts_statement(in_list) {
            return;
        }
        if word {
            let w = self.text();
            let ws = w.as_str();
            if ws == "using" || ws == "await" {
                let u = self.using_kind();
                if u > 0 {
                    if !in_list || !using_ok {
                        self.fail("using declarations are not allowed here");
                        return;
                    }
                    self.using_decl(u == 2);
                    self.semicolon();
                    return;
                }
            }
            if ws == "var" || ws == "const" || (ws == "let" && self.is_let_decl() && (in_list || !self.tok_nl(self.pos + 1) || !(self.peek_kind(1) == T_IDENT))) {
                self.var_decl();
                self.semicolon();
                return;
            }
            if ws == "function" {
                self.function_decl(in_list, 0);
                return;
            }
            if ws == "async" && self.peek_is(1, "function") && !self.tok_nl(self.pos + 1) {
                self.next();
                self.function_decl(in_list, F_ASYNC);
                return;
            }
            if ws == "class" {
                self.class_decl();
                return;
            }
            if ws == "if" {
                self.if_statement();
                return;
            }
            if ws == "for" {
                self.for_statement();
                return;
            }
            if ws == "while" {
                self.while_statement();
                return;
            }
            if ws == "do" {
                self.do_statement();
                return;
            }
            if ws == "return" && self.inline_ctl() >= 0 {
                self.inline_return();
                return;
            }
            if ws == "return" {
                if !self.in_function || (self.module_code && self.fs.len() == self.module_depth) || self.static_depth == self.fs.len() as int {
                    self.fail("return outside a function");
                    return;
                }
                self.next();
                let mut bare = false;
                if !self.is(";") && !self.is("}") && self.kind() != T_EOF && !self.nl_before() {
                    self.f().proto.void_ret = false;
                    self.expression();
                } else {
                    self.op(OP_UNDEF);
                    bare = true;
                }
                self.semicolon();
                self.unwind_to(-1);
                if bare {
                    // checked once the body is left: a finally may call super()
                    self.ctor_this_check();
                }
                self.op(OP_RETURN);
                return;
            }
            if ws == "break" || ws == "continue" {
                let cont = ws == "continue";
                self.next();
                let mut lab = String::new();
                if self.kind() == T_IDENT && !self.nl_before() && !is_reserved(self.text().as_str()) {
                    lab = self.text();
                    self.next();
                }
                self.semicolon();
                let target = self.find_target(lab.as_str(), cont);
                if target < 0 {
                    self.fail("SyntaxError: illegal break or continue");
                    return;
                }
                self.unwind_to(target);
                let j = self.emit(OP_JUMP, 0, 0);
                if cont {
                    self.f().ctl[target as usize].conts.push(j);
                } else {
                    self.f().ctl[target as usize].breaks.push(j);
                }
                return;
            }
            if ws == "throw" {
                self.next();
                if self.nl_before() {
                    self.fail("line break after throw");
                    return;
                }
                self.expression();
                self.semicolon();
                self.op(OP_THROW);
                return;
            }
            if ws == "try" {
                self.try_statement();
                return;
            }
            if ws == "switch" {
                self.switch_statement();
                return;
            }
            if ws == "debugger" {
                self.next();
                self.semicolon();
                return;
            }
            if ws == "with" {
                self.with_statement();
                return;
            }
            if ws == "import" && !self.peek_is(1, "(") && !self.peek_is(1, ".") {
                if !self.module_path.is_empty() || self.script_imports {
                    self.script_import();
                    return;
                }
                self.fail("modules are not supported");
                return;
            }
            if ws == "export" {
                if !self.module_path.is_empty() {
                    self.module_export(in_list);
                    return;
                }
                if self.script_imports {
                    self.script_export(in_list);
                    return;
                }
                self.fail("modules are not supported");
                return;
            }
            if label {
                if self.label_taken(w.as_str()) {
                    self.fail(format!("Label '{}' has already been declared", w).as_str());
                    return;
                }
                self.next();
                self.next();
                if (self.is("function") && (self.strict || sub || self.peek_is(1, "*"))) || (self.is("async") && self.peek_is(1, "function") && !self.tok_nl(self.pos + 1)) {
                    self.fail("A labelled function must be a plain function in sloppy mode code");
                    return;
                }
                if self.kind() == T_IDENT && !self.escaped() && self.lexical_here() {
                    self.fail("Lexical declaration cannot appear in a single-statement context");
                    return;
                }
                self.sub_label = sub;
                let body_loop = self.kind() == T_IDENT
                    && !self.escaped()
                    && (self.is("for") || self.is("while") || self.is("do") || (self.peek_is(1, ":") && !is_reserved(self.text().as_str())));
                if body_loop {
                    self.f().pending_labels.push(w.clone());
                    self.statement();
                    return;
                }
                let mut c = new_ctl(CT_BLOCK);
                c.labels.push(w.clone());
                self.f().ctl.push(c);
                self.statement();
                let ctl = self.f().ctl.pop().unwrap();
                for x in ctl.breaks.iter() {
                    self.patch(*x);
                }
                return;
            }
        }
        let e = self.expression();
        self.semicolon();
        self.expr_statement_tail(e);
    }

    /// The body of an if, a loop: no lexical declaration there.
    /// `if_body`: an `if`'s branch, where sloppy code may declare a plain
    /// function (Annex B)
    fn sub_statement(&mut self, if_body: bool) {
        if self.kind() == T_IDENT && !self.escaped() {
            let w = self.text();
            let func = (w.as_str() == "function" && (!if_body || self.strict || self.peek_is(1, "*")))
                || (w.as_str() == "async" && self.peek_is(1, "function") && !self.tok_nl(self.pos + 1));
            if func {
                self.fail("In strict mode code or a single-statement context, functions can only be declared at top level or inside a block");
                return;
            }
            let lexical = self.lexical_here();
            if lexical {
                self.fail("Lexical declaration cannot appear in a single-statement context");
                return;
            }
            if if_body && w.as_str() == "function" {
                // Annex B: `if (x) function f() {}` as if in a block
                let saved = self.cur_scope;
                let sc = self.new_scope(saved, false, false);
                self.enter_block(sc);
                self.emit(PH_HOIST, sc, 0);
                self.statement();
                self.leave_block(sc, saved);
                return;
            }
        }
        self.sub_label = true;
        self.statement();
        self.sub_label = false;
    }

    /// At a word: a lexical declaration starts here (const, class, `let`
    /// before a name or a pattern)
    fn lexical_here(&self) -> bool {
        let w = self.text();
        w.as_str() == "const"
            || w.as_str() == "class"
            || (w.as_str() == "let" && (self.peek_is(1, "[") || (self.peek_is(1, "{") && !self.tok_nl(self.pos + 1)) || (self.peek_kind(1) == T_IDENT && !self.tok_nl(self.pos + 1))))
    }

    /// An expression statement's end: its value dropped, or kept as the
    /// script's completion; `i++` and `x += v` on a local in place.
    fn expr_statement_tail(&mut self, e: Ex) {
        let top = self.is_program_top();
        if e.k == E_UPDATE_IDENT && e.a == self.pc() - 1 {
            let i = self.fs.len() - 1;
            let mut c = self.fs[i].proto.code[e.a as usize].c;
            c |= (U_DISCARD as i32);
            if top {
                c |= (U_TOP as i32);
            }
            self.fs[i].proto.code[e.a as usize].c = c;
            return;
        }
        if !top && e.k == E_ADDASSIGN && e.b == self.pc() - 1 && e.b > 0 {
            let i = self.fs.len() - 1;
            let add = self.fs[i].proto.code[(e.b - 1) as usize];
            if (add.code as int) == OP_ADD {
                let sr = self.fs[i].proto.code[e.b as usize].a;
                let lr = self.fs[i].proto.code[e.a as usize].a;
                let pure = if self.assigns == e.c { 1 } else { 0 };
                self.fs[i].proto.code[e.a as usize] = Op { code: (PH_REF as i32), a: lr, b: (M_ADDT_LOAD as i32), c: sr * 2 + pure };
                self.fs[i].proto.code[(e.b - 1) as usize] = Op { code: (PH_REF as i32), a: sr, b: (M_ADDT_STORE as i32), c: pure };
                self.fs[i].proto.code[e.b as usize] = Op { code: (PH_NOP as i32), a: 0, b: 0, c: -1 };
                return;
            }
        }
        if !top && e.k == E_MEMBER_ADDASSIGN {
            let last = self.last_op();
            let n = self.pc();
            if (last.code as int) == OP_SET_PROP && n >= 2 {
                let i = self.fs.len() - 1;
                if (self.fs[i].proto.code[(n - 2) as usize].code as int) == OP_ADD {
                    self.pop_op();
                    self.set_op(n - 2, OP_ADD_PROP_POP, (last.a as int), 0);
                    return;
                }
            }
        }
        if top {
            self.emit(OP_SET_LOCAL, 0, 0);
        }
        self.op(OP_POP);
    }

    fn block_statement(&mut self) {
        let saved = self.cur_scope;
        let s = self.new_scope(saved, false, false);
        self.enter_block(s);
        self.emit(PH_HOIST, s, 0);
        let end = self.mate_of(self.pos);
        self.expect("{");
        let (mark, saved_end) = self.using_open(end);
        while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
            self.list_stmt = true;
            self.using_here = true;
            self.statement();
        }
        self.using_close(mark, saved_end);
        self.expect("}");
        self.leave_block(s, saved);
    }

    /// A statement list that may hold `using` declarations starts (`end`
    /// its closing token): the mark and the end to give `using_close`.
    fn using_open(&mut self, end: int) -> (int, int) {
        let mark = self.using_mark;
        self.using_mark = self.usings.len() as int;
        let saved_end = self.using_end;
        self.using_end = end;
        ((mark), saved_end)
    }

    /// The list's end: its `using` region, if one started, ends here.
    fn using_close(&mut self, mark: int, saved_end: int) {
        while (self.usings.len() as int) > self.using_mark {
            let n = self.usings.len() - 1;
            let list = self.usings[n].list.clone();
            let err = self.usings[n].err.clone();
            let at = self.usings[n].at;
            let is_async = self.usings[n].is_async;
            self.usings.remove(n);
            self.f().ctl.pop();
            self.op(OP_END_TRY);
            self.emit_dispose(list.as_str(), "", is_async);
            let j = self.emit(OP_JUMP, 0, 0);
            self.patch(at);
            self.emit_ref(err.as_str(), M_INIT, true);
            self.op(OP_POP);
            self.emit_dispose(list.as_str(), err.as_str(), is_async);
            self.emit_ref(err.as_str(), M_LOAD, false);
            self.op(OP_THROW);
            self.patch(j);
        }
        self.using_mark = mark;
        self.using_end = saved_end;
    }

    /// `import(specifier[, options])`: __cerxesESM.dynImport(specifier,
    /// __cerxesRef, options), the promise of the module's namespace
    /// (`__cerxesRef`: the importing module, or the script's file).
    fn dyn_import(&mut self) -> Ex {
        let k = self.pos;
        if k > 0 && self.tok_is(k - 1, "new") {
            self.fail("Cannot use new with import");
            return ex(E_OTHER);
        }
        if self.peek_is(1, ".") {
            self.fail("import.meta may only appear in module code");
            return ex(E_OTHER);
        }
        self.next();
        if !self.is("(") {
            self.fail("Unexpected token import");
            return ex(E_OTHER);
        }
        self.next();
        if self.is(")") || self.is("...") {
            self.fail("import() requires a specifier");
            return ex(E_OTHER);
        }
        let g = self.atom("__cerxesESM");
        self.emit(OP_GET_GLOBAL, g, 0);
        let a = self.atom("dynImport");
        self.emit(OP_GET_METHOD, a, 0);
        let saved = self.no_in;
        self.no_in = false;
        self.assign();
        self.emit_ref("__cerxesRef", M_LOAD, false);
        if self.eat(",") && !self.is(")") {
            if self.is("...") {
                self.fail("Unexpected token ...");
            }
            self.assign();
            self.eat(",");
        } else {
            self.op(OP_UNDEF);
        }
        self.no_in = saved;
        self.expect(")");
        self.emit(OP_CALL, 3, 0);
        ex(E_OTHER)
    }

    /// [__cerxesUsing[name], __cerxesUsing]
    fn push_using_api(&mut self, name: &str) {
        let g = self.atom("__cerxesUsing");
        self.emit(OP_GET_GLOBAL, g, 0);
        let a = self.atom(name);
        self.emit(OP_GET_METHOD, a, 0);
    }

    /// DisposeResources of the list in binding `list`, after the error in
    /// binding `err` ("" for none); awaited for `await using`.
    fn emit_dispose(&mut self, list: &str, err: &str, is_async: bool) {
        self.push_using_api(if is_async { "disposeAsync" } else { "dispose" });
        self.emit_ref(list, M_LOAD, false);
        if err.is_empty() {
            self.op(OP_FALSE);
            self.op(OP_UNDEF);
        } else {
            self.op(OP_TRUE);
            self.emit_ref(err, M_LOAD, false);
        }
        self.emit(OP_CALL, 3, 0);
        if is_async {
            self.emit(OP_YIELD, 2, 0);
            self.resume_point();
        }
        self.op(OP_POP);
    }

    /// At `using` / `await`: 1 for a `using` declaration, 2 for `await
    /// using`, 0 for neither.
    fn using_kind(&self) -> int {
        if self.escaped() {
            return 0;
        }
        let k = self.pos;
        if self.is("using") && self.peek_kind(1) == T_IDENT && !self.tok_nl(k + 1) && !self.peek_is(1, "in") && !self.peek_is(1, "instanceof") && !self.peek_is(1, "of") {
            return 1;
        }
        let k2 = k + 2;
        if self.is("await") && (self.in_async || !self.module_path.is_empty()) && self.peek_is(1, "using") && !self.tok_nl(k + 1) && self.peek_kind(2) == T_IDENT && !self.tok_nl(k2) && !self.peek_is(2, "in") && !self.peek_is(2, "instanceof") {
            return 2;
        }
        0
    }

    /// `using x = v, …` / `await using x = v, …`, no semicolon.
    fn using_decl(&mut self, is_async: bool) {
        if is_async {
            self.next();
        }
        self.next();
        self.using_begin(is_async);
        let n = self.usings.len() - 1;
        let list = self.usings[n].list.clone();
        loop {
            if self.kind() != T_IDENT || self.is("let") {
                self.fail("using declarations bind identifiers");
                return;
            }
            let ts = self.pos;
            let name = self.text();
            self.next();
            self.skip_annotation();
            if !self.is("=") {
                self.fail("Missing initializer in using declaration");
                return;
            }
            self.next();
            self.push_using_api("add");
            self.emit_ref(list.as_str(), M_LOAD, false);
            let e = self.assign();
            self.name_anonymous(e, name.as_str());
            self.op(if is_async { OP_TRUE } else { OP_FALSE });
            self.emit(OP_CALL, 3, 0);
            let after = self.pos;
            self.goto(ts);
            self.pattern_target(K_CONST);
            self.goto(after);
            self.mark_pending();
            if !self.eat(",") {
                break;
            }
        }
    }

    /// The `using` region of the list being read, started here unless it
    /// has: its resource list made, a try around the rest of the list.
    fn using_begin(&mut self, is_async: bool) {
        if (self.usings.len() as int) > self.using_mark {
            return;
        }
        // disposing awaits when the list holds an `await using`
        let mut awaits = is_async;
        let mut k = self.pos;
        while !awaits && k < self.using_end {
            if self.tok_is(k, "{") || self.tok_is(k, "(") || self.tok_is(k, "[") {
                let m = self.mate_of(k);
                if m <= k {
                    break;
                }
                k = m + 1;
                continue;
            }
            if self.tok_is(k, "await") && self.tok_is(k + 1, "using") && (self.toks[(k + 2) as usize].kind as int) == T_IDENT {
                awaits = true;
            }
            k += 1;
        }
        let n = self.using_count;
        self.using_count += 1;
        let list = format!("%ul{}", n);
        let err = format!("%ue{}", n);
        let s = self.cur_scope;
        self.declare(s, list.as_str(), K_CATCH);
        self.declare(s, err.as_str(), K_CATCH);
        self.emit(OP_NEW_ARRAY, 0, 0);
        self.emit_ref(list.as_str(), M_INIT, true);
        self.op(OP_POP);
        let at = self.emit(OP_TRY, 0, 0);
        let mut c = new_ctl(CT_TRY);
        c.fin = -2;
        c.installed = true;
        c.uname = list.clone();
        c.uasync = awaits;
        self.f().ctl.push(c);
        self.usings.push(URegion { list: list, err: err, at: at, is_async: awaits });
    }

    /// `var` / `let` / `const` and its declarators, no semicolon.
    fn var_decl(&mut self) {
        let kw = self.text();
        self.next();
        let kind = if kw.as_str() == "var" {
            K_VAR
        } else if kw.as_str() == "const" {
            K_CONST
        } else {
            K_LET
        };
        loop {
            let ts = self.pos;
            let single = self.kind() == T_IDENT;
            self.skip_binding_target();
            self.skip_annotation();
            if self.is("=") {
                let name = if single { self.tok_text(ts) } else { String::new() };
                self.next();
                let e = self.assign();
                if !name.is_empty() {
                    self.name_anonymous(e, name.as_str());
                }
                let after = self.pos;
                self.goto(ts);
                self.pattern_target(kind);
                self.goto(after);
            } else if kind == K_CONST {
                self.fail("Missing initializer in const declaration");
                return;
            } else if kind != K_VAR {
                self.op(OP_UNDEF);
                let after = self.pos;
                self.goto(ts);
                self.pattern_target(kind);
                self.goto(after);
            } else if single {
                let nm = self.tok_text(ts);
                if is_reserved(nm.as_str()) || (self.strict && is_strict_reserved(nm.as_str())) || self.bad_ident(nm.as_str()) {
                    self.fail(format!("unexpected reserved word '{}'", nm).as_str());
                    return;
                }
                if !self.check_var(nm.as_str()) {
                    return;
                }
                let vs = self.f().var_scope;
                self.declare(vs, nm.as_str(), K_VAR);
            } else {
                self.fail("Missing initializer in destructuring declaration");
                return;
            }
            self.mark_pending();
            if !self.eat(",") {
                break;
            }
        }
    }

    fn skip_binding_target(&mut self) {
        if self.is("[") || self.is("{") {
            let a = self.after_mate(self.pos);
            if a < 0 {
                self.fail("unbalanced brackets");
                return;
            }
            self.goto(a);
            return;
        }
        if self.kind() == T_IDENT {
            self.next();
            return;
        }
        let t = self.text();
        self.fail(format!("expected an identifier but found '{}'", t).as_str());
    }

    fn if_statement(&mut self) {
        self.next();
        self.expect("(");
        let e = self.expression();
        self.expect(")");
        self.cond_jump_false(e);
        let jf = self.pc() - 1;
        self.sub_statement(true);
        if self.is("else") {
            self.next();
            let j = self.emit(OP_JUMP, 0, 0);
            self.patch(jf);
            self.sub_statement(true);
            self.patch(j);
        } else {
            self.patch(jf);
        }
    }

    fn while_statement(&mut self) {
        self.next();
        let labels = self.take_labels();
        self.expect("(");
        let top = self.pc();
        let e = self.expression();
        self.expect(")");
        self.cond_jump_false(e);
        let jf = self.pc() - 1;
        self.push_loop(labels);
        self.sub_statement(false);
        let ctl = self.f().ctl.pop().unwrap();
        for x in ctl.conts.iter() {
            self.patch_to(*x, top);
        }
        self.emit(OP_JUMP, top, 0);
        self.patch(jf);
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
    }

    fn do_statement(&mut self) {
        self.next();
        let labels = self.take_labels();
        let top = self.pc();
        self.push_loop(labels);
        self.sub_statement(false);
        let ctl = self.f().ctl.pop().unwrap();
        for x in ctl.conts.iter() {
            self.patch(*x);
        }
        if !self.is("while") {
            self.fail("expected 'while'");
            return;
        }
        self.next();
        self.expect("(");
        self.expression();
        self.expect(")");
        self.eat(";");
        self.emit(OP_JT, top, 0);
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
    }

    /// Whether token `k` stands between a conditional's `?` and its `:`
    /// (an `in` there is the operator), from token `from` on.
    fn in_cond_middle(&self, from: int, k: int) -> bool {
        let mut open: int = 0;
        let mut i = from;
        while i < k {
            if self.tok_is(i, "(") || self.tok_is(i, "[") || self.tok_is(i, "{") {
                let a = self.after_mate(i);
                if a < 0 || a > k {
                    return false;
                }
                i = a;
                continue;
            }
            if self.tok_is(i, "?") {
                open += 1;
            } else if self.tok_is(i, ":") && open > 0 {
                open -= 1;
            }
            i += 1;
        }
        open > 0
    }

    fn for_statement(&mut self) {
        self.next();
        let mut is_await = false;
        if self.is("await") {
            if !self.in_async {
                self.fail("for await is only valid in async functions");
                return;
            }
            self.next();
            is_await = true;
        }
        if !self.is("(") {
            self.expect("(");
            return;
        }
        let open = self.pos;
        let close = self.mate_of(open);
        self.next();
        let head = self.pos;
        // a for-in / for-of: `in` or `of` before any `;` (not a declared
        // name `of`)
        let mut k = self.scan_to(head, ";", true);
        if self.tok_is(k, "of") && k == head + 1 && (self.tok_is(head, "var") || self.tok_is(head, "let") || self.tok_is(head, "const") || (self.tok_is(head, "async") && self.tok_is(k + 1, "=>"))) {
            k = self.scan_to(k + 1, ";", true);
        }
        if k < close && (self.tok_is(k, "in") || self.tok_is(k, "of")) && !self.in_cond_middle(head, k) {
            self.forin_statement(head, k, close, is_await);
            return;
        }
        if is_await {
            self.fail("for await needs 'of'");
            return;
        }
        let labels = self.take_labels();
        let saved = self.cur_scope;
        let s = self.new_scope(saved, false, false);
        self.enter_block(s);
        let (umark, uend) = self.using_open(-1);
        if !self.is(";") {
            let decl = self.is("var") || self.is("const") || self.is_let_decl();
            let u = self.using_kind();
            self.no_in = true;
            if u > 0 {
                self.using_decl(u == 2);
            } else if decl {
                self.var_decl();
            } else {
                let e = self.expression();
                self.no_in = false;
                self.expr_statement_tail(e);
            }
            self.no_in = false;
        }
        self.expect(";");
        let top = self.pc();
        let mut jf: int = -1;
        if !self.is(";") {
            let e = self.expression();
            self.cond_jump_false(e);
            jf = self.pc() - 1;
        }
        self.expect(";");
        let upd = self.pos;
        self.goto(close);
        self.expect(")");
        self.push_loop(labels);
        self.sub_statement(false);
        let ctl = self.f().ctl.pop().unwrap();
        for x in ctl.conts.iter() {
            self.patch(*x);
        }
        self.emit(PH_COPY_ENV, s, 0);
        if upd < close {
            let body_end = self.pos;
            self.goto(upd);
            let e = self.expression();
            if self.pos != close {
                self.expect(")");
            }
            self.expr_statement_tail(e);
            self.goto(body_end);
        }
        self.emit(OP_JUMP, top, 0);
        if jf >= 0 {
            self.patch(jf);
        }
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
        self.using_close(umark, uend);
        self.leave_block(s, saved);
    }

    /// `for (head in|of obj) body`: `kw` the `in` / `of` token, `close`
    /// the `)`.
    fn forin_statement(&mut self, head: int, kw: int, close: int, is_await: bool) {
        let of = self.tok_is(kw, "of");
        let labels = self.take_labels();
        // `for (using x of …)` / `for (await using x of …)`
        let using_sync = of && self.tok_is(head, "using") && kw == head + 2 && (self.toks[(head + 1) as usize].kind as int) == T_IDENT;
        let using_async = of && self.tok_is(head, "await") && (self.in_async || !self.module_path.is_empty()) && self.tok_is(head + 1, "using") && kw == head + 3 && (self.toks[(head + 2) as usize].kind as int) == T_IDENT;
        let using = if using_async { 2 } else if using_sync { 1 } else { 0 };
        let decl = using > 0 || self.tok_is(head, "var") || self.tok_is(head, "const") || (self.tok_is(head, "let") && head + 1 < kw);
        if of && !decl && !is_await && kw == head + 1 && self.tok_is(head, "async") && !self.tok_escaped(head) {
            self.fail("The left-hand side of a for-of loop may not be 'async'");
            return;
        }
        let mut kind = K_VAR;
        if using > 0 {
            kind = K_CONST;
            if self.tok_is(kw - 1, "let") {
                self.fail("let is disallowed as a lexically bound name");
                return;
            }
        } else if decl {
            if self.tok_is(head, "const") {
                kind = K_CONST;
            } else if self.tok_is(head, "let") {
                kind = K_LET;
            }
            // one binding, an initializer only for `for (var x = … in …)`
            let comma = self.scan_to(head + 1, ",", false);
            if comma < kw && self.tok_is(comma, ",") {
                self.fail("Invalid left-hand side in for-in or for-of loop: must have a single binding");
                return;
            }
            let eq = self.scan_to(head + 1, "=", false);
            if eq < kw && self.tok_is(eq, "=") {
                let ident = eq == head + 2 && (self.toks[(head + 1) as usize].kind as int) == T_IDENT;
                if of || kind != K_VAR || self.strict || !ident {
                    self.fail("for-in or for-of loop variable declaration may not have an initializer");
                    return;
                }
                // Annex B: the initializer is assigned first
                let nm = self.tok_text(head + 1);
                let vs = self.f().var_scope;
                self.declare(vs, nm.as_str(), K_VAR);
                self.goto(eq + 1);
                self.no_in = true;
                self.assign();
                self.no_in = false;
                self.emit_ref(nm.as_str(), M_STORE, false);
                self.op(OP_POP);
            }
        }
        self.goto(kw + 1);
        if of {
            self.assign();
        } else {
            self.expression();
        }
        if self.pos != close {
            self.expect(")");
            return;
        }
        self.next();
        let body = self.pos;
        if is_await {
            if !of {
                self.fail("for await needs 'of'");
                return;
            }
            self.for_await_rest(head, kw, body, decl, kind, labels);
            return;
        }
        let mut t: int = -1;
        if of {
            self.op(OP_ITER_VALUES);
            t = self.emit(OP_TRY, 0, 0);
            let mut c = new_ctl(CT_ITER);
            c.installed = true;
            self.f().ctl.push(c);
        } else {
            self.op(OP_ITER_KEYS);
            self.f().ctl.push(new_ctl(CT_ITEM));
        }
        let top = self.pc();
        let next = self.emit(OP_ITER_NEXT, 0, 0);
        self.push_loop(labels);
        let saved = self.cur_scope;
        let s = self.new_scope(saved, false, false);
        self.enter_block(s);
        self.for_target(head, kw, decl, kind);
        let (umark, uend) = self.using_open(-1);
        if decl && (self.tok_is(head, "using") || self.tok_is(head, "await")) {
            self.for_using(head, kw);
        }
        self.goto(body);
        self.sub_statement(false);
        self.using_close(umark, uend);
        self.leave_block(s, saved);
        let ctl = self.f().ctl.pop().unwrap();
        let cont = self.pc();
        for x in ctl.conts.iter() {
            self.patch_to(*x, cont);
        }
        self.emit(OP_JUMP, top, 0);
        self.patch(next);
        if !of {
            for x in ctl.breaks.iter() {
                self.patch(*x);
            }
            self.f().ctl.pop();
            self.op(OP_POP);
            return;
        }
        self.op(OP_END_TRY);
        self.op(OP_POP);
        let j_done = self.emit(OP_JUMP, 0, 0);
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
        self.op(OP_END_TRY);
        self.emit(OP_ITER_CLOSE, 0, 0);
        let j_break = self.emit(OP_JUMP, 0, 0);
        self.patch(t);
        self.op(OP_SWAP);
        self.emit(OP_ITER_CLOSE, 0, 1);
        self.op(OP_THROW);
        self.patch(j_done);
        self.patch(j_break);
        self.f().ctl.pop();
    }

    /// `for (using x of …)`: each step's x is a resource of the step's own
    /// `using` region.
    fn for_using(&mut self, head: int, kw: int) {
        let is_async = self.tok_is(head, "await");
        self.using_begin(is_async);
        let n = self.usings.len() - 1;
        let list = self.usings[n].list.clone();
        let name = self.tok_text(kw - 1);
        self.push_using_api("add");
        self.emit_ref(list.as_str(), M_LOAD, false);
        self.emit_ref(name.as_str(), M_LOAD, false);
        self.op(if is_async { OP_TRUE } else { OP_FALSE });
        self.emit(OP_CALL, 3, 0);
        self.op(OP_POP);
    }

    /// The loop's target, read from `head`: the value is on the stack.
    fn for_target(&mut self, head: int, kw: int, decl: bool, kind: int) {
        let back = self.pos;
        self.goto(head);
        if decl {
            self.next();
            if self.tok_is(head, "await") {
                self.next();
            }
            self.pattern_target(kind);
            if self.is("=") {
                self.goto(kw);
            }
            self.mark_pending();
        } else {
            self.pattern_target(-2);
        }
        if self.pos != kw {
            self.fail("invalid left-hand side in a for-in or for-of loop");
            return;
        }
        self.goto(back);
    }

    /// `for await (x of y)`: each step awaits `it.next()`.
    fn for_await_rest(&mut self, head: int, kw: int, body: int, decl: bool, kind: int, labels: Vec<String>) {
        self.op(OP_ASYNC_ITER);
        self.f().ctl.push(new_ctl(CT_ITEM));
        let top = self.pc();
        self.op(OP_DUP);
        let a_next = self.atom("next");
        self.emit(OP_GET_METHOD, a_next, 0);
        self.emit(OP_CALL, 0, 0);
        self.emit(OP_YIELD, 2, 0);
        self.resume_point();
        self.op(OP_DUP);
        let a_done = self.atom("done");
        self.emit(OP_GET_PROP, a_done, 0);
        let exit = self.emit(OP_JT, 0, 0);
        let a_value = self.atom("value");
        self.emit(OP_GET_PROP, a_value, 0);
        self.push_loop(labels);
        let saved = self.cur_scope;
        let s = self.new_scope(saved, false, false);
        self.enter_block(s);
        self.for_target(head, kw, decl, kind);
        let (umark, uend) = self.using_open(-1);
        if decl && (self.tok_is(head, "using") || self.tok_is(head, "await")) {
            self.for_using(head, kw);
        }
        self.goto(body);
        self.sub_statement(false);
        self.using_close(umark, uend);
        self.leave_block(s, saved);
        let ctl = self.f().ctl.pop().unwrap();
        let cont = self.pc();
        for x in ctl.conts.iter() {
            self.patch_to(*x, cont);
        }
        self.emit(OP_JUMP, top, 0);
        self.patch(exit);
        self.op(OP_POP);
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
        self.f().ctl.pop();
        self.op(OP_POP);
    }

    fn try_statement(&mut self) {
        self.next();
        if !self.is("{") {
            self.expect("{");
            return;
        }
        // where the catch and finally blocks are
        let mut p = self.after_mate(self.pos);
        let mut has_catch = false;
        let mut fin: int = -1;
        if self.tok_is(p, "catch") {
            has_catch = true;
            p += 1;
            if self.tok_is(p, "(") {
                p = self.after_mate(p);
            }
            if self.tok_is(p, "{") {
                p = self.after_mate(p);
            }
        }
        if p >= 0 && self.tok_is(p, "finally") && self.tok_is(p + 1, "{") {
            fin = p + 1;
        }
        if !has_catch && fin < 0 {
            self.fail("try without catch or finally");
            return;
        }
        let t = self.emit(OP_TRY, 0, 0);
        let mut c = new_ctl(CT_TRY);
        c.fin = fin;
        c.installed = true;
        self.f().ctl.push(c);
        self.block_statement();
        self.f().ctl.pop();
        self.op(OP_END_TRY);
        if fin >= 0 {
            self.block_at(fin);
        }
        let j_end = self.emit(OP_JUMP, 0, 0);
        self.patch(t);
        if has_catch {
            self.next();
            let mut t2: int = -1;
            if fin >= 0 {
                t2 = self.emit(OP_TRY, 0, 0);
                let mut c2 = new_ctl(CT_TRY);
                c2.fin = fin;
                c2.installed = true;
                self.f().ctl.push(c2);
            }
            let saved = self.cur_scope;
            let s = self.new_scope(saved, false, false);
            self.enter_block(s);
            if self.eat("(") {
                if !(self.kind() == T_IDENT && self.peek_is(1, ")")) {
                    self.catch_patterns.push(s);
                }
                self.pattern_target(K_CATCH);
                self.skip_annotation();
                self.expect(")");
            } else {
                self.op(OP_POP);
            }
            self.emit(PH_HOIST, s, 0);
            let cend = self.mate_of(self.pos);
            self.expect("{");
            let (cmark, csaved) = self.using_open(cend);
            while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
                self.list_stmt = true;
                self.using_here = true;
                self.statement();
            }
            self.using_close(cmark, csaved);
            self.expect("}");
            self.leave_block(s, saved);
            if fin >= 0 {
                self.f().ctl.pop();
                self.op(OP_END_TRY);
                self.block_at(fin);
                let j2 = self.emit(OP_JUMP, 0, 0);
                self.patch(t2);
                self.finally_on_exception(fin);
                self.patch(j2);
            }
        } else {
            self.finally_on_exception(fin);
        }
        self.patch(j_end);
        if fin >= 0 {
            let a = self.after_mate(fin);
            self.goto(a);
        }
    }

    fn finally_on_exception(&mut self, fin: int) {
        self.f().ctl.push(new_ctl(CT_ITEM));
        self.block_at(fin);
        self.f().ctl.pop();
        self.op(OP_THROW);
    }

    /// From `i` in a switch's body, the next `case` / `default` of this
    /// switch, or its `}`.
    fn next_clause(&self, i: int, end: int) -> int {
        let mut k = i;
        while k < end {
            let t = &self.toks[k as usize];
            if (t.kind as int) == T_PUNCT {
                let s = self.names[t.v as usize].as_str();
                if s == "(" || s == "[" || s == "{" {
                    let m = self.mate[k as usize] as int;
                    if m < 0 {
                        return end;
                    }
                    k = m + 1;
                    continue;
                }
            } else if (t.kind as int) == T_IDENT && (t.flags & PT_ESCAPED) == 0 && (self.names[t.v as usize].as_str() == "case" || self.names[t.v as usize].as_str() == "default") {
                if !(self.tok_is(k - 1, ".") || self.tok_is(k - 1, "?.")) {
                    return k;
                }
            }
            k += 1;
        }
        end
    }

    fn switch_statement(&mut self) {
        self.next();
        self.expect("(");
        self.expression();
        self.expect(")");
        if !self.is("{") {
            self.expect("{");
            return;
        }
        let end = self.mate_of(self.pos);
        self.next();
        self.f().ctl.push(new_ctl(CT_ITEM));
        let saved = self.cur_scope;
        let s = self.new_scope(saved, false, false);
        self.scopes[s as usize].is_switch = true;
        self.enter_block(s);
        let labels = self.take_labels();
        let mut sw = new_ctl(CT_SWITCH);
        sw.labels = labels;
        self.f().ctl.push(sw);
        self.emit(PH_HOIST, s, 0);
        // the tests first, then the statements
        let mut jumps: Vec<int> = Vec::new();
        let mut bodies: Vec<int> = Vec::new();
        let mut default_case: int = -1;
        let mut idx: int = 0;
        while self.pos < end && self.error.is_empty() {
            if self.is("case") {
                self.next();
                self.expression();
                let j = self.emit(OP_CASE_JT, 0, 0);
                jumps.push(j);
            } else if self.is("default") {
                self.next();
                if default_case >= 0 {
                    self.fail("More than one default clause in switch statement");
                    return;
                }
                default_case = idx;
                jumps.push(-1);
            } else {
                self.fail("expected case or default");
                return;
            }
            self.expect(":");
            bodies.push(self.pos);
            let nx = self.next_clause(self.pos, end);
            self.goto(nx);
            idx += 1;
        }
        let j_default = self.emit(OP_JUMP, 0, 0);
        let mut i: int = 0;
        while i < idx && self.error.is_empty() {
            if i == default_case {
                self.patch(j_default);
            } else {
                self.patch(jumps[i as usize]);
            }
            self.goto(bodies[i as usize]);
            let stop = self.next_clause(self.pos, end);
            while self.pos < stop && self.error.is_empty() {
                self.list_stmt = true;
                self.statement();
            }
            i += 1;
        }
        if default_case < 0 {
            self.patch(j_default);
        }
        self.goto(end);
        self.expect("}");
        let ctl = self.f().ctl.pop().unwrap();
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
        self.leave_block(s, saved);
        self.f().ctl.pop();
        self.op(OP_POP);
    }

    fn with_statement(&mut self) {
        if self.strict || self.f().proto.strict {
            self.fail("Strict mode code may not include a with statement");
            return;
        }
        self.next();
        self.expect("(");
        self.expression();
        self.expect(")");
        let saved = self.cur_scope;
        let s = self.new_scope(saved, false, false);
        self.with_count += 1;
        let wn = format!("%with{}", self.with_count);
        let b = self.declare(s, wn.as_str(), K_LET);
        self.mark_inited(b);
        self.scopes[s as usize].with_bind = b;
        self.has_with = true;
        self.enter_block(s);
        self.emit_bind_init(b);
        self.op(OP_POP);
        self.sub_statement(false);
        self.leave_block(s, saved);
    }

    fn function_decl(&mut self, in_list: bool, extra: int) {
        let decl_scope = self.cur_scope;
        let at_body_top = in_list && decl_scope == self.f().var_scope;
        let mut flags = extra;
        if self.peek_is(1, "*") {
            flags |= F_GENERATOR;
        }
        let name = if self.peek_is(1, "*") { self.tok_text(self.pos + 2) } else { self.tok_text(self.pos + 1) };
        let p = self.function_expr(true, extra);
        if !self.error.is_empty() {
            return;
        }
        if in_list {
            if at_body_top {
                if !self.check_var(name.as_str()) {
                    return;
                }
            } else {
                let plain = (flags & (F_ASYNC | F_GENERATOR)) == 0;
                if !plain || self.scopes[decl_scope as usize].odd_fns.contains_key(&name) {
                    if self.scopes[decl_scope as usize].names.contains_key(&name) {
                        self.fail(format!("Identifier '{}' has already been declared", name).as_str());
                        return;
                    }
                }
                if !self.check_lex(decl_scope, name.as_str(), K_FUNC) {
                    return;
                }
                if !plain {
                    self.scopes[decl_scope as usize].odd_fns.insert(name.clone(), true);
                }
            }
            self.declare(decl_scope, name.as_str(), K_FUNC);
            self.hoists.push(PHoist { proto: p, name: name.clone() });
            let h = (self.hoists.len() as int) - 1;
            self.scopes[decl_scope as usize].hoists.push(h);
        }
        if !at_body_top && (flags & (F_ASYNC | F_GENERATOR)) == 0 {
            self.annexes.push(PAnnex { scope: decl_scope, name: name.clone(), proto: p, var_bind: -1 });
            let k = (self.annexes.len() as int) - 1;
            self.f().annex.push(k);
            self.emit(PH_ANNEX, k, 0);
        }
    }

    fn class_decl(&mut self) {
        let name = self.tok_text(self.pos + 1);
        let scope = self.cur_scope;
        self.class_expr(true);
        if !self.error.is_empty() {
            return;
        }
        if !self.check_lex(scope, name.as_str(), K_CLASS) {
            return;
        }
        let b = self.declare(scope, name.as_str(), K_CLASS);
        self.emit_bind_init(b);
        self.op(OP_POP);
        self.mark_inited(b);
    }
}

impl PComp {
    // =====================================================================
    // destructuring
    //
    // `pk` is the binding kind of a declaration's pattern, or -1 for an
    // assignment's target, -2 for a for-in / for-of loop's (written as an
    // initialization). The value is on the stack; the code takes it.

    /// A target and its default (`= d`), at the current token. The target
    /// is read after the default, as its code runs after it.
    fn pattern_elem(&mut self, pk: int) {
        let ts = self.pos;
        self.skip_target(pk);
        let single = self.pos - ts == 1 && (self.toks[ts as usize].kind as int) == T_IDENT;
        if pk == K_PARAM {
            self.skip_annotation();
        }
        if self.is("=") {
            self.next();
            self.op(OP_DUP);
            let j = self.emit(OP_JNOT_UNDEF, 0, 0);
            self.op(OP_POP);
            let e = self.assign();
            if single {
                let nm = self.tok_text(ts);
                self.name_anonymous(e, nm.as_str());
            }
            self.patch(j);
        }
        let after = self.pos;
        self.goto(ts);
        self.pattern_target(pk);
        self.goto(after);
    }

    fn skip_target(&mut self, pk: int) {
        if (self.is("[") || self.is("{")) && (pk >= 0 || self.nested_pattern()) {
            let a = self.after_mate(self.pos);
            if a < 0 {
                self.fail("unbalanced brackets");
                return;
            }
            self.goto(a);
            return;
        }
        if pk >= 0 {
            if self.kind() == T_IDENT {
                self.next();
            } else {
                let t = self.text();
                self.fail(format!("expected an identifier but found '{}'", t).as_str());
            }
            return;
        }
        let k = self.scan_to(self.pos, ", =", pk == -2);
        self.goto(k);
    }

    /// `[` / `{` here starts a nested pattern of an assignment target (not
    /// a member of a literal).
    fn nested_pattern(&self) -> bool {
        let a = self.after_mate(self.pos);
        if a < 0 {
            return false;
        }
        self.tok_is(a, ",") || self.tok_is(a, "=") || self.tok_is(a, "]") || self.tok_is(a, "}") || self.tok_is(a, ")") || self.tok_is(a, "of") || self.tok_is(a, "in")
    }

    /// A target without its default.
    fn pattern_target(&mut self, pk: int) {
        if self.is("[") && (pk >= 0 || self.nested_pattern()) {
            self.pattern_array(pk);
            return;
        }
        if self.is("{") && (pk >= 0 || self.nested_pattern()) {
            self.pattern_object(pk);
            return;
        }
        if pk >= 0 {
            let name = self.binding_ident();
            if !self.error.is_empty() {
                return;
            }
            if self.strict && (name.as_str() == "eval" || name.as_str() == "arguments") && pk != K_PARAM {
                self.fail("Unexpected eval or arguments in strict mode");
                return;
            }
            let scope = if pk == K_VAR { self.f().var_scope } else { self.cur_scope };
            if pk == K_VAR {
                if !self.check_var(name.as_str()) {
                    return;
                }
            } else if pk == K_LET || pk == K_CONST {
                if name.as_str() == "let" {
                    self.fail("let is disallowed as a lexically bound name");
                    return;
                }
                if !self.check_lex(scope, name.as_str(), pk) {
                    return;
                }
            } else if pk == K_CATCH && self.scopes[scope as usize].names.contains_key(name.as_str()) {
                self.fail(format!("Identifier '{}' has already been declared", name).as_str());
                return;
            }
            let b = self.declare(scope, name.as_str(), pk);
            let decl = pk == K_LET || pk == K_CONST || pk == K_PARAM;
            if decl {
                self.pending_inits.push(b);
            }
            if pk == K_PARAM {
                self.param_names.push(name.clone());
            }
            self.emit_ref(name.as_str(), M_INIT, decl);
            self.op(OP_POP);
            if pk == K_PARAM {
                // a parameter is set as its pattern goes: a later default
                // in the same pattern reads it (`([a, b = a]) => …`)
                self.mark_inited(b);
            }
            return;
        }
        let e = self.lhs();
        if !self.error.is_empty() {
            return;
        }
        let last = self.last_op();
        if e.k == E_IDENT && (last.code as int) == PH_REF {
            let name = self.ref_name(e.a as usize);
            if self.strict && (name.as_str() == "eval" || name.as_str() == "arguments") {
                self.fail("Unexpected eval or arguments in strict mode");
                return;
            }
            self.pop_op();
            self.emit3(PH_REF, e.a, M_STORE, 0);
            self.op(OP_POP);
            return;
        }
        if (e.k == E_MEMBER || e.k == E_PRIVATE) && ((last.code as int) == OP_GET_PROP || (last.code as int) == OP_GET_PRIVATE) {
            self.pop_op();
            self.op(OP_SWAP);
            self.emit_member_set(last);
            self.op(OP_POP);
            return;
        }
        if e.k == E_INDEX && (last.code as int) == OP_GET_ELEM {
            self.pop_op();
            self.op(OP_ROT3);
            self.op(OP_ROT3);
            self.op(OP_SET_ELEM);
            self.op(OP_POP);
            return;
        }
        self.fail("invalid destructuring target");
    }

    fn pattern_array(&mut self, pk: int) {
        let open = self.pos;
        let close = self.mate_of(open);
        // the elements, as the literal would count them
        let mut items: int = 0;
        let mut has_rest = false;
        let mut k = open + 1;
        while k < close {
            if self.tok_is(k, ",") {
                items += 1;
                k += 1;
                continue;
            }
            if self.tok_is(k, "...") {
                has_rest = true;
            }
            items += 1;
            k = self.scan_to(k, ",", false);
            if k < close {
                k += 1;
            }
        }
        self.emit(OP_TO_ARRAY, if has_rest { 0 } else { items + 1 }, 0);
        self.next();
        let mut i: int = 0;
        while !self.is("]") && self.kind() != T_EOF && self.error.is_empty() {
            if self.is(",") {
                self.next();
                i += 1;
                continue;
            }
            if self.eat("...") {
                self.emit(OP_ARRAY_REST, i, 0);
                self.pattern_target(pk);
                i += 1;
                if !self.is("]") {
                    self.fail("Rest element must be last element");
                    return;
                }
                continue;
            }
            self.op(OP_DUP);
            self.emit(OP_INT, i, 0);
            self.op(OP_GET_ELEM);
            self.pattern_elem(pk);
            i += 1;
            if !self.is("]") {
                self.expect(",");
            }
        }
        self.expect("]");
        self.op(OP_POP);
    }

    /// Whether the object pattern at `open` (its `{`) has a `...rest`.
    fn pattern_has_rest(&mut self, open: int) -> bool {
        let end = self.mate_of(open);
        let mut k = open + 1;
        while k < end && k >= 0 {
            if self.tok_is(k, "...") {
                return true;
            }
            if self.tok_is(k, "[") || self.tok_is(k, "{") || self.tok_is(k, "(") {
                k = self.after_mate(k);
            } else {
                k += 1;
            }
        }
        false
    }

    fn pattern_object(&mut self, pk: int) {
        self.op(OP_REQUIRE_OBJ);
        let has_rest = self.pattern_has_rest(self.pos);
        self.next();
        let mut seen: Vec<String> = Vec::new();
        let mut computed: int = 0;
        while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
            if self.eat("...") {
                let cnt = seen.len() as int;
                for s in seen.clone() {
                    self.push_str(s.as_str());
                }
                self.emit(OP_OBJ_REST, cnt, computed);
                self.pattern_target(pk);
                if !self.is("}") {
                    self.expect(",");
                }
                continue;
            }
            let key_pos = self.pos;
            let key_ident = self.kind() == T_IDENT;
            self.op(OP_DUP);
            if self.eat("[") {
                self.assign();
                self.expect("]");
                if has_rest {
                    // the key stays below the source for the rest
                    self.op(OP_TO_KEY);
                    self.op(OP_DUP);
                    self.op(OP_ROT4);
                    computed += 1;
                }
                self.op(OP_GET_ELEM);
            } else {
                let name = if self.kind() == T_STR {
                    let s = self.text();
                    self.next();
                    s
                } else if self.kind() == T_NUM {
                    let v = self.tok_num(self.pos);
                    self.next();
                    number_to_string(v)
                } else {
                    self.ident_name()
                };
                seen.push(name.clone());
                let ix = crate::jsstr::array_index(name.as_str());
                if ix >= 0 && ix < 2147483647 {
                    // an index key: an array's element
                    self.push_num(ix as double);
                    self.op(OP_GET_ELEM);
                } else {
                    let at = self.atom(name.as_str());
                    self.emit(OP_GET_PROP, at, 0);
                }
            }
            if self.eat(":") {
                self.pattern_elem(pk);
            } else if key_ident {
                let nm = self.tok_text(key_pos);
                if is_reserved(nm.as_str()) {
                    self.fail("unexpected reserved word");
                    return;
                }
                self.goto(key_pos);
                self.pattern_elem(pk);
            } else {
                self.fail("expected ':' in a pattern");
                return;
            }
            if !self.is("}") {
                self.expect(",");
            }
        }
        self.expect("}");
        self.op(OP_POP);
    }

    // =====================================================================
    // expressions

    fn name_anonymous(&mut self, e: Ex, name: &str) {
        if e.k == E_ANON {
            // a module's `export default function () {}` (its binding is
            // `__cerxesDefault`) is named default
            let c = if name == "__cerxesDefault" { self.str_const("default") } else { self.str_const(name) };
            self.emit(OP_SET_NAME, c, 0);
        }
    }

    /// A jump taken when the condition just written is false; the jump is
    /// the last op.
    fn cond_jump_false(&mut self, e: Ex) {
        let last = self.last_op();
        if e.k == E_CMP && is_compare((last.code as int)) {
            self.pop_op();
            self.emit(OP_CMP_JF, 0, (last.code as int));
            return;
        }
        if e.k == E_NOT && (last.code as int) == OP_NOT {
            self.pop_op();
            self.emit(OP_JT, 0, 0);
            return;
        }
        self.emit(OP_JF, 0, 0);
    }

    fn resume_point(&mut self) {
        let r = self.emit(OP_GEN_RESUME, 0, 0);
        let j = self.emit(OP_JUMP, 0, 0);
        self.patch(r);
        self.unwind_to(-1);
        self.op(OP_RETURN);
        self.patch(j);
    }

    pub fn expression(&mut self) -> Ex {
        let first = self.assign();
        if !self.is(",") {
            return first;
        }
        while self.eat(",") {
            self.op(OP_POP);
            self.assign();
        }
        ex(E_OTHER)
    }

    fn check_strict_target(&mut self, e: Ex) {
        if self.strict && e.k == E_IDENT {
            let s = self.ref_name(e.a as usize);
            if s.as_str() == "eval" || s.as_str() == "arguments" {
                self.fail("Unexpected eval or arguments in strict mode");
            }
        }
    }

    /// TypeScript: the parameter here is a name with a type (`a: T`,
    /// `a?: T`) and nothing more.
    fn plain_typed_param(&mut self) -> bool {
        let mut k = self.pos + 1;
        if self.tok_is(k, "?") {
            k += 1;
        }
        if !self.tok_is(k, ":") {
            return self.tok_is(k, ",") || self.tok_is(k, ")");
        }
        let e = self.type_end(k + 1);
        e >= 0 && (self.tok_is(e, ",") || self.tok_is(e, ")"))
    }

    fn arrow_ahead(&mut self) -> bool {
        if self.ts {
            return self.ts_arrow_ahead();
        }
        let mut k: int = 0;
        if self.peek_is(0, "async") && !self.tok_nl(self.pos + 1) && (self.peek_kind(1) == T_IDENT || self.peek_is(1, "(")) {
            if self.peek_kind(1) == T_IDENT {
                return self.peek_is(2, "=>");
            }
            k = 1;
        }
        if self.peek_kind(k) == T_IDENT && !self.peek_is(k, "(") {
            return self.peek_is(k + 1, "=>") && k == 0 && !self.tok_nl(self.pos + 1);
        }
        if !self.peek_is(k, "(") {
            return false;
        }
        let a = self.after_mate(self.pos + k);
        if a < 0 {
            return false;
        }
        self.tok_is(a, "=>") && !self.tok_nl(a)
    }

    /// arrow_ahead in TypeScript: type parameters before the list, a
    /// return type after it (not in a conditional's first branch, where
    /// `c ? (a) : b => d` is the conditional).
    fn ts_arrow_ahead(&mut self) -> bool {
        let mut k: int = self.pos;
        if self.is("async") && !self.tok_nl(self.pos + 1) && (self.peek_kind(1) == T_IDENT || self.peek_is(1, "(") || self.peek_is(1, "<")) {
            if self.peek_kind(1) == T_IDENT && !self.peek_is(1, "(") {
                return self.peek_is(2, "=>");
            }
            k += 1;
        }
        if self.tok_is(k, "<") {
            let e = self.type_params_end(k);
            if e < 0 || !self.tok_is(e, "(") {
                return false;
            }
            k = e;
        } else if (self.toks[k as usize].kind as int) == T_IDENT && !self.tok_is(k, "(") {
            return k == self.pos && self.tok_is(k + 1, "=>") && !self.tok_nl(k + 1);
        }
        if !self.tok_is(k, "(") {
            return false;
        }
        let a = self.after_mate(k);
        if a < 0 {
            return false;
        }
        if self.tok_is(a, "=>") {
            return !self.tok_nl(a);
        }
        if self.tok_is(a, ":") && !(self.pos > 0 && self.tok_is(self.pos - 1, "?")) {
            let e = self.return_type_end(a + 1);
            return e >= 0 && self.tok_is(e, "=>") && !self.tok_nl(e);
        }
        false
    }

    fn yield_expr(&mut self) -> Ex {
        if self.module_code && self.fs.len() == self.module_depth {
            // a module's own pause (esm.rs) is its only `yield`
            if self.module_yielded {
                self.fail("Unexpected reserved word 'yield'");
                return ex(E_OTHER);
            }
            self.module_yielded = true;
        }
        if self.in_params {
            self.fail("Yield expression not allowed in formal parameter");
            return ex(E_OTHER);
        }
        self.next();
        let mut delegate = false;
        let mut has = false;
        if !self.nl_before() && self.eat("*") {
            delegate = true;
            self.assign();
            has = true;
        } else if !self.nl_before() && !self.is(")") && !self.is("]") && !self.is("}") && !self.is(",") && !self.is(";") && !self.is(":") && self.kind() != T_EOF && !(self.kind() == T_IDENT && self.is("in")) {
            self.assign();
            has = true;
        }
        if !delegate {
            if !has {
                self.op(OP_UNDEF);
            }
            if self.f().proto.is_async {
                self.emit(OP_YIELD, 2, 0);
                self.resume_point();
            }
            self.emit(OP_YIELD, 0, 0);
            self.resume_point();
            return ex(E_OTHER);
        }
        if self.f().proto.is_async {
            // [iter, received, mode] → call → await → done? → yield
            self.op(OP_ASYNC_ITER);
            self.op(OP_UNDEF);
            self.emit(OP_INT, 0, 0);
            let atop = self.pc();
            let call = self.emit(OP_ASTAR_CALL, 0, 0);
            self.emit(OP_YIELD, 2, 0);
            self.resume_point();
            let dstep = self.emit(OP_ASTAR_DONE, 0, 0);
            self.emit(OP_YIELD, 0, 0);
            self.emit(OP_JUMP, atop, 0);
            let aret = self.pc();
            self.unwind_to(-1);
            self.op(OP_RETURN);
            let adone = self.pc();
            let j = self.fs.len() - 1;
            self.fs[j].proto.code[call as usize].a = (adone as i32);
            self.fs[j].proto.code[call as usize].b = (aret as i32);
            self.fs[j].proto.code[dstep as usize].a = (adone as i32);
            self.fs[j].proto.code[dstep as usize].b = (aret as i32);
            return ex(E_OTHER);
        }
        self.op(OP_ITER_VALUES);
        self.op(OP_UNDEF);
        self.emit(OP_INT, 0, 0);
        let top = self.pc();
        let step = self.emit(OP_YIELD_STAR, 0, 0);
        self.emit(OP_YIELD, 1, 0);
        self.emit(OP_JUMP, top, 0);
        let ret = self.pc();
        self.unwind_to(-1);
        self.op(OP_RETURN);
        let done = self.pc();
        let i = self.fs.len() - 1;
        self.fs[i].proto.code[step as usize].a = (done as i32);
        self.fs[i].proto.code[step as usize].b = (ret as i32);
        ex(E_OTHER)
    }

    fn assign(&mut self) -> Ex {
        if !self.error.is_empty() {
            return ex(E_OTHER);
        }
        if self.arrow_ahead() {
            return self.arrow();
        }
        if self.is("yield") && self.in_generator && !self.escaped() {
            return self.yield_expr();
        }
        // `[a, b] = v`, `({a} = v)`: a pattern
        if self.is("[") || self.is("{") {
            let a = self.after_mate(self.pos);
            if a >= 0 && self.tok_is(a, "=") {
                let ts = self.pos;
                self.goto(a);
                self.next();
                self.assigns += 1;
                self.assign();
                self.op(OP_DUP);
                let after = self.pos;
                self.goto(ts);
                if self.is("[") {
                    self.pattern_array(-1);
                } else {
                    self.pattern_object(-1);
                }
                self.goto(after);
                return ex(E_OTHER);
            }
        }
        let left = self.conditional();
        if self.kind() != T_PUNCT {
            return left;
        }
        let op = self.text();
        if !is_assign_op(op.as_str()) {
            return left;
        }
        self.next();
        self.assigns += 1;
        let o = op.as_str();
        let last = self.last_op();
        let n = self.pc();
        let logical = o == "&&=" || o == "||=" || o == "??=";
        let jcode = if o == "&&=" {
            OP_JF_KEEP
        } else if o == "||=" {
            OP_JT_KEEP
        } else {
            OP_JNN_KEEP
        };
        if left.k == E_IDENT && (last.code as int) == PH_REF && left.b == n - 1 {
            self.check_strict_target(left);
            let r = left.a;
            let name = self.ref_name(r as usize);
            if self.has_with && !logical {
                // the reference is resolved before the right side
                self.pop_op();
                self.emit3(PH_REF, r, M_WREF, 0);
                if o != "=" {
                    self.emit3(PH_REF, r, M_WLOAD, 0);
                }
                let e = self.assign();
                if o == "=" {
                    self.name_anonymous(e, name.as_str());
                } else {
                    let bop = &o[0..(o.as_bytes().len() - 1)];
                    let code = binary_code(bop);
                    self.op(code);
                }
                let strict = if self.strict || self.f().proto.strict { 1 } else { 0 };
                self.emit3(PH_REF, r, M_WSTORE, strict);
                return ex(E_OTHER);
            }
            if o == "=" {
                self.pop_op();
                let e = self.assign();
                self.name_anonymous(e, name.as_str());
                let st = self.emit3(PH_REF, r, M_STORE, 0);
                if e.k == E_ADD && e.a >= 0 && self.ref_name(e.a as usize) == name {
                    return Ex { k: E_ADDASSIGN, a: e.b, b: st, c: e.c };
                }
                return ex(E_OTHER);
            }
            if logical {
                let j = self.emit(jcode, 0, 0);
                let e = self.assign();
                self.name_anonymous(e, name.as_str());
                self.emit3(PH_REF, r, M_STORE, 0);
                self.patch(j);
                return ex(E_OTHER);
            }
            let cnt = self.assigns;
            self.assign();
            let bop = &o[0..(o.as_bytes().len() - 1)];
            let code = binary_code(bop);
            self.op(code);
            let st = self.emit3(PH_REF, r, M_STORE, 0);
            if o == "+=" {
                return Ex { k: E_ADDASSIGN, a: left.b, b: st, c: cnt };
            }
            return ex(E_OTHER);
        }
        if (left.k == E_MEMBER || left.k == E_PRIVATE) && ((last.code as int) == OP_GET_PROP || (last.code as int) == OP_GET_PRIVATE) {
            self.pop_op();
            if o == "=" {
                self.assign();
                self.emit_member_set(last);
                return ex(E_OTHER);
            }
            self.op(OP_DUP);
            self.emit_member_get(last);
            if logical {
                let j = self.emit(jcode, 0, 0);
                self.assign();
                self.emit_member_set(last);
                let jend = self.emit(OP_JUMP, 0, 0);
                self.patch(j);
                self.op(OP_SWAP);
                self.op(OP_POP);
                self.patch(jend);
                return ex(E_OTHER);
            }
            self.assign();
            let bop = &o[0..(o.as_bytes().len() - 1)];
            let code = binary_code(bop);
            self.op(code);
            self.emit_member_set(last);
            if o == "+=" && (last.code as int) == OP_GET_PROP {
                return Ex { k: E_MEMBER_ADDASSIGN, a: (last.a as int), b: -1, c: -1 };
            }
            return ex(E_OTHER);
        }
        if left.k == E_SUPER_MEMBER && ((last.code as int) == OP_GET_SUPER || (last.code as int) == OP_GET_SUPER_ELEM) {
            let named = (last.code as int) == OP_GET_SUPER;
            if o == "=" {
                self.pop_op();
                if self.in_derived_ctor() {
                    // the receiver is read first: `this` bound yet?
                    self.emit_this();
                    self.op(OP_POP);
                }
                self.assign();
                if named {
                    self.emit(OP_SET_SUPER, last.a as int, 0);
                } else {
                    self.op(OP_SET_SUPER_ELEM);
                }
                return ex(E_OTHER);
            }
            if !named {
                self.pop_op();
                self.op(OP_DUP);
                self.op(OP_GET_SUPER_ELEM);
            }
            if logical {
                let j = self.emit(jcode, 0, 0);
                self.assign();
                if named {
                    self.emit(OP_SET_SUPER, last.a as int, 0);
                    self.patch(j);
                } else {
                    self.op(OP_SET_SUPER_ELEM);
                    let jend = self.emit(OP_JUMP, 0, 0);
                    self.patch(j);
                    self.op(OP_SWAP);
                    self.op(OP_POP);
                    self.patch(jend);
                }
                return ex(E_OTHER);
            }
            self.assign();
            let bop = &o[0..(o.as_bytes().len() - 1)];
            let code = binary_code(bop);
            self.op(code);
            if named {
                self.emit(OP_SET_SUPER, last.a as int, 0);
            } else {
                self.op(OP_SET_SUPER_ELEM);
            }
            return ex(E_OTHER);
        }
        if left.k == E_INDEX && (last.code as int) == OP_GET_ELEM {
            self.pop_op();
            if o == "=" {
                self.assign();
                self.op(OP_SET_ELEM);
                return ex(E_OTHER);
            }
            self.op(OP_DUP2);
            self.op(OP_GET_ELEM);
            if logical {
                let j = self.emit(jcode, 0, 0);
                self.assign();
                self.op(OP_SET_ELEM);
                let jend = self.emit(OP_JUMP, 0, 0);
                self.patch(j);
                self.op(OP_ROT3);
                self.op(OP_POP);
                self.op(OP_POP);
                self.patch(jend);
                return ex(E_OTHER);
            }
            self.assign();
            let bop = &o[0..(o.as_bytes().len() - 1)];
            let code = binary_code(bop);
            self.op(code);
            self.op(OP_SET_ELEM);
            return ex(E_OTHER);
        }
        if o == "=" {
            self.fail("Invalid left-hand side in assignment");
        } else {
            self.fail("invalid assignment target");
        }
        ex(E_OTHER)
    }

    fn conditional(&mut self) -> Ex {
        let t = self.binary(0);
        if !self.is("?") {
            return t;
        }
        self.next();
        self.cond_jump_false(t);
        let jf = self.pc() - 1;
        let saved = self.no_in;
        self.no_in = false;
        self.assign();
        self.no_in = saved;
        let j = self.emit(OP_JUMP, 0, 0);
        self.patch(jf);
        self.expect(":");
        self.assign();
        self.patch(j);
        ex(E_OTHER)
    }

    fn binary(&mut self, min: int) -> Ex {
        let mut left = self.unary();
        // `??` mixed with `&&` / `||` without parentheses: an early error
        let mut had_logic = false;
        let mut had_coal = false;
        loop {
            if !self.error.is_empty() {
                break;
            }
            if self.ts && (self.is("as") || self.is("satisfies")) && !self.nl_before() && 8 >= min {
                // `x as T`: the value of x
                self.next();
                if self.is("const") {
                    self.next();
                } else {
                    self.skip_type_here();
                }
                continue;
            }
            if !(self.kind() == T_PUNCT || (self.kind() == T_IDENT && !self.escaped() && (self.is("instanceof") || self.is("in")))) {
                break;
            }
            let op = self.text();
            if op.as_str() == "in" && self.no_in {
                break;
            }
            let prec = binary_prec(op.as_str());
            if prec < 0 || prec < min {
                break;
            }
            self.next();
            let o = op.as_str();
            if o == "&&" || o == "||" || o == "??" {
                let code = if o == "&&" {
                    OP_JF_KEEP
                } else if o == "||" {
                    OP_JT_KEEP
                } else {
                    OP_JNN_KEEP
                };
                if o == "??" {
                    had_coal = true;
                } else {
                    had_logic = true;
                }
                let j = self.emit(code, 0, 0);
                self.binary(prec + 1);
                if had_logic && (had_coal || (o == "??" && self.logic_top)) {
                    self.fail("Unexpected token: ?? mixed with && or || needs parentheses");
                    return ex(E_OTHER);
                }
                if o == "??" && self.logic_top {
                    self.fail("Unexpected token: ?? mixed with && or || needs parentheses");
                    return ex(E_OTHER);
                }
                self.patch(j);
                left = ex(E_OTHER);
                continue;
            }
            let cnt = self.assigns;
            if o == "**" {
                self.binary(prec);
            } else {
                self.binary(prec + 1);
            }
            let code = binary_code(o);
            if code == OP_IN && left.k == E_PRIVNAME {
                self.emit(OP_HAS_PRIVATE, left.a, left.b);
            } else {
                self.op(code);
            }
            if is_compare(code) {
                left = ex(E_CMP);
            } else if o == "+" && left.k == E_IDENT {
                left = Ex { k: E_ADD, a: left.a, b: left.b, c: cnt };
            } else {
                left = ex(E_OTHER);
            }
        }
        self.logic_top = had_logic;
        left
    }

    /// `++` / `--` on the target just read (`e`).
    fn update(&mut self, e: Ex, prefix: bool, inc: bool) -> Ex {
        self.assigns += 1;
        let last = self.last_op();
        let code = if inc { OP_INC } else { OP_DEC };
        if e.k == E_IDENT && (last.code as int) == PH_REF && e.b == self.pc() - 1 {
            self.check_strict_target(e);
            let mut flags = 0;
            if prefix {
                flags |= U_PREFIX;
            }
            if inc {
                flags |= U_INC;
            }
            let i = self.fs.len() - 1;
            self.fs[i].proto.code[e.b as usize] = Op { code: (PH_REF as i32), a: (e.a as i32), b: (M_UPD as i32), c: (flags as i32) };
            return Ex { k: E_UPDATE_IDENT, a: e.b, b: -1, c: -1 };
        }
        if (e.k == E_MEMBER || e.k == E_PRIVATE) && ((last.code as int) == OP_GET_PROP || (last.code as int) == OP_GET_PRIVATE) {
            self.pop_op();
            self.op(OP_DUP);
            self.emit_member_get(last);
            self.op(OP_TONUM);
            if prefix {
                self.op(code);
                self.emit_member_set(last);
            } else {
                self.op(OP_DUP);
                self.op(OP_ROT3);
                self.op(code);
                self.emit_member_set(last);
                self.op(OP_POP);
            }
            return ex(E_OTHER);
        }
        if e.k == E_INDEX && (last.code as int) == OP_GET_ELEM {
            self.pop_op();
            self.op(OP_DUP2);
            self.op(OP_GET_ELEM);
            self.op(OP_TONUM);
            if prefix {
                self.op(code);
                self.op(OP_SET_ELEM);
            } else {
                self.op(OP_DUP);
                self.op(OP_ROT4);
                self.op(code);
                self.op(OP_SET_ELEM);
                self.op(OP_POP);
            }
            return ex(E_OTHER);
        }
        self.fail("invalid update target");
        ex(E_OTHER)
    }

    fn unary(&mut self) -> Ex {
        if self.ts && !self.jsx && self.is("<") {
            // TypeScript's `<T>x` (not in TSX, where it opens an element)
            let e = self.type_args_end(self.pos);
            if e < 0 {
                self.ts_fail();
                return ex(E_OTHER);
            }
            self.goto(e);
            return self.unary();
        }
        if self.kind() == T_PUNCT {
            let op = self.text();
            let s = op.as_str();
            if s == "!" || s == "-" || s == "+" || s == "~" {
                self.next();
                let a = self.unary();
                if self.is("**") {
                    self.fail("unary operator before **");
                    return ex(E_OTHER);
                }
                if s == "-" && a.k == E_NUM {
                    let v = self.last_num;
                    self.pop_op();
                    self.push_num(-v);
                    return ex(E_OTHER);
                }
                if s == "-" {
                    self.op(OP_NEG);
                } else if s == "+" {
                    // a = 1: ToNumber (a BigInt throws); ++ / -- use a = 0, ToNumeric
                    self.emit(OP_TONUM, 1, 0);
                } else if s == "!" {
                    self.op(OP_NOT);
                    return ex(E_NOT);
                } else {
                    self.op(OP_BNOT);
                }
                return ex(E_OTHER);
            }
            if s == "++" || s == "--" {
                self.next();
                let a = self.unary();
                return self.update(a, true, s == "++");
            }
        }
        if self.kind() == T_IDENT && !self.escaped() {
            let op = self.text();
            let s = op.as_str();
            if s == "typeof" || s == "void" || s == "delete" || (s == "await" && self.in_async) {
                if s == "await" && self.in_params {
                    self.fail("Illegal await-expression in formal parameters of async function");
                    return ex(E_OTHER);
                }
                self.next();
                let a = self.unary();
                if !self.error.is_empty() {
                    return ex(E_OTHER);
                }
                if self.is("**") && s != "await" {
                    self.fail("unary operator before **");
                    return ex(E_OTHER);
                }
                let last = self.last_op();
                if s == "typeof" {
                    if a.k == E_IDENT && (last.code as int) == PH_REF {
                        let i = self.fs.len() - 1;
                        let n = self.fs[i].proto.code.len() - 1;
                        self.fs[i].proto.code[n].b = (M_TYPEOF as i32);
                    } else {
                        self.op(OP_TYPEOF);
                    }
                } else if s == "delete" {
                    if self.strict && a.k == E_IDENT {
                        self.fail("Delete of an unqualified identifier in strict mode.");
                        return ex(E_OTHER);
                    }
                    if a.k == E_PRIVATE || (last.code as int) == OP_GET_PRIVATE {
                        self.fail("Private fields can not be deleted");
                        return ex(E_OTHER);
                    }
                    if (a.k == E_MEMBER || a.k == E_PRIVATE) && ((last.code as int) == OP_GET_PROP || (last.code as int) == OP_GET_PRIVATE) {
                        self.pop_op();
                        self.emit(OP_DEL_PROP, (last.a as int), 0);
                    } else if a.k == E_INDEX && (last.code as int) == OP_GET_ELEM {
                        self.pop_op();
                        self.op(OP_DEL_ELEM);
                    } else if a.k == E_IDENT && (last.code as int) == PH_REF {
                        let i = self.fs.len() - 1;
                        let n = self.fs[i].proto.code.len() - 1;
                        self.fs[i].proto.code[n].b = (M_DELETE as i32);
                    } else {
                        self.op(OP_POP);
                        self.op(OP_TRUE);
                    }
                } else if s == "void" {
                    self.op(OP_POP);
                    self.op(OP_UNDEF);
                } else if self.f().proto.is_async {
                    self.emit(OP_YIELD, 2, 0);
                    self.resume_point();
                }
                return ex(E_OTHER);
            }
        }
        let e = self.lhs();
        if self.kind() == T_PUNCT && !self.nl_before() && (self.is("++") || self.is("--")) {
            let inc = self.is("++");
            self.next();
            return self.update(e, false, inc);
        }
        e
    }

    /// Arguments of a call to the stack: their count, or -1 when there is
    /// a spread (then an array of them).
    fn call_args(&mut self) -> int {
        self.expect("(");
        let saved = self.no_in;
        self.no_in = false;
        let mut cnt: int = 0;
        let mut spread = false;
        while !self.is(")") && self.kind() != T_EOF && self.error.is_empty() {
            if self.eat("...") {
                if !spread {
                    self.emit(OP_NEW_ARRAY, cnt, 0);
                    spread = true;
                }
                self.assign();
                self.op(OP_ARRAY_SPREAD);
            } else {
                self.assign();
                if spread {
                    self.op(OP_ARRAY_PUSH);
                } else {
                    cnt += 1;
                }
            }
            if !self.eat(",") {
                break;
            }
        }
        self.no_in = saved;
        self.expect(")");
        if spread {
            -1
        } else {
            cnt
        }
    }

    /// The callee just read becomes a function and its `this`.
    fn callee(&mut self, e: Ex, optional: bool) {
        let last = self.last_op();
        if e.k == E_PRIVATE && (last.code as int) == OP_GET_PRIVATE {
            self.pop_op();
            self.op(OP_DUP);
            self.emit3(OP_GET_PRIVATE, last.a as int, last.b as int, last.c as int);
            self.op(OP_SWAP);
        } else if (e.k == E_MEMBER || e.k == E_PRIVATE) && ((last.code as int) == OP_GET_PROP || (last.code as int) == OP_GET_PRIVATE) {
            self.pop_op();
            self.emit(OP_GET_METHOD, (last.a as int), if optional { 1 } else { 0 });
        } else if e.k == E_INDEX && (last.code as int) == OP_GET_ELEM {
            self.pop_op();
            self.op(OP_GET_METHOD_ELEM);
        } else if e.k == E_SUPER_MEMBER {
            self.emit_this();
        } else {
            self.op(OP_UNDEF);
        }
    }

    fn chain_check(&mut self) {
        let j = self.emit(OP_JNULLISH, 0, 1);
        let f = self.f();
        let last = f.chain.len() - 1;
        f.chain[last].push(j);
    }

    fn call(&mut self, e: Ex, optional: bool) {
        self.callee(e, optional);
        if optional {
            self.emit(OP_OVER, 0, 0);
            let j = self.emit(OP_JNULLISH, 0, 3);
            let f = self.f();
            let last = f.chain.len() - 1;
            f.chain[last].push(j);
        }
        let n = self.call_args();
        if n < 0 {
            self.op(OP_CALL_SPREAD);
        } else if !optional && e.k == E_IDENT && self.ref_name(e.a as usize).as_str() == "eval" {
            let site = self.cur_scope;
            let mut s = site;
            while s >= 0 {
                self.scopes[s as usize].has_eval = true;
                s = self.scopes[s as usize].parent;
            }
            self.eval_sites.push(site);
            if !self.strict {
                // the function's vars a sloppy eval may add: in `%ev`
                let mut fs = site;
                while fs >= 0 && !self.scopes[fs as usize].is_func {
                    fs = self.scopes[fs as usize].parent;
                }
                if fs >= 0 && !self.scopes[fs as usize].is_program && self.scopes[fs as usize].with_bind < 0 {
                    let b = self.declare(fs, "%ev", K_VAR);
                    self.scopes[fs as usize].with_bind = b;
                    self.has_with = true;
                }
            }
            let flags = self.ctx * 2 + if self.in_params { 1 } else { 0 };
            let pv = self.priv_env_desc();
            if !pv.is_empty() {
                self.eval_privs.insert(site, pv);
            }
            self.emit3(PH_EVAL, site, n, flags);
        } else {
            self.emit(OP_CALL, n, 0);
        }
    }

    /// Member access, calls, `new`, optional chains.
    fn lhs(&mut self) -> Ex {
        self.f().chain.push(Vec::new());
        let mut e: Ex;
        if self.is("new") && !self.escaped() {
            self.next();
            if self.eat(".") {
                let esc = self.escaped();
                let t = self.ident_name();
                if t.as_str() != "target" || esc {
                    self.fail("new.target expected");
                }
                if !self.new_target_ok() {
                    self.fail("new.target expression is not allowed here");
                }
                self.mark_lexical_ctor();
                self.op(OP_NEW_TARGET);
                e = ex(E_OTHER);
            } else {
                self.member_only();
                if self.ts && self.is("<") {
                    let a = self.type_args_end(self.pos);
                    if a < 0 {
                        self.ts_fail();
                    } else {
                        self.goto(a);
                    }
                }
                let mut n: int = 0;
                if self.is("(") {
                    n = self.call_args();
                }
                if n < 0 {
                    self.op(OP_NEW_SPREAD);
                } else {
                    self.emit(OP_NEW, n, 0);
                }
                e = ex(E_OTHER);
            }
        } else if self.is("super") && !self.escaped() {
            self.next();
            if self.is("(") && (self.ctx & CX_SUPER_CALL) == 0 {
                self.fail("'super' keyword unexpected here");
                return ex(E_OTHER);
            }
            if !self.is("(") && (self.ctx & CX_SUPER_PROP) == 0 {
                self.fail("'super' keyword unexpected here");
                return ex(E_OTHER);
            }
            if self.is("(") {
                self.mark_lexical_ctor();
                let n = self.call_args();
                if n < 0 {
                    self.op(OP_SUPER_CALL_SPREAD);
                } else {
                    self.emit(OP_SUPER_CALL, n, 0);
                }
                if self.in_derived_ctor() {
                    // `this` bound once: a second super(…) throws
                    let r = self.new_ref("%this", true, -1);
                    self.emit3(PH_REF, r, M_LOAD, 0);
                    self.op(OP_CHECK_UNINIT);
                    self.op(OP_POP);
                    let r2 = self.new_ref("%this", true, -1);
                    self.emit3(PH_REF, r2, M_INIT, 0);
                }
                e = ex(E_OTHER);
            } else if self.eat(".") {
                let name = self.ident_name();
                let at = self.atom(name.as_str());
                self.emit(OP_GET_SUPER, at, 0);
                e = ex(E_SUPER_MEMBER);
            } else if self.eat("[") {
                self.expression();
                self.expect("]");
                self.op(OP_GET_SUPER_ELEM);
                e = ex(E_SUPER_MEMBER);
            } else {
                self.fail("unexpected super");
                e = ex(E_OTHER);
            }
        } else if !self.ksubs.is_empty() && self.ksub_here() {
            e = ex(E_OTHER);
        } else if self.kcall_start() && self.kcall_here() {
            e = ex(E_OTHER);
        } else if self.iife_start() && self.iife_here() {
            e = ex(E_OTHER);
        } else {
            e = self.primary();
        }
        let mut chain = false;
        loop {
            if !self.error.is_empty() {
                break;
            }
            if self.is(".") {
                self.next();
                if self.kind() == T_PRIVATE {
                    let s = format!("\u{1}#{}", self.text());
                    self.next();
                    e = self.emit_priv_get(s.as_str());
                } else {
                    let name = self.ident_name();
                    let at = self.atom(name.as_str());
                    self.emit(OP_GET_PROP, at, 0);
                    e = Ex { k: E_MEMBER, a: at, b: -1, c: -1 };
                }
            } else if self.is("?.") {
                self.next();
                chain = true;
                if self.kind() == T_PRIVATE {
                    let s = format!("\u{1}#{}", self.text());
                    self.next();
                    self.chain_check();
                    e = self.emit_priv_get(s.as_str());
                } else if self.is("(") {
                    self.call(e, true);
                    e = ex(E_OTHER);
                } else if self.eat("[") {
                    self.chain_check();
                    self.expression();
                    self.expect("]");
                    self.op(OP_GET_ELEM);
                    e = ex(E_INDEX);
                } else {
                    let name = self.ident_name();
                    self.chain_check();
                    let at = self.atom(name.as_str());
                    self.emit(OP_GET_PROP, at, 0);
                    e = Ex { k: E_MEMBER, a: at, b: -1, c: -1 };
                }
            } else if self.is("[") {
                self.next();
                let saved = self.no_in;
                self.no_in = false;
                self.expression();
                self.no_in = saved;
                self.expect("]");
                self.op(OP_GET_ELEM);
                e = ex(E_INDEX);
            } else if self.is("(") {
                self.call(e, false);
                e = ex(E_OTHER);
            } else if self.kind() == T_TEMPLATE {
                if chain {
                    self.fail("Invalid tagged template on optional chain");
                    break;
                }
                self.tagged(e);
                e = ex(E_OTHER);
            } else if self.ts && self.is("!") && !self.nl_before() {
                // `x!`: not null, for the types only
                self.next();
            } else if self.ts && self.is("<") {
                // `f<T>(…)`; else a comparison
                let a = self.type_args_end(self.pos);
                if a >= 0 && (self.tok_is(a, "(") || (a < self.tok_count() && (self.toks[a as usize].kind as int) == T_TEMPLATE)) {
                    self.goto(a);
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        let i = self.fs.len() - 1;
        let last = self.fs[i].chain.len() - 1;
        let jumps = self.fs[i].chain[last].clone();
        self.fs[i].chain.truncate(last);
        for j in jumps.iter() {
            self.patch(*j);
        }
        if chain {
            return ex(E_CHAIN);
        }
        e
    }

    /// The callee of `new`: member accesses without calls.
    fn member_only(&mut self) {
        if self.is("new") && !self.escaped() {
            self.next();
            self.member_only();
            let mut n: int = 0;
            if self.is("(") {
                n = self.call_args();
            }
            if n < 0 {
                self.op(OP_NEW_SPREAD);
            } else {
                self.emit(OP_NEW, n, 0);
            }
        } else {
            self.primary();
        }
        loop {
            if !self.error.is_empty() {
                break;
            }
            if self.eat(".") {
                let name = self.ident_name();
                let at = self.atom(name.as_str());
                self.emit(OP_GET_PROP, at, 0);
            } else if self.eat("[") {
                self.expression();
                self.expect("]");
                self.op(OP_GET_ELEM);
            } else {
                break;
            }
        }
    }

    /// A template substitution's source, read and written here.
    fn sub_expression(&mut self, src: &str) {
        let saved_src = self.src_rc.clone();
        self.src_rc = Rc::new(String::from(src));
        let saved = self.push_tokens(src);
        if self.error.is_empty() {
            self.expression();
            if self.kind() != T_EOF && self.error.is_empty() {
                self.fail("bad template substitution");
            }
        }
        self.pop_tokens(&saved);
        self.src_rc = saved_src;
    }

    fn template(&mut self) -> Ex {
        let xi = (self.toks[self.pos as usize].x as int) as usize;
        let parts = self.xs[xi].parts.clone();
        let exprs = self.xs[xi].exprs.clone();
        let oks = self.xs[xi].cooked_ok.clone();
        self.next();
        for ok in oks.iter() {
            if !*ok {
                self.fail("Invalid escape sequence in template");
                return ex(E_OTHER);
            }
        }
        self.push_str(parts[0].as_str());
        let mut i: usize = 0;
        for src in exprs.iter() {
            self.sub_expression(src.as_str());
            self.op(OP_TOSTR);
            self.op(OP_ADD);
            if i + 1 < parts.len() && !parts[i + 1].is_empty() {
                let s = parts[i + 1].clone();
                self.push_str(s.as_str());
                self.op(OP_ADD);
            }
            i += 1;
        }
        ex(E_OTHER)
    }

    fn tagged(&mut self, e: Ex) {
        let last = self.last_op();
        if e.k == E_PRIVATE && (last.code as int) == OP_GET_PRIVATE {
            self.pop_op();
            self.op(OP_DUP);
            self.emit3(OP_GET_PRIVATE, last.a as int, last.b as int, last.c as int);
            self.op(OP_SWAP);
        } else if (e.k == E_MEMBER || e.k == E_PRIVATE) && ((last.code as int) == OP_GET_PROP || (last.code as int) == OP_GET_PRIVATE) {
            self.pop_op();
            self.emit(OP_GET_METHOD, (last.a as int), 0);
        } else {
            self.op(OP_UNDEF);
        }
        let xi = (self.toks[self.pos as usize].x as int) as usize;
        let parts = self.xs[xi].parts.clone();
        let exprs = self.xs[xi].exprs.clone();
        let raws = self.xs[xi].raws.clone();
        let oks = self.xs[xi].cooked_ok.clone();
        self.next();
        let fi = self.fs.len() - 1;
        let slot = self.fs[fi].proto.consts.len() as int;
        self.fs[fi].proto.consts.push(Val::Undef);
        let cnt = parts.len() as int;
        let site = self.emit(OP_TEMPLATE_OBJ, 0, slot);
        let mut i: usize = 0;
        for p in parts.iter() {
            if i < oks.len() && !oks[i] {
                self.op(OP_UNDEF);
            } else {
                self.push_str(p.as_str());
            }
            i += 1;
        }
        i = 0;
        while i < parts.len() {
            let r = if i < raws.len() { raws[i].clone() } else { String::new() };
            self.push_str(r.as_str());
            i += 1;
        }
        self.emit(OP_TEMPLATE_OBJ, cnt, -1 - slot);
        self.patch(site);
        let argc = (exprs.len() as int) + 1;
        for src in exprs.iter() {
            self.sub_expression(src.as_str());
        }
        self.emit(OP_CALL, argc, 0);
    }

    fn primary(&mut self) -> Ex {
        let k = self.kind();
        if k == T_IDENT && !self.module_path.is_empty() && self.is("__cerxesModuleExports") {
            // what a module returns (`compile_module`)
            self.next();
            let src = self.module_exports_src();
            return self.source_expr(src.as_str());
        }
        if k == T_NUM {
            let raw = self.text();
            if self.strict && raw.as_str() == "octal" {
                self.fail("Octal literals are not allowed in strict mode.");
                return ex(E_OTHER);
            }
            if raw.starts_with("n:") {
                let digits: String = String::from(&raw[2..]);
                self.next();
                self.push_bigint(digits.as_str());
                return ex(E_LITERAL);
            }
            let v = self.tok_num(self.pos);
            self.next();
            self.push_num(v);
            self.last_num = v;
            return ex(E_NUM);
        }
        if k == T_STR {
            if self.strict && self.escaped() {
                self.fail("Octal escape sequences are not allowed in strict mode.");
                return ex(E_OTHER);
            }
            let s = self.text();
            self.next();
            self.push_str(s.as_str());
            return ex(E_LITERAL);
        }
        if k == T_TEMPLATE {
            return self.template();
        }
        if k == T_REGEX {
            let s = self.text();
            let fx = self.toks[self.pos as usize].x as int;
            let f = if fx >= 0 { self.xs[fx as usize].flags.clone() } else { String::new() };
            // a literal that is not a regular expression is an early error
            let rerr = crate::regex::check(s.as_str(), f.as_str());
            if !rerr.is_empty() {
                self.fail(format!("Invalid regular expression: /{}/{}: {}", s, f, rerr).as_str());
                return ex(E_OTHER);
            }
            self.next();
            let cs = self.str_const(s.as_str());
            let cf = self.str_const(f.as_str());
            self.emit(OP_REGEX, cs, cf);
            return ex(E_LITERAL);
        }
        if k == T_PRIVATE {
            // `#x in obj`
            let s = format!("\u{1}#{}", self.text());
            self.priv_ref(s.as_str());
            self.next();
            if !self.is("in") {
                self.fail("Unexpected private name");
                return ex(E_OTHER);
            }
            let (key, brand, _) = self.priv_lookup(s.as_str());
            let at = self.atom(key.as_str());
            let bt = if brand.is_empty() { -1 } else { self.atom(brand.as_str()) };
            self.op(OP_UNDEF);
            return Ex { k: E_PRIVNAME, a: at, b: bt, c: -1 };
        }
        if k == T_IDENT {
            let w = self.text();
            let escaped = self.escaped();
            let ws = w.as_str();
            if !escaped {
                if ws == "function" {
                    let p = self.function_expr(false, 0);
                    self.emit(OP_CLOSURE, p, 0);
                    return ex(if self.last_fn_name.is_empty() { E_ANON } else { E_OTHER });
                }
                if ws == "async" && self.peek_is(1, "function") && !self.tok_nl(self.pos + 1) {
                    self.next();
                    let p = self.function_expr(false, F_ASYNC);
                    self.emit(OP_CLOSURE, p, 0);
                    return ex(if self.last_fn_name.is_empty() { E_ANON } else { E_OTHER });
                }
                if ws == "class" {
                    return self.class_expr(false);
                }
                if ws == "this" {
                    self.next();
                    self.emit_this();
                    return ex(E_LITERAL);
                }
                if ws == "null" {
                    self.next();
                    self.op(OP_NULL);
                    return ex(E_LITERAL);
                }
                if ws == "true" {
                    self.next();
                    self.op(OP_TRUE);
                    return ex(E_LITERAL);
                }
                if ws == "false" {
                    self.next();
                    self.op(OP_FALSE);
                    return ex(E_LITERAL);
                }
                if ws == "import" {
                    return self.dyn_import();
                }
                if is_reserved(ws) {
                    self.fail(format!("unexpected token '{}'", ws).as_str());
                    return ex(E_OTHER);
                }
            } else if is_reserved(ws) {
                self.fail("Keyword must not contain escaped characters");
                return ex(E_OTHER);
            }
            if self.bad_ident(ws) {
                self.fail(format!("unexpected reserved word '{}'", ws).as_str());
                return ex(E_OTHER);
            }
            if (self.ctx & CX_NO_ARGS) != 0 && ws == "arguments" {
                self.fail("'arguments' is not allowed in class field initializer or static initialization block");
                return ex(E_OTHER);
            }
            self.next();
            let at = self.emit_ref(ws, M_LOAD, false);
            let r = self.fs[self.fs.len() - 1].proto.code[at as usize].a;
            return Ex { k: E_IDENT, a: (r as int), b: at, c: -1 };
        }
        if self.is("(") {
            self.next();
            let saved = self.no_in;
            self.no_in = false;
            let e = self.expression();
            self.no_in = saved;
            self.expect(")");
            return e;
        }
        if self.is("[") {
            self.array_literal();
            return ex(E_LITERAL);
        }
        if self.is("{") {
            self.object_literal();
            return ex(E_LITERAL);
        }
        let t = self.text();
        self.fail(format!("unexpected token '{}'", t).as_str());
        ex(E_OTHER)
    }

    fn array_literal(&mut self) {
        self.next();
        let saved = self.no_in;
        self.no_in = false;
        let mut cnt: int = 0;
        let mut open = false;
        while !self.is("]") && self.kind() != T_EOF && self.error.is_empty() {
            if self.is(",") {
                self.next();
                if !open {
                    self.emit(OP_NEW_ARRAY, cnt, 0);
                    open = true;
                }
                self.op(OP_ARRAY_HOLE);
                continue;
            }
            if self.eat("...") {
                if !open {
                    self.emit(OP_NEW_ARRAY, cnt, 0);
                    open = true;
                }
                self.assign();
                self.op(OP_ARRAY_SPREAD);
            } else {
                self.assign();
                if open {
                    self.op(OP_ARRAY_PUSH);
                } else {
                    cnt += 1;
                }
            }
            if !self.is("]") {
                self.expect(",");
            }
        }
        self.no_in = saved;
        self.expect("]");
        if !open {
            self.emit(OP_NEW_ARRAY, cnt, 0);
        }
    }

    /// A property key: the code of a computed one is written; answers the
    /// name ("" when computed) and sets key_flags (1 computed, 2 private).
    fn prop_key(&mut self, key_flags: &mut int) -> String {
        *key_flags = 0;
        if self.eat("[") {
            *key_flags = 1;
            self.assign();
            self.expect("]");
            return String::new();
        }
        if self.kind() == T_PRIVATE {
            *key_flags = 2;
            let s = format!("\u{1}#{}", self.text());
            self.next();
            let (key, _, _) = self.priv_lookup(s.as_str());
            return key;
        }
        if self.kind() == T_STR {
            let s = self.text();
            self.next();
            return s;
        }
        if self.kind() == T_NUM {
            let v = self.tok_num(self.pos);
            self.next();
            return number_to_string(v);
        }
        self.ident_name()
    }

    fn object_literal(&mut self) {
        self.op(OP_NEW_OBJECT);
        self.next();
        let saved = self.no_in;
        self.no_in = false;
        let mut protos = 0;
        while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
            if self.eat("...") {
                self.assign();
                self.op(OP_INIT_SPREAD);
                if !self.is("}") {
                    self.expect(",");
                }
                continue;
            }
            let mut flags = 0;
            let mut fflags = 0;
            let ms = self.pos;
            if self.is("async") && !self.peek_is(1, "(") && !self.peek_is(1, ":") && !self.peek_is(1, ",") && !self.peek_is(1, "}") && !self.peek_is(1, "=") && !self.tok_nl(self.pos + 1) {
                self.next();
                fflags |= F_ASYNC;
            }
            if self.eat("*") {
                fflags |= F_GENERATOR;
            }
            if (self.is("get") || self.is("set")) && !self.peek_is(1, "(") && !self.peek_is(1, ":") && !self.peek_is(1, ",") && !self.peek_is(1, "}") && !self.peek_is(1, "=") {
                if self.is("get") {
                    flags |= F_GETTER;
                } else {
                    flags |= F_SETTER;
                }
                self.next();
            }
            if self.kind() == T_PRIVATE {
                self.fail("private names are not allowed in object literals");
                return;
            }
            let key_ident = self.kind() == T_IDENT;
            let key_text = self.text();
            let mut kf: int = 0;
            let name = self.prop_key(&mut kf);
            let computed = (kf & 1) != 0;
            if fflags != 0 && !self.is("(") {
                self.fail("Unexpected token in object literal: a method needs '('");
                return;
            }
            if self.is("(") {
                self.member_start = ms;
                let p = self.method(fflags | (flags & (F_GETTER | F_SETTER)), name.as_str());
                self.emit(OP_CLOSURE, p, 0);
                if (flags & (F_GETTER | F_SETTER)) != 0 {
                    let getter = (flags & F_GETTER) != 0;
                    if computed {
                        self.emit(if getter { OP_INIT_GETTER_ELEM } else { OP_INIT_SETTER_ELEM }, 0, 0);
                    } else {
                        let at = self.atom(name.as_str());
                        self.emit(if getter { OP_INIT_GETTER } else { OP_INIT_SETTER }, at, 0);
                    }
                } else if computed {
                    self.emit(OP_DEFINE_METHOD_ELEM, 0, 0);
                } else {
                    let at = self.atom(name.as_str());
                    self.emit(OP_DEFINE_METHOD, at, 0);
                }
            } else if (flags & (F_GETTER | F_SETTER)) != 0 {
                self.fail("getter or setter without a body");
                return;
            } else if self.eat(":") {
                if !computed && key_text.as_str() == "__proto__" {
                    protos += 1;
                    if protos > 1 {
                        self.fail("Duplicate __proto__ fields are not allowed in object literals");
                        return;
                    }
                }
                let e = self.assign();
                if computed {
                    self.op(OP_INIT_ELEM);
                } else {
                    self.name_anonymous(e, name.as_str());
                    let at = self.atom(name.as_str());
                    let is_proto = name.as_str() == "__proto__";
                    self.emit(OP_INIT_PROP, at, if is_proto { 1 } else { 0 });
                }
            } else if key_ident && !computed {
                if self.is("=") {
                    self.fail("Invalid shorthand property initializer");
                    return;
                }
                if is_reserved(key_text.as_str()) || self.bad_ident(key_text.as_str()) {
                    self.fail("unexpected reserved word");
                    return;
                }
                self.emit_ref(key_text.as_str(), M_LOAD, false);
                let at = self.atom(key_text.as_str());
                self.emit(OP_INIT_PROP, at, 0);
            } else {
                self.fail("expected ':' in object literal");
                return;
            }
            if !self.is("}") {
                self.expect(",");
            }
        }
        self.no_in = saved;
        self.expect("}");
    }
}

impl PComp {
    // =====================================================================
    // functions and classes

    /// `function [*] [name] (…) { … }` from the `function` token.
    fn function_expr(&mut self, decl: bool, extra: int) -> int {
        let mut first = self.pos;
        if (extra & F_ASYNC) != 0 && first > 0 && self.tok_is(first - 1, "async") {
            first -= 1;
        }
        self.next();
        let mut flags = extra;
        if self.eat("*") {
            flags |= F_GENERATOR;
        }
        if decl {
            flags |= F_DECL;
        }
        let mut name = String::new();
        if self.kind() == T_IDENT && !self.is("(") {
            if decl {
                name = self.binding_ident();
            } else {
                // an expression's name is read as its body is
                let sg = self.in_generator;
                let sa = self.in_async;
                let sc = self.ctx;
                self.in_generator = (flags & F_GENERATOR) != 0;
                self.in_async = (flags & F_ASYNC) != 0;
                self.ctx = 0;
                name = self.binding_ident();
                self.in_generator = sg;
                self.in_async = sa;
                self.ctx = sc;
            }
        } else if decl {
            self.fail("function name expected");
            return -1;
        }
        let p = self.function_rest(flags, name.as_str(), first);
        self.last_fn_name = name;
        p
    }

    /// The innermost inlined body of this function's code, or -1.
    fn inline_ctl(&self) -> int {
        let f = &self.fs[self.fs.len() - 1];
        let mut i = (f.ctl.len() as int) - 1;
        while i >= 0 {
            if f.ctl[i as usize].kind == CT_INLINE {
                return i;
            }
            i -= 1;
        }
        -1
    }

    /// `return` in an inlined body: its value into the body's result
    /// binding (`scope` of the ctl entry) and a jump to the body's end.
    fn inline_return(&mut self) {
        self.next();
        if !self.is(";") && !self.is("}") && self.kind() != T_EOF && !self.nl_before() {
            self.expression();
        } else {
            self.op(OP_UNDEF);
        }
        self.semicolon();
        let t = self.inline_ctl();
        let b = self.f().ctl[t as usize].scope;
        let name = self.binds[b as usize].name.clone();
        let r = self.new_ref(name.as_str(), false, b);
        self.emit3(PH_REF, r, M_STORE, 0);
        self.op(OP_POP);
        self.unwind_to(t);
        let j = self.emit(OP_JUMP, 0, 0);
        self.f().ctl[t as usize].breaks.push(j);
    }

    /// Whether the tokens from `a` to `b` (exclusive) may be compiled into
    /// the code around them: nothing that sees the function itself
    /// (`this`, `arguments`, `new.target`, `super`, a direct eval) and
    /// nothing that would be hoisted out of a block (`var`, a function
    /// declaration). Plain `function` bodies inside are their own and
    /// skipped; arrows see the body's `this`, so they are not.
    fn inline_ok(&self, a: int, b: int) -> bool {
        let mut i = a;
        while i < b {
            let t = &self.toks[i as usize];
            let k = t.kind as int;
            if k == T_IDENT {
                if (t.flags & PT_ESCAPED) != 0 {
                    return false;
                }
                let w = self.names[t.v as usize].as_str();
                if w == "function" {
                    // a declaration hoists: where a statement starts
                    let p = i - 1;
                    if p < a || self.tok_is(p, ";") || self.tok_is(p, "{") || self.tok_is(p, "}") || self.tok_is(p, ")") || self.tok_is(p, "else") || self.tok_is(p, "do") {
                        return false;
                    }
                    let p1 = p - 1;
                    if self.tok_is(p, ":") && p1 >= a && (self.toks[p1 as usize].kind as int) == T_IDENT {
                        let q = p - 2;
                        if q < a || self.tok_is(q, ";") || self.tok_is(q, "{") || self.tok_is(q, "}") || self.tok_is(q, ")") {
                            return false;
                        }
                    }
                    // skip its name, parameters and body
                    let mut j = i + 1;
                    if self.tok_is(j, "*") {
                        j += 1;
                    }
                    if j < b && (self.toks[j as usize].kind as int) == T_IDENT && !self.tok_is(j, "(") {
                        j += 1;
                    }
                    if !self.tok_is(j, "(") {
                        return false;
                    }
                    let pc = self.mate_of(j);
                    let ob = pc + 1;
                    if !self.tok_is(ob, "{") {
                        return false;
                    }
                    i = self.mate_of(ob) + 1;
                    continue;
                }
                if w == "this" || w == "arguments" || w == "super" || w == "yield" || w == "await" || w == "eval" || w == "var" {
                    return false;
                }
                let i1 = i + 1;
                if w == "new" && self.tok_is(i1, ".") {
                    return false;
                }
            } else if k == T_TEMPLATE {
                let xi = t.x;
                if xi >= 0 {
                    for e in self.xs[xi as usize].exprs.iter() {
                        let es = e.as_str();
                        if es.contains("this") || es.contains("arguments") || es.contains("super") || es.contains("yield") || es.contains("await") || es.contains("eval") || es.contains("var") || es.contains("function") || es.contains("new") {
                            return false;
                        }
                    }
                }
            } else if k != T_PUNCT && k != T_NUM && k != T_STR && k != T_REGEX {
                return false;
            }
            i += 1;
        }
        true
    }

    /// `function (a, b) { … }(x, y)` or `(function (…) { … })(…)`: an
    /// anonymous function called where it is made runs as a block of the
    /// code around it, with no function object, scope object or call. Its
    /// parameters are `let` bindings of the block given the arguments,
    /// `return` stores into a result binding and jumps to the block's end.
    /// Only for bodies `inline_ok` accepts, simple parameters and
    /// arguments without spread.
    fn iife_start(&self) -> bool {
        // cheap: a word of 8 letters or a one-letter punctuator
        let t = &self.toks[self.pos as usize];
        let tk = t.kind as int;
        let tl = self.names[t.v as usize].as_bytes().len();
        (tk == T_IDENT && tl == 8) || (tk == T_PUNCT && tl == 1)
    }

    #[inline(never)]
    fn iife_here(&mut self) -> bool {
        let at = self.pos;
        if self.ts || self.in_params || !self.error.is_empty() || self.fs.is_empty() {
            return false;
        }
        let wrapped = self.is("(");
        let f0 = if wrapped { at + 1 } else { at };
        let po = f0 + 1;
        if !self.tok_is(f0, "function") || !self.tok_is(po, "(") {
            return false;
        }
        let pc = self.mate_of(po);
        let mut params: Vec<String> = Vec::new();
        let mut j = f0 + 2;
        while j < pc {
            let t = &self.toks[j as usize];
            if (t.kind as int) != T_IDENT || (t.flags & PT_ESCAPED) != 0 {
                return false;
            }
            let w = self.names[t.v as usize].clone();
            if is_reserved(w.as_str()) || is_strict_reserved(w.as_str()) || w.as_str() == "eval" || w.as_str() == "arguments" || w.as_str() == "let" || w.as_str() == "yield" || w.as_str() == "await" || w.as_str() == "async" {
                return false;
            }
            for q in params.iter() {
                if q.as_str() == w.as_str() {
                    return false;
                }
            }
            params.push(w);
            j += 1;
            if j < pc {
                if !self.tok_is(j, ",") {
                    return false;
                }
                j += 1;
                if j == pc {
                    // a trailing comma
                    break;
                }
            }
        }
        let bo = pc + 1;
        if !self.tok_is(bo, "{") {
            return false;
        }
        let bc = self.mate_of(bo);
        let mut call = bc + 1;
        if wrapped {
            if !self.tok_is(call, ")") || self.mate_of(at) != call {
                return false;
            }
            call += 1;
        }
        if !self.tok_is(call, "(") {
            return false;
        }
        let ac = self.mate_of(call);
        let mut k = call;
        k += 1;
        while k < ac {
            if self.tok_is(k, "...") {
                return false;
            }
            k += 1;
        }
        // a directive prologue may make the body strict
        let b1 = bo + 1;
        if (self.toks[b1 as usize].kind as int) == T_STR {
            return false;
        }
        if !self.inline_ok(b1, bc) {
            return false;
        }
        // the arguments, in the scope around
        self.goto(call);
        let n = self.call_args();
        if n < 0 || !self.error.is_empty() {
            return true;
        }
        let end = self.pos;
        let saved = self.cur_scope;
        let s = self.new_scope(saved, false, false);
        self.enter_block(s);
        self.emit(PH_HOIST, s, 0);
        let np = params.len() as int;
        let mut pb: Vec<int> = Vec::new();
        for w in params.iter() {
            let b = self.declare(s, w.as_str(), K_LET);
            pb.push(b);
        }
        let mut i = n - 1;
        while i >= 0 {
            if i < np {
                self.emit_bind_init(pb[i as usize]);
            }
            self.op(OP_POP);
            i -= 1;
        }
        i = n;
        while i < np {
            self.op(OP_UNDEF);
            self.emit_bind_init(pb[i as usize]);
            self.op(OP_POP);
            i += 1;
        }
        for b in pb.iter() {
            self.mark_inited(*b);
        }
        let rname = format!("%iife{}", self.binds.len());
        let rb = self.declare(s, rname.as_str(), K_LET);
        self.op(OP_UNDEF);
        self.emit_bind_init(rb);
        self.op(OP_POP);
        self.mark_inited(rb);
        let mut c = new_ctl(CT_INLINE);
        c.scope = rb;
        self.f().ctl.push(c);
        self.goto(b1);
        let (mark, saved_end) = self.using_open(bc);
        while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
            self.list_stmt = true;
            self.using_here = true;
            self.statement();
        }
        self.using_close(mark, saved_end);
        let ctl = self.f().ctl.pop().unwrap();
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
        let r = self.new_ref(rname.as_str(), false, rb);
        self.emit3(PH_REF, r, M_LOAD, 0);
        self.leave_block(s, saved);
        self.goto(end);
        true
    }

    /// `f(…)`: a call by a plain name, which `kcall_here` looks at.
    fn kcall_start(&self) -> bool {
        let t = &self.toks[self.pos as usize];
        if (t.kind as int) != T_IDENT {
            return false;
        }
        let p1 = self.pos + 1;
        self.tok_is(p1, "(")
    }

    /// The tokens are the program's own and may be inlined into: no `with`
    /// and no direct eval anywhere (they change what names mean).
    fn kscan(&mut self) -> bool {
        if self.kstate == 0 {
            self.kstate = 1;
            let mut j: int = 0;
            while j < self.main_toks {
                let t = &self.toks[j as usize];
                let k = t.kind as int;
                if k == T_IDENT {
                    let w = self.names[t.v as usize].as_str();
                    if w == "with" || w == "eval" {
                        let p = j - 1;
                        let n1 = j + 1;
                        if !self.tok_is(p, ".") && !self.tok_is(p, "?.") && (w == "with" || self.tok_is(n1, "(")) {
                            self.kstate = -1;
                            return false;
                        }
                    }
                } else if k == T_TEMPLATE {
                    let xi = t.x;
                    if xi >= 0 {
                        for e in self.xs[xi as usize].exprs.iter() {
                            if e.contains("eval") || e.contains("with") {
                                self.kstate = -1;
                                return false;
                            }
                        }
                    }
                }
                j += 1;
            }
        }
        self.kstate == 1
    }

    /// The function declared as `name` (KFn), or -1: declared once, a
    /// plain function, every other use of the name a call (so never
    /// written), and a small body `inline_ok` accepts with no functions
    /// in it.
    fn kcallee(&mut self, name: &str) -> int {
        if let Some(k) = self.kcache.get(name) {
            return *k;
        }
        let k = self.kcallee_scan(name);
        self.kcache.insert(String::from(name), k);
        k
    }

    fn kcallee_scan(&mut self, name: &str) -> int {
        let nid = match self.name_ids.get(name) {
            Some(i) => *i,
            None => -1,
        };
        if nid < 0 || is_reserved(name) || is_strict_reserved(name) || name == "async" || name == "eval" || name == "arguments" || name == "let" {
            return -1;
        }
        let mut decl: int = -1;
        let mut j: int = 0;
        while j < self.main_toks {
            let t = &self.toks[j as usize];
            let k = t.kind as int;
            if k == T_TEMPLATE {
                let xi = t.x;
                if xi >= 0 {
                    for e in self.xs[xi as usize].exprs.iter() {
                        if e.contains(name) {
                            return -1;
                        }
                    }
                }
            } else if k == T_IDENT && (t.v as int) == nid {
                if (t.flags & PT_ESCAPED) != 0 {
                    return -1;
                }
                let p = j - 1;
                let n1 = j + 1;
                if self.tok_is(p, ".") || self.tok_is(p, "?.") {
                    // a property
                } else if self.tok_is(p, "function") {
                    if decl >= 0 {
                        return -1;
                    }
                    decl = j;
                } else if !self.tok_is(n1, "(") || self.tok_is(p, "new") {
                    return -1;
                }
            }
            j += 1;
        }
        if decl < 0 {
            return -1;
        }
        // a statement: `function name(…) {…}`, not async
        let q = decl - 2;
        if q >= 0 && !self.tok_is(q, ";") && !self.tok_is(q, "{") && !self.tok_is(q, "}") {
            return -1;
        }
        let po = decl + 1;
        if !self.tok_is(po, "(") {
            return -1;
        }
        let pc = self.mate_of(po);
        let mut params: Vec<String> = Vec::new();
        let mut i = po + 1;
        while i < pc {
            let t = &self.toks[i as usize];
            if (t.kind as int) != T_IDENT || (t.flags & PT_ESCAPED) != 0 {
                return -1;
            }
            let w = self.names[t.v as usize].clone();
            if is_reserved(w.as_str()) || is_strict_reserved(w.as_str()) || w.as_str() == "eval" || w.as_str() == "arguments" || w.as_str() == "let" || w.as_str() == "yield" || w.as_str() == "await" || w.as_str() == "async" {
                return -1;
            }
            for x in params.iter() {
                if x.as_str() == w.as_str() {
                    return -1;
                }
            }
            params.push(w);
            i += 1;
            if i < pc {
                if !self.tok_is(i, ",") {
                    return -1;
                }
                i += 1;
            }
        }
        let bo = pc + 1;
        if !self.tok_is(bo, "{") {
            return -1;
        }
        let bc = self.mate_of(bo);
        let b1 = bo + 1;
        if bc - bo > 120 || (self.toks[b1 as usize].kind as int) == T_STR {
            return -1;
        }
        if !self.inline_ok(b1, bc) {
            return -1;
        }
        // no functions inside: the body's bindings stay out of scope objects
        i = b1;
        while i < bc {
            let t = &self.toks[i as usize];
            let k = t.kind as int;
            if k == T_IDENT && (self.tok_is(i, "function") || self.tok_is(i, "class")) {
                return -1;
            }
            if k == T_PUNCT && self.tok_is(i, "=>") {
                return -1;
            }
            if k == T_TEMPLATE {
                let xi = t.x;
                if xi >= 0 {
                    for e in self.xs[xi as usize].exprs.iter() {
                        if e.contains("=>") || e.contains("class") {
                            return -1;
                        }
                    }
                }
            }
            i += 1;
        }
        self.kfns.push(KFn { decl: decl, params: params, bo: bo, bc: bc });
        (self.kfns.len() as int) - 1
    }

    /// The end of the argument starting at `a` (a `,` or the call's `)`
    /// at `ac`), or -1 for a spread.
    fn karg_end(&self, a: int, ac: int) -> int {
        let mut k = a;
        while k < ac {
            if self.tok_is(k, "(") || self.tok_is(k, "[") || self.tok_is(k, "{") {
                k = self.mate_of(k) + 1;
            } else if self.tok_is(k, ",") {
                return k;
            } else if self.tok_is(k, "...") {
                return -1;
            } else {
                k += 1;
            }
        }
        ac
    }

    /// `f(x, y, () => e)` where f is a KFn whose body calls its parameter
    /// once as `g()`: the body runs as a block of the code around it
    /// (parameters `let` bindings given the arguments, `return` a jump to
    /// the end) and `g()` is `e`, compiled in the call's scope: no closure,
    /// and the names the closure saw stay in registers. Whether `f` is
    /// that function where the call is (and the body's other names mean
    /// there what they mean where it is declared) is known at the end, so
    /// the call is compiled too and `finish` keeps one (PH_KALT).
    #[inline(never)]
    fn kcall_here(&mut self) -> bool {
        let at = self.pos;
        if self.ts || self.in_params || !self.error.is_empty() || self.fs.is_empty() || !self.outer_desc.is_empty() || at >= self.main_toks || self.ksubs.len() > 1 || self.escaped() {
            return false;
        }
        let call = at + 1;
        let ac = self.mate_of(call);
        // most calls: no `() =>` among the arguments
        let mut q = call + 1;
        let mut seen = false;
        while q < ac {
            if self.tok_is(q, "=>") {
                let q1 = q - 1;
                let q2 = q - 2;
                if self.tok_is(q1, ")") && self.tok_is(q2, "(") {
                    seen = true;
                    break;
                }
            }
            if self.tok_is(q, "(") || self.tok_is(q, "[") || self.tok_is(q, "{") {
                let m = self.mate_of(q);
                let m1 = q + 1;
                // `(() =>` and `((() =>` are looked at inside
                if !self.tok_is(m1, "(") {
                    q = m;
                }
            }
            q += 1;
        }
        if !seen {
            return false;
        }
        // the arguments: one closure `() => e`, the rest small and plain
        let mut starts: Vec<int> = Vec::new();
        let mut ends: Vec<int> = Vec::new();
        let mut ci: int = -1;
        let mut cb: int = -1;
        let mut ce: int = -1;
        let mut a = call + 1;
        while a < ac {
            let e = self.karg_end(a, ac);
            if e < 0 || e == a {
                return false;
            }
            let mut s0 = a;
            let mut e0 = e;
            while self.tok_is(s0, "(") && self.mate_of(s0) == e0 - 1 {
                s0 += 1;
                e0 -= 1;
            }
            let s1 = s0 + 1;
            let s2 = s0 + 2;
            let s3 = s0 + 3;
            if self.tok_is(s0, "(") && self.tok_is(s1, ")") && self.tok_is(s2, "=>") && s3 < e0 && !self.tok_is(s3, "{") {
                if ci >= 0 {
                    return false;
                }
                ci = starts.len() as int;
                cb = s3;
                ce = e0;
            } else if e - a > 16 {
                return false;
            }
            starts.push(a);
            ends.push(e);
            a = if e < ac { e + 1 } else { e };
        }
        if ci < 0 {
            return false;
        }
        // the closure's body: one expression, nothing only a function has
        let mut k = cb;
        while k < ce {
            let t = &self.toks[k as usize];
            let tk = t.kind as int;
            if tk == T_IDENT && (self.tok_is(k, "yield") || self.tok_is(k, "await")) {
                return false;
            }
            if tk == T_TEMPLATE {
                let xi = t.x;
                if xi >= 0 {
                    for e in self.xs[xi as usize].exprs.iter() {
                        if e.contains("yield") || e.contains("await") {
                            return false;
                        }
                    }
                }
            }
            if self.tok_is(k, "(") || self.tok_is(k, "[") || self.tok_is(k, "{") {
                k = self.mate_of(k);
            } else if self.tok_is(k, ",") {
                return false;
            }
            k += 1;
        }
        // the other arguments: nothing compiled twice that makes things
        let na = starts.len() as int;
        let mut i: int = 0;
        while i < na {
            if i != ci {
                let mut q = starts[i as usize];
                while q < ends[i as usize] {
                    let tk = self.toks[q as usize].kind as int;
                    if tk == T_TEMPLATE || self.tok_is(q, "function") || self.tok_is(q, "class") || self.tok_is(q, "=>") || self.tok_is(q, "yield") || self.tok_is(q, "await") {
                        return false;
                    }
                    q += 1;
                }
            }
            i += 1;
        }
        if !self.kscan() {
            return false;
        }
        let name = self.text();
        let kf = self.kcallee(name.as_str());
        if kf < 0 {
            return false;
        }
        let bo = self.kfns[kf as usize].bo;
        let bc = self.kfns[kf as usize].bc;
        let params = self.kfns[kf as usize].params.clone();
        let np = params.len() as int;
        if ci >= np || (at >= bo && at <= bc) {
            return false;
        }
        // the closure parameter: called once, `g()`, and used no other way
        let pname = params[ci as usize].clone();
        let mut hits: int = 0;
        let mut j = bo + 1;
        while j < bc {
            let t = &self.toks[j as usize];
            if (t.kind as int) == T_IDENT && self.names[t.v as usize].as_str() == pname.as_str() {
                let p = j - 1;
                let n1 = j + 1;
                let n2 = j + 2;
                if !self.tok_is(p, ".") && !self.tok_is(p, "?.") {
                    if !self.tok_is(n1, "(") || !self.tok_is(n2, ")") || self.tok_is(p, "new") {
                        return false;
                    }
                    hits += 1;
                }
            }
            j += 1;
        }
        if hits != 1 {
            return false;
        }
        let site = self.ksites.len() as int;
        let strict = self.f().proto.strict;
        let saved = self.cur_scope;
        self.ksites.push(KSite { name: name.clone(), scope: saved, strict: strict, s0: self.scopes.len() as int, r0: self.refs.len() as int, sa1: 0, sb1: 0, ra1: 0, rb1: 0, cb0: 0, cb1: 0, ex: Vec::new(), pick: 0 });
        self.emit(PH_KALT, site, 0);
        // inlined: the arguments, in the scope around
        let saved_in = self.no_in;
        self.no_in = false;
        let mut given: Vec<int> = Vec::new();
        i = 0;
        while i < na {
            if i != ci {
                self.goto(starts[i as usize]);
                self.assign();
                if self.pos != ends[i as usize] && self.error.is_empty() {
                    self.fail("inlined call: bad argument");
                }
                given.push(i);
            }
            i += 1;
        }
        self.no_in = saved_in;
        if !self.error.is_empty() {
            return true;
        }
        let s = self.new_scope(saved, false, false);
        self.enter_block(s);
        self.emit(PH_HOIST, s, 0);
        let mut pb: Vec<int> = Vec::new();
        i = 0;
        while i < np {
            if i == ci {
                pb.push(-1);
            } else {
                let b = self.declare(s, params[i as usize].as_str(), K_LET);
                pb.push(b);
            }
            i += 1;
        }
        let ng = given.len() as int;
        let mut g = ng - 1;
        while g >= 0 {
            let pi = given[g as usize];
            if pi < np {
                self.emit_bind_init(pb[pi as usize]);
            }
            self.op(OP_POP);
            g -= 1;
        }
        i = na;
        while i < np {
            if i != ci {
                self.op(OP_UNDEF);
                self.emit_bind_init(pb[i as usize]);
                self.op(OP_POP);
            }
            i += 1;
        }
        for b in pb.iter() {
            if *b >= 0 {
                self.mark_inited(*b);
            }
        }
        let rname = format!("%iife{}", self.binds.len());
        let rb = self.declare(s, rname.as_str(), K_LET);
        self.op(OP_UNDEF);
        self.emit_bind_init(rb);
        self.op(OP_POP);
        self.mark_inited(rb);
        let mut c = new_ctl(CT_INLINE);
        c.scope = rb;
        self.f().ctl.push(c);
        self.ksubs.push(KSub { name: pname, bo: bo, bc: bc, body: cb, body_end: ce, scope: saved, active: true, hits: 0, site: site });
        let cb0 = self.refs.len() as int;
        self.ksites[site as usize].cb0 = cb0;
        let b1 = bo + 1;
        self.goto(b1);
        let (mark, saved_end) = self.using_open(bc);
        while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
            self.list_stmt = true;
            self.using_here = true;
            self.statement();
        }
        self.using_close(mark, saved_end);
        let cb1 = self.refs.len() as int;
        self.ksites[site as usize].cb1 = cb1;
        let sub = self.ksubs.pop().unwrap();
        if sub.hits != 1 && self.error.is_empty() {
            self.fail("inlined call: closure not reached");
        }
        let ctl = self.f().ctl.pop().unwrap();
        for x in ctl.breaks.iter() {
            self.patch(*x);
        }
        let r = self.new_ref(rname.as_str(), false, rb);
        self.emit3(PH_REF, r, M_LOAD, 0);
        self.leave_block(s, saved);
        let ra1 = self.refs.len() as int;
        self.ksites[site as usize].ra1 = ra1;
        let sa1 = self.scopes.len() as int;
        self.ksites[site as usize].sa1 = sa1;
        self.emit(PH_KMID, site, 0);
        // the call
        self.goto(at);
        let e = self.primary();
        if self.error.is_empty() {
            self.call(e, false);
        }
        let rb1 = self.refs.len() as int;
        self.ksites[site as usize].rb1 = rb1;
        let sb1 = self.scopes.len() as int;
        self.ksites[site as usize].sb1 = sb1;
        self.emit(PH_KEND, site, 0);
        let end = ac + 1;
        self.goto(end);
        true
    }

    /// In a body `kcall_here` inlines: the closure parameter's call, the
    /// closure's body in a block of the call site's scope (the body's
    /// own names hidden from it).
    #[inline(never)]
    fn ksub_here(&mut self) -> bool {
        let top = self.ksubs.len() - 1;
        let at = self.pos;
        if !self.ksubs[top].active || at <= self.ksubs[top].bo || at >= self.ksubs[top].bc {
            return false;
        }
        let t = &self.toks[at as usize];
        if (t.kind as int) != T_IDENT || self.names[t.v as usize].as_str() != self.ksubs[top].name.as_str() {
            return false;
        }
        let a1 = at + 1;
        let a2 = at + 2;
        if !self.tok_is(a1, "(") || !self.tok_is(a2, ")") {
            return false;
        }
        let site = self.ksubs[top].site as usize;
        let body = self.ksubs[top].body;
        let body_end = self.ksubs[top].body_end;
        let outer = self.ksubs[top].scope;
        self.ksubs[top].active = false;
        self.ksubs[top].hits += 1;
        let x0 = self.refs.len() as int;
        let saved = self.cur_scope;
        let s = self.new_scope(outer, false, false);
        self.enter_block(s);
        self.emit(PH_HOIST, s, 0);
        self.goto(body);
        self.assign();
        if self.pos != body_end && self.error.is_empty() {
            self.fail("inlined call: bad closure body");
        }
        self.leave_block(s, saved);
        let x1 = self.refs.len() as int;
        self.ksites[site].ex.push(x0);
        self.ksites[site].ex.push(x1);
        self.ksubs[top].active = true;
        let end = at + 3;
        self.goto(end);
        true
    }

    /// Which code of each inlined call to keep (KSite `pick`): the
    /// inlined one when the name is the function's where the call is, it
    /// is as strict as the call's function, and the names its body uses
    /// mean the same there as where it is declared. The answer: the refs
    /// of the code not kept, two for each: the scopes made there (from
    /// and to), -1 for the others (none when there are no inlined calls).
    fn kpick(&mut self) -> Vec<int> {
        let mut dead: Vec<int> = Vec::new();
        if self.ksites.is_empty() {
            return dead;
        }
        let n = self.refs.len();
        let mut i: usize = 0;
        while i < n {
            dead.push(-1);
            dead.push(-1);
            i += 1;
        }
        let ns = self.ksites.len();
        let mut k: usize = 0;
        while k < ns {
            let ok = self.kpick_ok(k);
            self.ksites[k].pick = if ok { 1 } else { 0 };
            let a = if ok { self.ksites[k].ra1 } else { self.ksites[k].r0 };
            let b = if ok { self.ksites[k].rb1 } else { self.ksites[k].ra1 };
            let lo = if ok { self.ksites[k].sa1 } else { self.ksites[k].s0 };
            let hi = if ok { self.ksites[k].sb1 } else { self.ksites[k].sa1 };
            let mut r = a;
            while r < b {
                // an inlined call inside one not kept: the outer one's
                let d = (2 * r) as usize;
                if dead[d] < 0 {
                    dead[d] = lo;
                    dead[d + 1] = hi;
                }
                r += 1;
            }
            k += 1;
        }
        dead
    }

    fn kpick_ok(&self, k: usize) -> bool {
        let name = self.ksites[k].name.clone();
        let b = self.lookup_here(self.ksites[k].scope, name.as_str());
        if b < 0 || self.binds[b as usize].global || self.binds[b as usize].kind != K_FUNC {
            return false;
        }
        let ds = self.binds[b as usize].scope;
        let mut proto: int = -1;
        for h in self.scopes[ds as usize].hoists.iter() {
            if self.hoists[*h as usize].name.as_str() == name.as_str() {
                proto = self.hoists[*h as usize].proto;
            }
        }
        if proto < 0 {
            return false;
        }
        let pi = proto - self.proto_base;
        if pi < 0 || pi >= (self.protos.len() as int) || self.protos[pi as usize].strict != self.ksites[k].strict {
            return false;
        }
        let s0 = self.ksites[k].s0;
        let mut r = self.ksites[k].cb0;
        let cb1 = self.ksites[k].cb1;
        let ex = self.ksites[k].ex.clone();
        while r < cb1 {
            let mut skip = false;
            let mut e: usize = 0;
            while e + 1 < ex.len() {
                if r >= ex[e] && r < ex[e + 1] {
                    skip = true;
                }
                e += 2;
            }
            let ref_ = &self.refs[r as usize];
            if !skip && (ref_.direct as int) < 0 {
                let nm = self.names[ref_.nid as usize].as_str();
                let x = self.lookup_here((ref_.scope as int), nm);
                let inner = x >= 0 && self.binds[x as usize].scope >= s0;
                if !inner && x != self.lookup_here(ds, nm) {
                    return false;
                }
            }
            r += 1;
        }
        true
    }

    fn arrow(&mut self) -> Ex {
        let first = self.pos;
        let mut flags = F_ARROW;
        if self.is("async") && !self.peek_is(1, "=>") {
            self.next();
            flags |= F_ASYNC;
        }
        let p = self.function_rest(flags, "", first);
        self.emit(OP_CLOSURE, p, 0);
        ex(E_ANON)
    }

    fn method(&mut self, flags: int, name0: &str) -> int {
        // a private method's name is its `#name`
        let bare = priv_shown(name0);
        let name = bare.as_str();
        let full = if (flags & F_GETTER) != 0 && !name.is_empty() {
            format!("get {}", name)
        } else if (flags & F_SETTER) != 0 && !name.is_empty() {
            format!("set {}", name)
        } else {
            String::from(name)
        };
        let first = self.member_start;
        self.function_rest(flags | F_METHOD, full.as_str(), first)
    }

    /// Parameters and body; answers the proto.
    fn function_rest(&mut self, flags: int, name: &str, first: int) -> int {
        let arrow = (flags & F_ARROW) != 0;
        let mut proto = Proto::new(name);
        proto.arrow = arrow;
        let outer_strict = if self.fs.is_empty() { false } else { self.fs[self.fs.len() - 1].proto.strict };
        proto.strict = outer_strict || (flags & (F_STRICT | F_METHOD)) != 0;
        proto.method = (flags & F_METHOD) != 0;
        proto.class_ctor = (flags & F_CTOR) != 0;
        proto.derived = (flags & F_DERIVED) != 0;
        proto.getter_setter = (flags & (F_GETTER | F_SETTER)) != 0;
        proto.generator = (flags & F_GENERATOR) != 0;
        proto.is_async = (flags & F_ASYNC) != 0;
        let outer_parser_strict = self.strict;
        let fnid = self.push_fn(proto, false, 0);
        if !arrow {
            let mut cx: int = 0;
            if (flags & F_METHOD) != 0 {
                cx |= CX_SUPER_PROP;
            }
            if (flags & F_CTOR) != 0 && (flags & F_DERIVED) != 0 {
                cx |= CX_SUPER_CALL;
            }
            self.ctx = cx;
        }
        let parent = self.cur_scope;
        let fscope = self.new_scope(parent, true, arrow);
        self.scopes[fscope as usize].decl_fn = (flags & F_DECL) != 0;
        self.fns[fnid as usize].fscope = fscope;
        self.f().var_scope = fscope;
        self.cur_scope = fscope;
        if (flags & F_CTOR) != 0 && (flags & F_DERIVED) != 0 {
            // `this`, uninitialized until super(…) (never marked inited:
            // every read is checked)
            let tb = self.declare(fscope, "%this", K_LET);
            self.binds[tb as usize].tdz = true;
        }
        self.emit(PH_ENTER, fscope, 1);
        self.emit(PH_PROLOGUE, fscope, fnid);
        // an async function's parameters' errors reject its promise
        let async_only = (flags & F_ASYNC) != 0 && (flags & F_GENERATOR) == 0;
        if async_only {
            self.op(OP_GEN_START);
        }
        let saved_names = self.param_names.clone();
        self.param_names = Vec::new();
        let saved_pending = self.pending_inits.clone();
        self.pending_inits = Vec::new();
        // parameters: no yield / await expressions; a function's own
        // generator / async kind decides what `yield` / `await` name
        let saved_in_params = self.in_params;
        self.in_params = true;
        if !arrow {
            self.in_generator = (flags & F_GENERATOR) != 0;
            self.in_async = (flags & F_ASYNC) != 0;
        }
        let mut i: int = 0;
        let mut length: int = 0;
        let mut counting = true;
        let mut simple = true;
        let mut param_exprs = false;
        // TypeScript's parameter properties
        let mut props: Vec<String> = Vec::new();
        if arrow && !self.is("(") && !(self.ts && self.is("<")) {
            let nm = self.binding_ident();
            if self.error.is_empty() {
                let b = self.declare(fscope, nm.as_str(), K_PARAM);
                self.param_names.push(nm.clone());
                self.fns[fnid as usize].param_binds.push(b);
                self.mark_inited(b);
            }
            i = 1;
            length = 1;
        } else {
            self.skip_type_params();
            self.expect("(");
            while !self.is(")") && self.kind() != T_EOF && self.error.is_empty() {
                if self.ts && self.is("this") && self.peek_is(1, ":") {
                    // `this: T` only types `this`
                    self.next();
                    self.skip_annotation();
                    if !self.eat(",") {
                        break;
                    }
                    continue;
                }
                if self.ts && (self.is("public") || self.is("private") || self.is("protected") || self.is("readonly") || self.is("override")) && self.peek_kind(1) == T_IDENT && !self.peek_is(1, "(") {
                    // a parameter property: `this.x = x` as the body starts
                    if (flags & F_CTOR) == 0 {
                        self.ts_fail();
                        break;
                    }
                    while (self.is("public") || self.is("private") || self.is("protected") || self.is("readonly") || self.is("override")) && self.peek_kind(1) == T_IDENT {
                        self.next();
                    }
                    if self.kind() != T_IDENT {
                        self.ts_fail();
                        break;
                    }
                    let pn = self.text();
                    props.push(pn);
                }
                if self.eat("...") {
                    self.f().proto.rest = i;
                    counting = false;
                    simple = false;
                    if self.is("[") || self.is("{") {
                        param_exprs = true;
                    }
                    self.emit(OP_GET_LOCAL, i, 0);
                    self.pattern_target(K_PARAM);
                    self.skip_annotation();
                    self.mark_pending();
                    self.fns[fnid as usize].param_binds.push(-1);
                    i += 1;
                    break;
                }
                let plain = self.kind() == T_IDENT && (self.peek_is(1, ",") || self.peek_is(1, ")") || (self.ts && self.plain_typed_param()));
                if plain {
                    let nm = self.binding_ident();
                    if !self.error.is_empty() {
                        break;
                    }
                    self.skip_annotation();
                    let b = self.declare(fscope, nm.as_str(), K_PARAM);
                    self.param_names.push(nm.clone());
                    self.fns[fnid as usize].param_binds.push(b);
                    self.mark_inited(b);
                    if counting {
                        length += 1;
                    }
                } else {
                    simple = false;
                    param_exprs = true;
                    let ts = self.pos;
                    self.skip_target(K_PARAM);
                    self.skip_annotation();
                    let has_default = self.is("=");
                    self.goto(ts);
                    if has_default {
                        counting = false;
                    } else if counting {
                        length += 1;
                    }
                    self.emit(OP_GET_LOCAL, i, 0);
                    self.pattern_elem(K_PARAM);
                    self.mark_pending();
                    self.fns[fnid as usize].param_binds.push(-1);
                }
                i += 1;
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")");
            self.skip_return_type();
        }
        self.in_params = false;
        self.f().proto.nparams = i;
        self.f().proto.length = length;
        self.fns[fnid as usize].base = i;
        if (flags & F_SETTER) != 0 && (i != 1 || self.f().proto.rest >= 0) {
            self.fail("Setter must have exactly one formal parameter");
        }
        if (flags & F_GETTER) != 0 && i != 0 {
            self.fail("Getter must not have any formal parameters");
        }
        if (flags & F_GENERATOR) != 0 {
            self.op(OP_GEN_START);
        }
        if arrow {
            self.expect("=>");
            // a static block's `await` is reserved in an arrow's
            // parameters, not in its body
            if (self.ctx & CX_AWAIT) != 0 {
                self.ctx -= CX_AWAIT;
            }
        }
        self.in_function = true;
        self.in_generator = (flags & F_GENERATOR) != 0;
        self.in_async = (flags & F_ASYNC) != 0;
        let mut own_strict = false;
        if name == "__cerxesModuleBody" && !self.module_code {
            self.module_code = true;
            self.module_scope = fscope;
            self.module_depth = self.fs.len();
        }
        if arrow && !self.is("{") {
            let saved_no_in = self.no_in;
            self.assign();
            self.no_in = saved_no_in;
            self.op(OP_RETURN);
        } else {
            let body_end = self.mate_of(self.pos);
            self.expect("{");
            own_strict = self.directives();
            if own_strict {
                self.f().proto.strict = true;
            }
            // void_ret until a `return` with a value
            self.f().proto.void_ret = true;
            if i == 0 && (flags & (F_GENERATOR | F_ASYNC | F_CTOR | F_GETTER | F_SETTER | F_METHOD)) == 0 {
                self.fwd_check(body_end);
            }
            let mut bscope: int = -1;
            if param_exprs {
                bscope = self.new_scope(fscope, false, false);
                self.scopes[bscope as usize].params_scope = fscope;
                self.enter_block(bscope);
                self.emit(PH_BODY_VARS, bscope, fscope);
                self.f().var_scope = bscope;
            }
            let vs = self.f().var_scope;
            self.emit(PH_HOIST, vs, 0);
            if !props.is_empty() && (flags & F_DERIVED) == 0 {
                self.param_props(&props);
                props = Vec::new();
            }
            let (umark, uend) = self.using_open(body_end);
            while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
                self.list_stmt = true;
                self.using_here = true;
                let st = self.pos;
                self.statement();
                if !props.is_empty() && self.tok_is(st, "super") && self.tok_is(st + 1, "(") {
                    // in a derived class, after `super(…)`
                    self.param_props(&props);
                    props = Vec::new();
                }
            }
            self.using_close(umark, uend);
            if !props.is_empty() {
                self.ts_fail();
            }
            self.expect("}");
            self.ctor_this_check();
            self.op(OP_RETURN_UNDEF);
            if bscope >= 0 {
                self.leave_block(bscope, fscope);
            }
        }
        if !arrow && (flags & (F_DECL | F_METHOD)) == 0 && !name.is_empty() && !self.scopes[fscope as usize].names.contains_key(name) {
            self.declare(fscope, name, K_CALLEE);
        }
        self.end_annex();
        self.in_params = saved_in_params;
        self.check_params(simple, own_strict, flags, outer_parser_strict);
        if simple && !arrow && !self.f().proto.strict {
            self.fns[fnid as usize].mapped = true;
        }
        if own_strict && !outer_parser_strict && !arrow && (flags & (F_METHOD | F_GETTER | F_SETTER)) == 0 && is_strict_reserved(name) {
            self.fail("Unexpected eval or arguments in strict mode");
        }
        self.param_names = saved_names;
        self.pending_inits = saved_pending;
        let p = self.pop_fn();
        if self.error.is_empty() {
            self.set_span(p, first);
        }
        p
    }

    /// A body that is just `this.m.apply(this, arguments);`: Proto.fwd_atom
    /// is m's atom (the tokens from the body's first to its `}` at end).
    fn fwd_check(&mut self, end: int) {
        let p = self.pos;
        let semi = if self.tok_is(p + 10, ";") { 1 } else { 0 };
        if p + 10 + semi != end {
            return;
        }
        if !(self.tok_is(p, "this") && self.tok_is(p + 1, ".") && self.toks[(p + 2) as usize].kind as int == T_IDENT && self.tok_is(p + 3, ".") && self.tok_is(p + 4, "apply")) {
            return;
        }
        if !(self.tok_is(p + 5, "(") && self.tok_is(p + 6, "this") && self.tok_is(p + 7, ",") && self.tok_is(p + 8, "arguments") && self.tok_is(p + 9, ")")) {
            return;
        }
        let m = self.tok_text(p + 2);
        let a = self.atom(m.as_str());
        self.f().proto.fwd_atom = a;
    }

    /// Early errors of a parameter list.
    fn check_params(&mut self, simple: bool, own_strict: bool, flags: int, outer_strict: bool) {
        if !simple && own_strict {
            self.fail("Illegal 'use strict' directive in function with non-simple parameter list");
            return;
        }
        let strict_here = own_strict || outer_strict;
        let unique = !simple || (flags & (F_ARROW | F_METHOD)) != 0 || strict_here;
        if !unique {
            return;
        }
        let names = self.param_names.clone();
        if strict_here {
            for nm in names.iter() {
                if is_strict_reserved(nm.as_str()) {
                    self.fail("Unexpected eval or arguments in strict mode");
                    return;
                }
            }
        }
        let mut i: usize = 0;
        while i < names.len() {
            let mut j = i + 1;
            while j < names.len() {
                if names[i] == names[j] {
                    self.fail("Duplicate parameter name not allowed in this context");
                    return;
                }
                j += 1;
            }
            i += 1;
        }
    }

    /// The field initializer function `id` (-1: a new one) on top again.
    fn resume_member_fn(&mut self, id: int, class_scope: int) -> int {
        if id < 0 {
            let mut proto = Proto::new("");
            proto.strict = true;
            proto.method = true;
            let fnid = self.push_fn(proto, false, 0);
            let fscope = self.new_scope(class_scope, true, false);
            self.fns[fnid as usize].fscope = fscope;
            self.f().var_scope = fscope;
            self.cur_scope = fscope;
            self.emit(PH_ENTER, fscope, 1);
            self.emit(PH_PROLOGUE, fscope, fnid);
            self.ctx = CX_SUPER_PROP | CX_NO_ARGS;
            self.park_count += 1;
            return self.park_count;
        }
        let mut j: usize = 0;
        while j < self.parked_ids.len() {
            if self.parked_ids[j] == id {
                break;
            }
            j += 1;
        }
        let st = self.parked.remove(j);
        self.parked_ids.remove(j);
        let fnid = st.fnid;
        self.fs.push(st);
        self.cur_scope = self.fns[fnid as usize].fscope;
        self.ctx = CX_SUPER_PROP | CX_NO_ARGS;
        id
    }

    fn park_member_fn(&mut self, id: int) {
        let st = self.fs.pop().unwrap();
        self.cur_scope = st.saved_scope;
        self.in_function = st.saved_in_function;
        self.in_generator = st.saved_in_generator;
        self.in_async = st.saved_in_async;
        self.strict = st.saved_strict;
        self.ctx = st.saved_ctx;
        self.parked.push(st);
        self.parked_ids.push(id);
    }

    /// A class static block at its `{`: its own var scope, no `return`.
    fn static_block(&mut self) {
        let saved = self.cur_scope;
        let saved_vs = self.f().var_scope;
        let saved_in_fn = self.in_function;
        let s = self.new_scope(saved, false, false);
        self.enter_block(s);
        self.f().var_scope = s;
        self.emit(PH_HOIST, s, 0);
        self.expect("{");
        let saved_depth = self.static_depth;
        self.static_depth = self.fs.len() as int;
        while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
            self.list_stmt = true;
            self.statement();
        }
        self.static_depth = saved_depth;
        self.expect("}");
        self.leave_block(s, saved);
        self.f().var_scope = saved_vs;
        self.in_function = saved_in_fn;
    }

    fn finish_member_fn(&mut self, id: int, class_scope: int) -> int {
        self.resume_member_fn(id, class_scope);
        self.op(OP_RETURN_UNDEF);
        self.pop_fn()
    }

    fn class_expr(&mut self, decl: bool) -> Ex {
        let saved_strict = self.strict;
        self.strict = true;
        let e = self.class_body_expr(decl);
        self.strict = saved_strict;
        e
    }

    fn class_body_expr(&mut self, decl: bool) -> Ex {
        let first = self.pos;
        self.next();
        let mut name = String::new();
        if self.kind() == T_IDENT && !self.is("extends") && !self.is("{") && !(self.ts && self.is("implements")) {
            name = self.binding_ident();
        } else if decl {
            self.fail("class name expected");
            return ex(E_OTHER);
        }
        self.skip_type_params();
        let derived = self.eat("extends");
        if derived {
            self.lhs();
            if self.ts && self.is("<") {
                let a = self.type_args_end(self.pos);
                if a < 0 {
                    self.ts_fail();
                } else {
                    self.goto(a);
                }
            }
        }
        if self.ts && self.eat("implements") {
            loop {
                self.skip_type_here();
                if !self.eat(",") {
                    break;
                }
            }
        }
        let saved = self.cur_scope;
        let mut inner: int = -1;
        let mut inner_b: int = -1;
        // the class's scope: its name, its computed field keys
        inner = self.new_scope(saved, false, false);
        if !name.is_empty() {
            inner_b = self.declare(inner, name.as_str(), K_CONST);
        }
        self.enter_block(inner);
        let class_scope = self.cur_scope;
        let saved_in_class = self.in_class;
        self.in_class = true;
        self.priv_decl.push(Vec::new());
        self.priv_refs.push(Vec::new());
        let (pnames, pkinds) = self.priv_scan(self.pos);
        let cid = self.priv_next;
        self.priv_next += 1;
        let mut branded = false;
        for k in pkinds.iter() {
            if *k == PK_METHOD || *k == PK_ACCESSOR {
                branded = true;
            }
        }
        self.priv_cls.push(cid);
        self.priv_nm.push(pnames);
        self.priv_kd.push(pkinds);
        // instances of a class with private methods carry its brand: its
        // prototype, where those methods are, under a hidden key
        let home = format!("%home{}", cid);
        let mut home_b: int = -1;
        if branded {
            home_b = self.declare(class_scope, home.as_str(), K_LET);
        }
        let c_at = self.emit(OP_CLOSURE, -1, 0);
        if derived {
            self.op(OP_SWAP);
        }
        self.emit(OP_CLASS, if derived { 1 } else { 0 }, 0);
        self.expect("{");
        let mut ctor: int = -1;
        let mut inst_id: int = -1;
        let mut stat_id: int = -1;
        if branded {
            self.op(OP_DUP);
            self.emit_bind_init(home_b);
            self.op(OP_POP);
            self.mark_inited(home_b);
            inst_id = self.resume_member_fn(inst_id, class_scope);
            self.in_function = true;
            self.op(OP_THIS);
            self.emit_ref(home.as_str(), M_LOAD, false);
            let bt = self.atom(format!("\u{1}brand\u{2}{}", cid).as_str());
            self.emit(OP_DEFINE_FIELD, bt, 1);
            self.op(OP_POP);
            self.park_member_fn(inst_id);
        }
        while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
            if self.eat(";") {
                continue;
            }
            if self.ts && self.ts_member_skipped() {
                continue;
            }
            let mut flags = 0;
            if self.is("static") && !self.peek_is(1, "(") && !self.peek_is(1, "=") {
                self.next();
                flags |= F_STATIC;
                if self.ts && self.ts_member_skipped() {
                    continue;
                }
                if self.is("{") {
                    stat_id = self.resume_member_fn(stat_id, class_scope);
                    self.in_function = true;
                    self.in_generator = false;
                    self.in_async = false;
                    self.ctx = CX_SUPER_PROP | CX_NO_ARGS | CX_AWAIT;
                    self.static_block();
                    self.park_member_fn(stat_id);
                    continue;
                }
            }
            let mut fflags = 0;
            let ms = self.pos;
            if self.is("async") && !self.peek_is(1, "(") && !self.peek_is(1, "=") && !self.tok_nl(self.pos + 1) {
                self.next();
                fflags |= F_ASYNC;
            }
            if self.eat("*") {
                fflags |= F_GENERATOR;
            }
            if (self.is("get") || self.is("set")) && !self.peek_is(1, "(") && !self.peek_is(1, "=") && !self.peek_is(1, ";") && !self.peek_is(1, "}") {
                if self.is("get") {
                    flags |= F_GETTER;
                } else {
                    flags |= F_SETTER;
                }
                self.next();
            }
            let kpos = self.pos;
            let computed = self.is("[");
            let mut kend = if computed { self.after_mate(kpos) } else { kpos + 1 };
            if self.ts && self.tok_is(kend, "?") && (self.tok_is(kend + 1, "(") || self.tok_is(kend + 1, "<")) {
                // `m?(): T;` declares only; with a body the parser reads it
                let e = self.sig_rest_end(kend + 1);
                if e >= 0 && !self.tok_is(e, "{") {
                    self.goto(e);
                    self.semicolon();
                    continue;
                }
                self.ts_fail();
                return ex(E_OTHER);
            }
            if self.ts && kend >= 0 && (self.tok_is(kend, "<") || self.tok_is(kend, "(")) {
                // an overload's signature (no body) is skipped
                let e = self.sig_rest_end(kend);
                if e >= 0 && !self.tok_is(e, "{") {
                    self.goto(e);
                    self.semicolon();
                    continue;
                }
                if self.tok_is(kend, "<") {
                    kend = self.type_params_end(kend);
                }
            }
            let is_method = self.tok_is(kend, "(") || (self.ts && self.tok_is(kend, "<"));
            if is_method {
                let kk = self.kind();
                let is_ctor = !computed && (flags & F_STATIC) == 0 && (kk == T_IDENT || kk == T_STR) && self.text().as_str() == "constructor";
                if is_ctor {
                    if (fflags & (F_GENERATOR | F_ASYNC)) != 0 || (flags & (F_GETTER | F_SETTER)) != 0 {
                        self.fail("Class constructor may not be a generator, an async method or an accessor");
                        return ex(E_OTHER);
                    }
                    if ctor >= 0 {
                        self.fail("A class may only have one constructor");
                        return ex(E_OTHER);
                    }
                    self.next();
                    let mut mf = fflags | F_CTOR | F_METHOD;
                    if derived {
                        mf |= F_DERIVED;
                    }
                    let p = self.function_rest(mf, name.as_str(), ms);
                    self.set_op(c_at, OP_CLOSURE, p, 0);
                    ctor = p;
                    continue;
                }
                if (flags & F_STATIC) != 0 && !computed && (kk == T_IDENT || kk == T_STR) && self.text().as_str() == "prototype" {
                    self.fail("Classes may not have a static property named 'prototype'");
                    return ex(E_OTHER);
                }
                if (flags & F_STATIC) != 0 {
                    self.op(OP_OVER);
                } else {
                    self.op(OP_DUP);
                }
                let mut kf: int = 0;
                let kname = self.prop_key(&mut kf);
                if kf == 2 {
                    let pk = if (flags & F_GETTER) != 0 { "g" } else if (flags & F_SETTER) != 0 { "s" } else { "o" };
                    let raw = priv_raw(kname.as_str());
                    self.priv_declare(raw.as_str(), pk, (flags & F_STATIC) != 0);
                    if raw.as_str() == "\u{1}#constructor" {
                        self.fail("Classes may not have a private field named '#constructor'");
                        return ex(E_OTHER);
                    }
                }
                self.member_start = ms;
                let mf = fflags | (flags & (F_GETTER | F_SETTER | F_STATIC));
                let p = self.method(mf, kname.as_str());
                self.emit(OP_CLOSURE, p, 0);
                let at = if computed { 0 } else { self.atom(kname.as_str()) };
                if (flags & F_GETTER) != 0 {
                    self.emit(if computed { OP_INIT_GETTER_ELEM } else { OP_INIT_GETTER }, at, 1);
                } else if (flags & F_SETTER) != 0 {
                    self.emit(if computed { OP_INIT_SETTER_ELEM } else { OP_INIT_SETTER }, at, 1);
                } else {
                    self.emit(if computed { OP_DEFINE_METHOD_ELEM } else { OP_DEFINE_METHOD }, at, 0);
                }
                self.op(OP_POP);
                continue;
            }
            // a field: `this.key = init` in the initializer function
            let st = (flags & F_STATIC) != 0;
            if !computed && (self.kind() == T_IDENT || self.kind() == T_STR) && (self.text().as_str() == "constructor" || (st && self.text().as_str() == "prototype")) {
                self.fail("Classes may not have a field named 'constructor' or a static field named 'prototype'");
                return ex(E_OTHER);
            }
            if self.kind() == T_PRIVATE && self.text().as_str() == "constructor" {
                self.fail("Classes may not have a private field named '#constructor'");
                return ex(E_OTHER);
            }
            // a computed key is read once, as the class is defined
            let mut fk = String::new();
            if computed {
                self.next();
                self.assign();
                self.expect("]");
                self.op(OP_TO_KEY);
                self.fk_count += 1;
                fk = format!("%fieldkey{}", self.fk_count);
                let kb = self.declare(class_scope, fk.as_str(), K_LET);
                self.emit_bind_init(kb);
                self.op(OP_POP);
                self.mark_inited(kb);
            }
            let id = self.resume_member_fn(if st { stat_id } else { inst_id }, class_scope);
            if st {
                stat_id = id;
            } else {
                inst_id = id;
            }
            self.in_function = true;
            self.op(OP_THIS);
            let mut kf: int = 0;
            let kname = if computed {
                self.emit_ref(fk.as_str(), M_LOAD, false);
                String::new()
            } else {
                self.prop_key(&mut kf)
            };
            if kf == 2 {
                let raw = priv_raw(kname.as_str());
                self.priv_declare(raw.as_str(), "o", st);
            }
            self.skip_annotation();
            if self.eat("=") {
                let e = self.assign();
                if !computed {
                    let nm = priv_shown(kname.as_str());
                    self.name_anonymous(e, nm.as_str());
                }
            } else {
                self.op(OP_UNDEF);
            }
            if computed {
                self.op(OP_DEFINE_FIELD_ELEM);
            } else {
                let at = self.atom(kname.as_str());
                self.emit(OP_DEFINE_FIELD, at, if kf == 2 { 1 } else { 0 });
            }
            self.op(OP_POP);
            self.park_member_fn(id);
            self.semicolon();
        }
        self.expect("}");
        self.in_class = saved_in_class;
        self.priv_cls.pop();
        self.priv_nm.pop();
        self.priv_kd.pop();
        if !self.error.is_empty() {
            return ex(E_OTHER);
        }
        self.priv_end();
        if !self.error.is_empty() {
            return ex(E_OTHER);
        }
        if ctor < 0 {
            let src = if derived { "(...args) { super(...args); }" } else { "() {}" };
            let saved_tok = self.push_tokens(src);
            let mut mf = F_CTOR | F_STRICT | F_METHOD;
            if derived {
                mf |= F_DERIVED;
            }
            let p = self.function_rest(mf, name.as_str(), -1);
            self.pop_tokens(&saved_tok);
            if derived {
                // `new` of the class may enter its parent (Vm::ctor_target)
                let pi = (p - self.proto_base) as usize;
                self.protos[pi].fwd_atom = -2;
            }
            self.set_op(c_at, OP_CLOSURE, p, 0);
            ctor = p;
        }
        // the class's text is its constructor's
        self.set_span(ctor, first);
        if inner_b >= 0 {
            self.op(OP_OVER);
            self.emit_bind_init(inner_b);
            self.op(OP_POP);
        }
        if inst_id >= 0 {
            self.op(OP_OVER);
            let pf = self.finish_member_fn(inst_id, class_scope);
            self.emit(OP_CLOSURE, pf, 0);
            self.op(OP_SET_FIELDS);
            self.op(OP_POP);
        }
        self.op(OP_POP);
        if stat_id >= 0 {
            self.op(OP_DUP);
            let ps = self.finish_member_fn(stat_id, class_scope);
            self.emit(OP_CLOSURE, ps, 0);
            self.emit(OP_SET_FIELDS, 1, 0);
            self.op(OP_OVER);
            self.emit(OP_CALL, 0, 0);
            self.op(OP_POP);
        }
        if inner >= 0 {
            self.leave_block(inner, saved);
            if inner_b >= 0 {
                self.mark_inited(inner_b);
            }
        }
        if name.is_empty() {
            ex(E_ANON)
        } else {
            ex(E_OTHER)
        }
    }
}

impl PComp {
    // =====================================================================
    // TypeScript's types, read only to be skipped: on the tokens, without
    // emitting anything. Each `*_end` answers the token after what it
    // read, or -1 when it is not one pcomp reads (the script then goes to
    // the parser, crate::parser::script_js).

    fn t_kind(&self) -> int {
        if self.tp >= self.tok_count() {
            return T_EOF;
        }
        self.toks[self.tp as usize].kind as int
    }

    fn t_is(&self, s: &str) -> bool {
        self.tg == 0 && self.tok_is(self.tp, s)
    }

    fn t_peek_is(&self, k: int, s: &str) -> bool {
        self.tg == 0 && self.tok_is(self.tp + k, s)
    }

    fn t_peek_kind(&self, k: int) -> int {
        let i = self.tp + k;
        if i >= self.tok_count() {
            return T_EOF;
        }
        self.toks[i as usize].kind as int
    }

    fn t_nl(&self) -> bool {
        self.tok_nl(self.tp)
    }

    fn t_next(&mut self) {
        if self.tg != 0 {
            self.tbad = true;
        }
        if self.tp < self.tok_count() - 1 {
            self.tp += 1;
        }
    }

    fn t_eat(&mut self, s: &str) -> bool {
        if self.t_is(s) {
            self.t_next();
            return true;
        }
        false
    }

    fn t_expect(&mut self, s: &str) {
        if !self.t_eat(s) {
            self.tbad = true;
        }
    }

    /// Past the bracket at the reader's token and its mate.
    fn t_skip_balanced(&mut self) {
        let a = self.after_mate(self.tp);
        if a < 0 || self.tg != 0 {
            self.tbad = true;
            return;
        }
        self.tp = a;
    }

    /// A closing `>`, one of a `>>` / `>>>` too.
    fn t_eat_gt(&mut self) {
        if self.tbad || self.tp >= self.tok_count() || (self.toks[self.tp as usize].kind as int) != T_PUNCT {
            self.tbad = true;
            return;
        }
        let t = self.tok_text(self.tp);
        let n = t.as_bytes().len() as int;
        if self.tg >= n || t.as_bytes()[self.tg as usize] != 62 {
            self.tbad = true;
            return;
        }
        self.tg += 1;
        if self.tg == n {
            self.tg = 0;
            self.tp += 1;
        }
    }

    fn t_ident_name(&mut self) {
        if self.t_kind() != T_IDENT || self.tg != 0 {
            self.tbad = true;
            return;
        }
        self.t_next();
    }

    /// `<T, U extends X = Y>`
    fn t_type_params(&mut self) {
        self.t_expect("<");
        while !self.tbad && !self.t_is(">") && self.t_kind() != T_EOF {
            while (self.t_is("const") || self.t_is("in") || self.t_is("out")) && self.t_peek_kind(1) == T_IDENT {
                self.t_next();
            }
            self.t_ident_name();
            if self.t_eat("extends") {
                self.t_type(true);
            }
            if self.t_eat("=") {
                self.t_type(true);
            }
            if !self.t_eat(",") {
                break;
            }
        }
        self.t_eat_gt();
    }

    /// `<A, B>`
    fn t_type_args(&mut self) {
        self.t_expect("<");
        while !self.tbad && !self.t_is(">") && self.t_kind() != T_EOF {
            self.t_type(true);
            if !self.t_eat(",") {
                break;
            }
        }
        self.t_eat_gt();
    }

    /// A return type: a type, or `x is T` / `asserts x is T`.
    fn t_return_type(&mut self) {
        if self.t_is("asserts") && self.t_peek_kind(1) == T_IDENT && !self.tok_nl(self.tp + 1) {
            self.t_next();
            self.t_next();
            if self.t_eat("is") {
                self.t_type(true);
            }
            return;
        }
        if self.t_kind() == T_IDENT && self.t_peek_is(1, "is") && !self.tok_nl(self.tp + 1) {
            self.t_next();
            self.t_next();
            self.t_type(true);
            return;
        }
        self.t_type(true);
    }

    fn t_type(&mut self, cond: bool) {
        if self.tbad {
            return;
        }
        if self.t_is("<") {
            self.t_type_params();
            self.t_fn_type_rest();
            return;
        }
        if self.t_is("abstract") && self.t_peek_is(1, "new") {
            self.t_next();
        }
        if self.t_is("new") {
            self.t_next();
            if self.t_is("<") {
                self.t_type_params();
            }
            self.t_fn_type_rest();
            return;
        }
        self.t_eat("|");
        self.t_intersection();
        while !self.tbad && self.t_is("|") {
            self.t_next();
            self.t_intersection();
        }
        if cond && self.t_is("extends") && !self.t_nl() {
            self.t_next();
            self.t_type(false);
            self.t_expect("?");
            self.t_type(true);
            self.t_expect(":");
            self.t_type(true);
        }
    }

    fn t_fn_type_rest(&mut self) {
        if !self.t_is("(") {
            self.tbad = true;
            return;
        }
        self.t_skip_balanced();
        self.t_expect("=>");
        self.t_return_type();
    }

    fn t_intersection(&mut self) {
        self.t_eat("&");
        self.t_operator();
        while !self.tbad && self.t_is("&") {
            self.t_next();
            self.t_operator();
        }
    }

    fn t_starts(&self, k: int) -> bool {
        let kd = self.t_peek_kind(k);
        if kd == T_IDENT || kd == T_STR || kd == T_NUM || kd == T_TEMPLATE {
            return true;
        }
        self.t_peek_is(k, "(") || self.t_peek_is(k, "[") || self.t_peek_is(k, "{") || self.t_peek_is(k, "-") || self.t_peek_is(k, "<")
    }

    fn t_operator(&mut self) {
        if (self.t_is("keyof") || self.t_is("unique") || self.t_is("readonly")) && self.t_starts(1) {
            self.t_next();
            self.t_operator();
            return;
        }
        if self.t_is("infer") && self.t_peek_kind(1) == T_IDENT {
            self.t_next();
            self.t_next();
            if self.t_is("extends") && !self.t_nl() {
                // `infer U extends X` inside a conditional's check
                self.t_next();
                self.t_type(false);
            }
            return;
        }
        self.t_primary();
        while !self.tbad && self.t_is("[") && !self.t_nl() {
            self.t_next();
            if !self.t_is("]") {
                self.t_type(true);
            }
            self.t_expect("]");
        }
    }

    fn t_primary(&mut self) {
        if self.tbad {
            return;
        }
        let k = self.t_kind();
        if self.t_is("(") {
            self.t_skip_balanced();
            if self.t_is("=>") {
                self.t_next();
                self.t_return_type();
            }
            return;
        }
        if self.t_is("{") || self.t_is("[") {
            self.t_skip_balanced();
            return;
        }
        if self.t_is("-") {
            self.t_next();
            if self.t_kind() != T_NUM {
                self.tbad = true;
                return;
            }
            self.t_next();
            return;
        }
        if (k == T_STR || k == T_NUM || k == T_TEMPLATE) && self.tg == 0 {
            self.t_next();
            return;
        }
        if self.t_is("typeof") {
            self.t_next();
            if self.t_is("import") {
                self.t_import_type();
            } else {
                self.t_entity();
            }
            if self.t_is("<") && !self.t_nl() {
                self.t_type_args();
            }
            return;
        }
        if self.t_is("import") {
            self.t_import_type();
            return;
        }
        if k == T_IDENT && self.tg == 0 {
            self.t_entity();
            if self.t_is("<") && !self.t_nl() {
                self.t_type_args();
            }
            return;
        }
        self.tbad = true;
    }

    fn t_entity(&mut self) {
        self.t_ident_name();
        while !self.tbad && self.t_is(".") {
            self.t_next();
            self.t_ident_name();
        }
    }

    fn t_import_type(&mut self) {
        self.t_next();
        if !self.t_is("(") {
            self.tbad = true;
            return;
        }
        self.t_skip_balanced();
        while !self.tbad && self.t_eat(".") {
            self.t_ident_name();
        }
        if self.t_is("<") && !self.t_nl() {
            self.t_type_args();
        }
    }

    fn t_start(&mut self, i: int) {
        self.tp = i;
        self.tg = 0;
        self.tbad = false;
    }

    fn t_done(&self) -> int {
        if self.tbad || self.tg != 0 {
            return -1;
        }
        self.tp
    }

    /// After the type at token `i`.
    fn type_end(&mut self, i: int) -> int {
        self.t_start(i);
        self.t_type(true);
        self.t_done()
    }

    /// After the return type at token `i`.
    fn return_type_end(&mut self, i: int) -> int {
        self.t_start(i);
        self.t_return_type();
        self.t_done()
    }

    /// After the type parameters `<…>` at token `i`.
    fn type_params_end(&mut self, i: int) -> int {
        self.t_start(i);
        self.t_type_params();
        self.t_done()
    }

    /// After the type arguments `<…>` at token `i`.
    fn type_args_end(&mut self, i: int) -> int {
        self.t_start(i);
        self.t_type_args();
        self.t_done()
    }

    /// TypeScript's declarations at a statement: the erased ones skipped,
    /// `abstract class` read as a class; true when it read the statement.
    /// What has a runtime meaning (`enum`, `namespace`, `import`,
    /// `export`) is the parser's.
    fn ts_statement(&mut self, in_list: bool) -> bool {
        let w = self.text();
        let ws = w.as_str();
        let next_ident = self.peek_kind(1) == T_IDENT && !self.tok_nl(self.pos + 1);
        if ws == "type" && next_ident && (self.peek_is(2, "=") || self.peek_is(2, "<")) {
            self.next();
            self.next();
            self.skip_type_params();
            self.expect("=");
            self.skip_type_here();
            self.semicolon();
            return true;
        }
        if ws == "interface" && next_ident {
            self.skip_to_body();
            return true;
        }
        if ws == "declare" && next_ident {
            self.next();
            if self.is("var") || self.is("let") || self.is("const") {
                self.next();
                loop {
                    self.skip_binding_target();
                    self.skip_annotation();
                    if !self.eat(",") {
                        break;
                    }
                }
                self.semicolon();
                return true;
            }
            if self.is("function") || (self.is("async") && self.peek_is(1, "function")) {
                let e = self.fn_signature_end(self.pos);
                if e < 0 {
                    self.ts_fail();
                    return true;
                }
                self.goto(e);
                self.semicolon();
                return true;
            }
            if self.is("type") {
                return self.ts_statement(in_list);
            }
            if self.is("module") && self.peek_kind(1) == T_STR && !self.peek_is(2, "{") {
                self.next();
                self.next();
                self.semicolon();
                return true;
            }
            // class, abstract class, interface, enum, namespace, module, global
            self.skip_to_body();
            return true;
        }
        if ws == "abstract" && self.peek_is(1, "class") && !self.tok_nl(self.pos + 1) {
            self.next();
            self.class_decl();
            return true;
        }
        if ws == "function" || (ws == "async" && self.peek_is(1, "function") && !self.tok_nl(self.pos + 1)) {
            // an overload's signature, without a body
            let e = self.fn_signature_end(self.pos);
            if e >= 0 && !self.tok_is(e, "{") {
                self.goto(e);
                self.semicolon();
                return true;
            }
            return false;
        }
        if ws == "import" && self.type_only_import() {
            self.skip_module_clause();
            return true;
        }
        if ws == "export" && ((self.peek_is(1, "type") && (self.peek_is(2, "{") || self.peek_is(2, "*"))) || self.peek_is(1, "interface") || self.peek_is(1, "declare") || (self.peek_is(1, "type") && self.peek_kind(2) == T_IDENT && (self.peek_is(3, "=") || self.peek_is(3, "<")))) {
            if self.peek_is(2, "{") || self.peek_is(2, "*") {
                self.skip_module_clause();
                return true;
            }
            self.next();
            if self.ts_statement(in_list) {
                return true;
            }
            self.ts_fail();
            return true;
        }
        if (ws == "enum" && next_ident) || (ws == "const" && self.peek_is(1, "enum")) {
            if ws == "const" {
                self.next();
            }
            self.enum_decl();
            return true;
        }
        if ws == "import" && !self.peek_is(1, "(") && !self.peek_is(1, ".") {
            self.script_import();
            return true;
        }
        if ws == "export" {
            if !self.module_path.is_empty() {
                self.module_export(in_list);
                return true;
            }
            self.script_export(in_list);
            return true;
        }
        if (ws == "namespace" || ws == "module") && next_ident && (self.peek_is(2, "{") || self.peek_is(2, ".")) {
            self.namespace_decl();
            return true;
        }
        if ((ws == "namespace" || ws == "module") && next_ident) || ws == "import" || ws == "export" {
            self.ts_fail();
            return true;
        }
        let _ = in_list;
        false
    }

    /// `import type …`, or `import { type A, type B } from …`: only types.
    fn type_only_import(&self) -> bool {
        if self.peek_is(1, "type") && (self.peek_kind(2) == T_IDENT || self.peek_is(2, "{") || self.peek_is(2, "*")) && !self.peek_is(2, "from") {
            return true;
        }
        if !self.peek_is(1, "{") {
            return false;
        }
        let close = self.mate_of(self.pos + 1);
        let mut k = self.pos + 2;
        let mut any = false;
        while k < close {
            // each entry starts with `type`
            let k1 = k + 1;
            let named = (self.toks[k1 as usize].kind as int) == T_IDENT && !self.tok_is(k1, "as");
            if !(self.tok_is(k, "type") && named) {
                return false;
            }
            any = true;
            while k < close && !self.tok_is(k, ",") {
                k += 1;
            }
            k += 1;
        }
        any
    }

    /// Past an import's or an export's clause, to its `;`.
    fn skip_module_clause(&mut self) {
        self.next();
        loop {
            if self.kind() == T_EOF {
                return;
            }
            if self.is(";") {
                self.next();
                return;
            }
            if self.is("{") {
                let a = self.after_mate(self.pos);
                if a < 0 {
                    self.ts_fail();
                    return;
                }
                self.goto(a);
                if !self.is("from") {
                    self.semicolon();
                    return;
                }
                continue;
            }
            if self.is("from") || self.kind() == T_STR {
                if self.is("from") {
                    self.next();
                }
                if self.kind() == T_STR {
                    self.next();
                }
                if (self.is("with") || self.is("assert")) && self.peek_is(1, "{") {
                    self.next();
                    let a = self.after_mate(self.pos);
                    self.goto(a);
                }
                self.semicolon();
                return;
            }
            self.next();
        }
    }

    /// Past the `{ … }` that ends a declaration (`interface I extends A {`).
    fn skip_to_body(&mut self) {
        let mut k = self.pos;
        while k < self.tok_count() && !self.tok_is(k, "{") && (self.toks[k as usize].kind as int) != T_EOF {
            if self.tok_is(k, "(") || self.tok_is(k, "[") {
                let a = self.after_mate(k);
                if a < 0 {
                    break;
                }
                k = a;
                continue;
            }
            k += 1;
        }
        let a = self.after_mate(k);
        if !self.tok_is(k, "{") || a < 0 {
            self.ts_fail();
            return;
        }
        self.goto(a);
    }

    /// After `function name<T>(…): R` at token `i` (`async` and `*` too),
    /// or -1.
    fn fn_signature_end(&mut self, i: int) -> int {
        let mut k = i;
        if self.tok_is(k, "async") {
            k += 1;
        }
        if !self.tok_is(k, "function") {
            return -1;
        }
        k += 1;
        if self.tok_is(k, "*") {
            k += 1;
        }
        if k < self.tok_count() && (self.toks[k as usize].kind as int) == T_IDENT && !self.tok_is(k, "(") {
            k += 1;
        }
        self.sig_rest_end(k)
    }

    /// After `<T>(…): R` at token `k` (a method's or a function's), or -1.
    fn sig_rest_end(&mut self, i: int) -> int {
        let mut k = i;
        if self.tok_is(k, "<") {
            k = self.type_params_end(k);
            if k < 0 {
                return -1;
            }
        }
        if !self.tok_is(k, "(") {
            return -1;
        }
        k = self.after_mate(k);
        if k < 0 {
            return -1;
        }
        if self.tok_is(k, ":") {
            k = self.return_type_end(k + 1);
        }
        k
    }

    /// TypeScript: a class member's modifiers (`public`, `readonly` …)
    /// skipped; true when the whole member is one without code
    /// (`declare x: T`, `abstract m(): T`, `[key: string]: T`).
    fn ts_member_skipped(&mut self) -> bool {
        loop {
            let m = self.is("public") || self.is("private") || self.is("protected") || self.is("readonly") || self.is("override") || self.is("declare") || self.is("abstract") || self.is("accessor");
            if !m {
                break;
            }
            // the word is a member's name: `readonly() {}`, `public = 1`
            if self.peek_is(1, "(") || self.peek_is(1, "=") || self.peek_is(1, ";") || self.peek_is(1, "}") || self.peek_is(1, ":") || self.peek_is(1, "?") || self.peek_is(1, "!") || self.peek_is(1, "<") || self.tok_nl(self.pos + 1) {
                break;
            }
            if self.is("accessor") {
                self.ts_fail();
                return true;
            }
            if self.is("declare") || self.is("abstract") {
                // no code: up to the member's end
                self.next();
                self.skip_member_sig();
                return true;
            }
            self.next();
        }
        if self.is("[") && self.peek_kind(1) == T_IDENT && self.peek_is(2, ":") {
            // an index signature
            self.skip_member_sig();
            return true;
        }
        if self.is("static") && self.peek_is(1, "[") && self.peek_kind(2) == T_IDENT && self.peek_is(3, ":") {
            self.next();
            self.skip_member_sig();
            return true;
        }
        false
    }

    /// Past a member that has no code: its name, `?`, type or signature,
    /// and the `;`.
    fn skip_member_sig(&mut self) {
        while self.is("static") || self.is("readonly") || self.is("public") || self.is("private") || self.is("protected") || self.is("override") || self.is("abstract") || self.is("declare") || self.is("get") || self.is("set") || self.is("async") || self.is("*") {
            if self.peek_is(1, "(") || self.peek_is(1, ":") || self.peek_is(1, "?") || self.peek_is(1, ";") || self.peek_is(1, "<") {
                break;
            }
            self.next();
        }
        if self.is("[") {
            let a = self.after_mate(self.pos);
            if a < 0 {
                self.ts_fail();
                return;
            }
            self.goto(a);
        } else {
            self.next();
        }
        if self.is("?") || self.is("!") {
            self.next();
        }
        if self.is("<") || self.is("(") {
            let e = self.sig_rest_end(self.pos);
            if e < 0 {
                self.ts_fail();
                return;
            }
            self.goto(e);
        } else if self.is(":") {
            self.next();
            self.skip_type_here();
        }
        if self.is("=") {
            // `declare x = 1` is an error; an abstract field has no value
            self.ts_fail();
            return;
        }
        self.semicolon();
    }

    /// `this.x = x` for each parameter property, as statements.
    fn param_props(&mut self, names: &Vec<String>) {
        let mut src = String::new();
        for n in names.iter() {
            src.push_str(format!("this.{} = {};\n", n, n).as_str());
        }
        let saved = self.push_tokens(src.as_str());
        while self.kind() != T_EOF && self.error.is_empty() {
            self.list_stmt = true;
            self.statement();
        }
        self.pop_tokens(&saved);
    }

    /// The source's text from token `a` to token `b` (both in).
    fn src_text(&self, a: int, b: int) -> String {
        let from = self.toks[a as usize].start as usize;
        let to = self.toks[b as usize].end as usize;
        self.src_rc.chars().skip(from).take(to - from).collect::<String>()
    }

    /// `enum E { A, B = 5, C = 'c' }` as TypeScript writes it:
    ///   var E; (function (E) { E[E["A"] = 0] = "A"; var A = E["A"]; … })(E || (E = {}));
    /// (each member's name stands for it in the later initializers).
    fn enum_decl(&mut self) {
        self.next();
        let name = self.binding_ident();
        if !self.is("{") {
            self.ts_fail();
            return;
        }
        let close = self.mate_of(self.pos);
        self.next();
        let mut body = format!("var {}; (function ({}) {{\n", name, name);
        let mut prev = String::new();
        let mut first = true;
        while self.pos < close && self.error.is_empty() {
            let member = if self.kind() == T_STR {
                let t = self.text();
                self.next();
                t
            } else {
                self.ident_name()
            };
            if !self.error.is_empty() {
                return;
            }
            let mq = pq(member.as_str());
            let mut is_string = false;
            let value: String;
            if self.eat("=") {
                let a = self.pos;
                let e = self.scan_to(a, ",", false);
                if e <= a {
                    self.ts_fail();
                    return;
                }
                is_string = e == a + 1 && self.kind() == T_STR;
                if self.kind() == T_TEMPLATE {
                    self.ts_fail();
                    return;
                }
                let last = e - 1;
                value = format!("({})", self.src_text(a, last));
                self.goto(e);
            } else if first {
                value = String::from("0");
            } else {
                value = format!("{}[{}] + 1", name, pq(prev.as_str()));
            }
            if is_string {
                body.push_str(format!("{}[{}] = {};\n", name, mq, value).as_str());
            } else {
                body.push_str(format!("{}[{}[{}] = {}] = {};\n", name, name, mq, value, mq).as_str());
            }
            if pcomp_ident(member.as_str()) && member != name {
                body.push_str(format!("var {} = {}[{}];\n", member, name, mq).as_str());
            }
            prev = member;
            first = false;
            if !self.eat(",") {
                break;
            }
        }
        if self.pos != close {
            self.ts_fail();
            return;
        }
        self.next();
        body.push_str(format!("}})({} || ({} = {{}}));\n", name, name).as_str());
        self.ts_source(body.as_str());
    }

    /// `namespace N { … }` as TypeScript writes it, as the parser does:
    ///   var N; (function (N) { …; N.a = a; })(N || (N = {}));
    /// an exported declaration stays, its names set on N after it.
    fn namespace_decl(&mut self) {
        self.next();
        let name = self.binding_ident();
        if self.is(".") && self.error.is_empty() {
            // `namespace A.B { … }`: each inner one exported from the outer
            let mut chain: Vec<String> = Vec::new();
            while self.eat(".") {
                chain.push(self.ident_name());
            }
            if !self.is("{") || !self.error.is_empty() {
                self.ts_fail();
                return;
            }
            let open = self.pos;
            let close = self.mate_of(open);
            let mut src = format!("namespace {} {{", name);
            for c in chain.iter() {
                src.push_str(format!(" export namespace {} {{", c).as_str());
            }
            let inner = open + 1;
            let t = self.src_text(inner, close);
            src.push_str(t.as_str());
            let mut depth: usize = 0;
            while depth < chain.len() {
                src.push_str(" }");
                depth += 1;
            }
            src.push_str("\n");
            self.goto(close);
            self.next();
            self.ts_source(src.as_str());
            return;
        }
        if !self.is("{") || !self.error.is_empty() {
            self.ts_fail();
            return;
        }
        let open = self.pos;
        let close = self.mate_of(open);
        let mut body = format!("var {}; (function ({}) {{\n", name, name);
        let mut pending: Vec<String> = Vec::new();
        let mut from = open + 1;
        let mut k = open + 1;
        while k < close {
            if self.tok_is(k, "{") || self.tok_is(k, "(") || self.tok_is(k, "[") {
                k = self.mate_of(k) + 1;
                continue;
            }
            let starts = k == open + 1 || self.tok_is(k - 1, ";") || self.tok_is(k - 1, "}") || self.tok_nl(k);
            if starts && self.tok_is(k, "export") {
                if k > from {
                    let last = k - 1;
                    let t = self.src_text(from, last);
                    body.push_str(t.as_str());
                    body.push_str("\n");
                }
                for n in pending.iter() {
                    body.push_str(format!("{}.{} = {};\n", name, n, n).as_str());
                }
                pending = Vec::new();
                // the names it declares
                let mut j = k + 1;
                if self.tok_is(j, "declare") {
                    self.ts_fail();
                    return;
                }
                if self.tok_is(j, "const") && self.tok_is(j + 1, "enum") {
                    j += 1;
                }
                if self.tok_is(j, "abstract") || self.tok_is(j, "async") {
                    j += 1;
                }
                if self.tok_is(j, "function") || self.tok_is(j, "class") || self.tok_is(j, "enum") || self.tok_is(j, "namespace") {
                    let mut n = j + 1;
                    if self.tok_is(n, "*") {
                        n += 1;
                    }
                    pending.push(self.tok_text(n));
                } else if self.tok_is(j, "const") || self.tok_is(j, "let") || self.tok_is(j, "var") {
                    // plain names only: `export const a = 1, b = 2`
                    let mut n = j + 1;
                    loop {
                        if (self.toks[n as usize].kind as int) != T_IDENT {
                            self.ts_fail();
                            return;
                        }
                        pending.push(self.tok_text(n));
                        let e = self.scan_to(n + 1, ", ;", false);
                        if !self.tok_is(e, ",") {
                            break;
                        }
                        n = e + 1;
                    }
                } else if self.tok_is(j, "interface") || self.tok_is(j, "type") {
                    // types only
                } else {
                    self.ts_fail();
                    return;
                }
                from = k + 1;
            }
            k += 1;
        }
        if close > from {
            let last = close - 1;
            let t = self.src_text(from, last);
            body.push_str(t.as_str());
            body.push_str("\n");
        }
        for n in pending.iter() {
            body.push_str(format!("{}.{} = {};\n", name, n, n).as_str());
        }
        body.push_str(format!("}})({} || ({} = {{}}));\n", name, name).as_str());
        self.goto(close);
        self.next();
        self.ts_source(body.as_str());
    }

    /// A value `import` in a script, as the parser reads it: each name a
    /// `const` read from `__cerxesImport("m")`; a type-only entry erased.
    fn script_import(&mut self) {
        self.next();
        if self.kind() == T_STR {
            let m = self.text();
            self.next();
            self.semicolon();
            let call = self.import_text(m.as_str());
            self.ts_source(format!("{};\n", call).as_str());
            return;
        }
        let mut names: Vec<String> = Vec::new();
        let mut keys: Vec<String> = Vec::new();
        if self.kind() == T_IDENT && !self.is("{") && !self.is("*") {
            if self.peek_is(1, "=") {
                // `import A = B.C`: an alias (`require` is the parser's)
                if self.peek_is(2, "require") {
                    self.ts_fail();
                    return;
                }
                let n = self.text();
                let a = self.pos + 2;
                let mut e = a;
                while e < self.tok_count() && (self.toks[e as usize].kind as int) != T_EOF && !self.tok_is(e, ";") && !(e > a && self.tok_nl(e)) {
                    e += 1;
                }
                if e == a {
                    self.ts_fail();
                    return;
                }
                let last = e - 1;
                let t = self.src_text(a, last);
                self.goto(e);
                self.semicolon();
                self.ts_source(format!("var {} = {};\n", n, t).as_str());
                return;
            }
            names.push(self.text());
            keys.push(String::from("default"));
            self.next();
            self.eat(",");
        }
        if self.is("*") {
            self.next();
            if !self.eat("as") {
                self.ts_fail();
                return;
            }
            names.push(self.text());
            keys.push(String::new());
            self.next();
        } else if self.is("{") {
            self.next();
            while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
                if self.is("type") && self.peek_kind(1) == T_IDENT && !self.peek_is(1, "as") && !self.peek_is(1, ",") && !self.peek_is(1, "}") {
                    self.next();
                    self.next();
                    if self.eat("as") {
                        self.next();
                    }
                } else {
                    let k = self.text();
                    self.next();
                    let n = if self.eat("as") {
                        let t = self.text();
                        self.next();
                        t
                    } else {
                        k.clone()
                    };
                    names.push(n);
                    keys.push(k);
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("}");
        }
        if !self.eat("from") || self.kind() != T_STR {
            self.ts_fail();
            return;
        }
        let m = self.text();
        self.next();
        if (self.is("with") || self.is("assert")) && self.peek_is(1, "{") {
            self.next();
            let a = self.after_mate(self.pos);
            self.goto(a);
        }
        self.semicolon();
        let call = self.import_text(m.as_str());
        if names.is_empty() {
            self.ts_source(format!("{};\n", call).as_str());
            return;
        }
        let mut src = String::from("const ");
        let mut i: usize = 0;
        while i < names.len() {
            if i > 0 {
                src.push_str(", ");
            }
            if keys[i].is_empty() {
                src.push_str(format!("{} = {}", names[i], call).as_str());
            } else {
                src.push_str(format!("{} = {}[{}]", names[i], call, pq(keys[i].as_str())).as_str());
            }
            i += 1;
        }
        src.push_str(";\n");
        self.ts_source(src.as_str());
    }

    /// `export` in a script: the declaration stays, the keyword goes;
    /// export lists and re-exports are dropped.
    fn script_export(&mut self, in_list: bool) {
        self.next();
        if self.eat("default") {
            if self.is("function") || self.is("class") || (self.is("async") && self.peek_is(1, "function")) || self.is("abstract") || self.is("interface") {
                self.list_stmt = in_list;
                self.statement();
                return;
            }
            // an expression (`export default { a, b }` is an object)
            let e = self.assign();
            self.semicolon();
            self.expr_statement_tail(e);
            return;
        }
        if self.is("=") || self.is("{") || self.is("*") || (self.is("as") && self.peek_is(1, "namespace")) {
            self.pos -= 1;
            self.skip_module_clause();
            return;
        }
        if self.is("import") {
            self.ts_fail();
            return;
        }
        self.list_stmt = in_list;
        self.statement();
    }

    /// `__cerxesImport("m")`; in a module, `__cerxesImport("m", "its/file")`,
    /// so "./x" and "../x" are read from the module's folder.
    fn import_text(&self, m: &str) -> String {
        if self.module_path.is_empty() {
            return format!("__cerxesImport({})", pq(m));
        }
        format!("__cerxesImport({}, {})", pq(m), pq(self.module_path.as_str()))
    }

    /// An export the module returns: its name, the expression that reads it.
    fn module_export_name(&mut self, name: &str, value: &str) {
        let mut i: usize = 0;
        while i < self.mex_names.len() {
            if self.mex_names[i].as_str() == name {
                // the parser names the module in the error
                self.ts_fail();
                return;
            }
            i += 1;
        }
        self.mex_names.push(String::from(name));
        self.mex_values.push(String::from(value));
    }

    /// The names the declaration statement at the current token declares in
    /// the module's own scope, each exported under its own name, or all
    /// under `default` when `as_default`.
    fn module_export_decl(&mut self, in_list: bool, as_default: bool) {
        let start = self.binds.len();
        let scope = self.cur_scope;
        let vs = self.f().var_scope;
        self.list_stmt = in_list;
        self.statement();
        if !self.error.is_empty() {
            return;
        }
        let mut found: Vec<String> = Vec::new();
        let mut i = start;
        while i < self.binds.len() {
            let bs = self.binds[i].scope;
            if bs == scope || bs == vs {
                found.push(self.binds[i].name.clone());
            }
            i += 1;
        }
        for n in found.iter() {
            if as_default {
                self.module_export_name("default", n.as_str());
            } else {
                self.module_export_name(n.as_str(), n.as_str());
            }
        }
    }

    /// A name in an export list: a name or a string.
    fn export_name(&mut self) -> String {
        let t = self.text();
        self.next();
        t
    }

    /// `with { … }` / `assert { … }` after a module name, and the semicolon.
    fn module_clause_end(&mut self) {
        if (self.is("with") || self.is("assert")) && self.peek_is(1, "{") {
            self.next();
            let a = self.after_mate(self.pos);
            if a < 0 {
                self.ts_fail();
                return;
            }
            self.goto(a);
        }
        self.semicolon();
    }

    /// `export` in a module (`compile_module`): the declaration stays, and
    /// what it exports is noted for the module to return (as the parser's
    /// `module_export` does).
    fn module_export(&mut self, in_list: bool) {
        self.next();
        if self.eat("default") {
            if self.ts && self.is("interface") {
                self.skip_to_body();
                return;
            }
            let named = (self.is("function") && (self.peek_kind(1) == T_IDENT || (self.peek_is(1, "*") && self.peek_kind(2) == T_IDENT)))
                || (self.is("async") && self.peek_is(1, "function") && (self.peek_kind(2) == T_IDENT || (self.peek_is(2, "*") && self.peek_kind(3) == T_IDENT)))
                || (self.is("class") && self.peek_kind(1) == T_IDENT && !self.peek_is(1, "extends") && !self.peek_is(1, "implements") && !self.peek_is(1, "{"))
                || (self.is("abstract") && self.peek_is(1, "class") && self.peek_kind(2) == T_IDENT && !self.peek_is(2, "extends") && !self.peek_is(2, "implements") && !self.peek_is(2, "{"));
            if named {
                self.module_export_decl(in_list, true);
                return;
            }
            let e = self.assign();
            if !self.error.is_empty() {
                return;
            }
            self.name_anonymous(e, "default");
            self.semicolon();
            let scope = self.cur_scope;
            let b = self.declare(scope, "__cerxesDefault", K_CONST);
            self.emit_bind_init(b);
            self.op(OP_POP);
            self.mark_inited(b);
            self.module_export_name("default", "__cerxesDefault");
            return;
        }
        if self.is("=") || (self.is("as") && self.peek_is(1, "namespace")) || (self.ts && self.is("type") && (self.peek_is(1, "{") || self.peek_is(1, "*"))) {
            self.pos -= 1;
            self.skip_module_clause();
            return;
        }
        if self.is("*") {
            self.next();
            let mut alias = String::new();
            let mut aliased = false;
            if self.eat("as") {
                alias = self.export_name();
                aliased = true;
            }
            if !self.eat("from") || self.kind() != T_STR {
                self.ts_fail();
                return;
            }
            let m = self.text();
            self.next();
            self.module_clause_end();
            if aliased {
                let read = self.import_text(m.as_str());
                self.module_export_name(alias.as_str(), read.as_str());
            } else {
                self.mex_stars.push(m);
            }
            return;
        }
        if self.is("{") {
            self.next();
            let mut names: Vec<String> = Vec::new();
            let mut keys: Vec<String> = Vec::new();
            while !self.is("}") && self.kind() != T_EOF && self.error.is_empty() {
                if self.ts && self.is("type") && self.peek_kind(1) == T_IDENT && !self.peek_is(1, "as") && !self.peek_is(1, ",") && !self.peek_is(1, "}") {
                    // `type T`: erased
                    self.next();
                    self.next();
                    if self.eat("as") {
                        self.next();
                    }
                } else {
                    let k = self.export_name();
                    let n = if self.eat("as") { self.export_name() } else { k.clone() };
                    names.push(n);
                    keys.push(k);
                }
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("}");
            let mut from = String::new();
            let mut has_from = false;
            if self.eat("from") {
                if self.kind() != T_STR {
                    self.ts_fail();
                    return;
                }
                from = self.text();
                has_from = true;
                self.next();
            }
            self.module_clause_end();
            let mut i: usize = 0;
            while i < names.len() {
                let v = if has_from {
                    let read = self.import_text(from.as_str());
                    format!("{}[{}]", read, pq(keys[i].as_str()))
                } else {
                    keys[i].clone()
                };
                let n = names[i].clone();
                self.module_export_name(n.as_str(), v.as_str());
                i += 1;
            }
            return;
        }
        if self.is("import") {
            self.ts_fail();
            return;
        }
        self.module_export_decl(in_list, false);
    }

    /// What the module returns: its exports as an object (the modules of
    /// `export *` under them, but their defaults).
    fn module_exports_src(&self) -> String {
        let mut fields: Vec<String> = Vec::new();
        let mut i: usize = 0;
        while i < self.mex_names.len() {
            fields.push(format!("{}: {}", pq(self.mex_names[i].as_str()), self.mex_values[i]));
            i += 1;
        }
        let own = format!("{{{}}}", fields.join(", "));
        if self.mex_stars.is_empty() {
            return format!("({})", own);
        }
        let mut parts: Vec<String> = Vec::new();
        for m in self.mex_stars.iter() {
            let read = self.import_text(m.as_str());
            parts.push(format!("__cerxesStar({})", read));
        }
        format!("Object.assign({{}}, {}, {})", parts.join(", "), own)
    }

    /// An expression written as source, compiled where the current token is.
    fn source_expr(&mut self, src: &str) -> Ex {
        let saved_src = self.src_rc.clone();
        self.src_rc = Rc::new(String::from(src));
        let saved = self.push_tokens(src);
        let e = self.assign();
        self.pop_tokens(&saved);
        self.src_rc = saved_src;
        e
    }

    /// Statements written as source for the ones read (`import`).
    fn ts_source(&mut self, src: &str) {
        let saved_src = self.src_rc.clone();
        self.src_rc = Rc::new(String::from(src));
        let saved = self.push_tokens(src);
        while self.kind() != T_EOF && self.error.is_empty() {
            self.list_stmt = true;
            self.statement();
        }
        self.pop_tokens(&saved);
        self.src_rc = saved_src;
    }

    /// TypeScript cannot be read here: the parser reads it.
    fn ts_fail(&mut self) {
        self.fail("TypeScript pcomp does not read");
    }

    /// Skips the type at the current token (pcomp reads on after it).
    fn skip_type_here(&mut self) {
        let e = self.type_end(self.pos);
        if e < 0 && self.type_ends_in_eq() {
            return;
        }
        if e < 0 {
            self.ts_fail();
            return;
        }
        self.goto(e);
    }

    /// A type that ends inside a `>>=` / `>=` token (`a: A<B<C>>= v`): the
    /// token becomes its `=`, where pcomp reads on; false when it did not.
    fn type_ends_in_eq(&mut self) -> bool {
        if self.tbad || self.tg <= 0 {
            return false;
        }
        let t = self.tok_text(self.tp);
        let g = self.tg as usize;
        if g >= t.as_bytes().len() {
            return false;
        }
        let rest = String::from(&t[g..]);
        if rest.as_str() != "=" {
            return false;
        }
        let eq = self.name_id("=");
        let i = self.tp as usize;
        self.toks[i].v = eq as i32;
        self.toks[i].start += self.tg as i32;
        self.tg = 0;
        let at = self.tp;
        self.goto(at);
        true
    }

    /// `: T` (and a `?` / `!` before it) after a binding or a parameter.
    fn skip_annotation(&mut self) {
        if !self.ts {
            return;
        }
        if self.is("?") || self.is("!") {
            self.next();
        }
        if self.is(":") {
            self.next();
            self.skip_type_here();
        }
    }

    /// `: R` after a parameter list.
    fn skip_return_type(&mut self) {
        if self.ts && self.is(":") {
            self.next();
            let e = self.return_type_end(self.pos);
            if e < 0 {
                self.ts_fail();
                return;
            }
            self.goto(e);
        }
    }

    /// `<T>` before a parameter list.
    fn skip_type_params(&mut self) {
        if self.ts && self.is("<") {
            let e = self.type_params_end(self.pos);
            if e < 0 {
                self.ts_fail();
                return;
            }
            self.goto(e);
        }
    }
}

/// `s` as a JavaScript string literal.
fn pq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if c == '"' {
            out.push_str("\\\"");
        } else if c == '\\' {
            out.push_str("\\\\");
        } else if c == '\n' {
            out.push_str("\\n");
        } else if c == '\r' {
            out.push_str("\\r");
        } else if (c as u32) < 32 || c == '\u{2028}' || c == '\u{2029}' {
            out.push_str(format!("\\u{:04x}", c as u32).as_str());
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// A name a `var` may declare.
fn pcomp_ident(s: &str) -> bool {
    if s.is_empty() || is_reserved(s) {
        return false;
    }
    let mut first = true;
    for c in s.chars() {
        let ok = c == '_' || c == '$' || c.is_alphabetic() || (!first && c.is_numeric());
        if !ok {
            return false;
        }
        first = false;
    }
    true
}

/// A script compiled as a unit, read and written in one pass.
pub fn compile_unit(src: &str) -> crate::unit::Unit {
    compile_unit_with(src, false)
}

/// compile_unit, TypeScript's types skipped with `ts`.
pub fn compile_unit_with(src: &str, ts: bool) -> crate::unit::Unit {
    compile_unit_syntax(src, ts, false, "", "")
}

/// compile_unit: TypeScript's types skipped with `ts`, JSX read with `jsx`
/// (`factory`, `fragment` its names).
pub fn compile_unit_syntax(src: &str, ts: bool, jsx: bool, factory: &str, fragment: &str) -> crate::unit::Unit {
    compile_unit_full(src, ts, jsx, factory, fragment, false)
}

/// A host's script (CErXes `Engine::compile`): compile_unit_syntax, and
/// its `import`s read the host's modules (`script_imports`).
pub fn compile_script(src: &str, ts: bool, jsx: bool, factory: &str, fragment: &str) -> crate::unit::Unit {
    compile_unit_full(src, ts, jsx, factory, fragment, true)
}

fn compile_unit_full(src: &str, ts: bool, jsx: bool, factory: &str, fragment: &str, imports: bool) -> crate::unit::Unit {
    let names = crate::vm::first_atoms();
    let mut atoms: HashMap<String, int> = HashMap::new();
    let mut i: int = 0;
    for n in names.iter() {
        atoms.insert(n.clone(), i);
        i += 1;
    }
    let mut c = PComp::new_syntax(src, atoms, names, 0, jsx, factory, fragment);
    c.ts = ts;
    c.script_imports = imports;
    let entry = c.compile_program();
    let mut u = crate::unit::Unit::new();
    if !c.error.is_empty() || entry < 0 {
        u.error = if c.error.is_empty() { String::from("SyntaxError") } else { c.error.clone() };
        return u;
    }
    u.atom_names = c.atom_names;
    u.protos = c.protos;
    u.entry = entry;
    u.source = String::from(src);
    u
}

/// Rust: the script's tokens without the room their doubling left.
#[ranger::target(rust)]
fn trim_toks(v: &mut Vec<PTok>) {
    v.shrink_to_fit();
}

#[ranger::target(es6, python, go, cpp, java7, kotlin, csharp, dart, scala, php, swift6)]
fn trim_toks(_v: &mut Vec<PTok>) {}

/// A private key ("\u{1}#x\u{2}<id>") as its name ("\u{1}#x").
fn priv_raw(k: &str) -> String {
    let i = crate::jsstr::index_of(k, "\u{2}", 0);
    if i < 0 {
        return String::from(k);
    }
    crate::jsstr::slice(k, 0, i)
}

/// A name as shown: a private key's `#x`.
fn priv_shown(k: &str) -> String {
    let r = priv_raw(k);
    if r.starts_with("\u{1}") {
        return crate::jsstr::slice(r.as_str(), 1, crate::jsstr::len(r.as_str()));
    }
    r
}

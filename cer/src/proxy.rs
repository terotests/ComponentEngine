// SPDX-License-Identifier: AGPL-3.0-or-later
//! Proxy. A proxy (C_PROXY) holds its target in `env` and its handler in
//! `home` (both -1 once revoked); `pos` bit 1 when the target is callable.
//! Every object operation that meets a proxy calls one of the prelude's
//! hooks, which runs the handler's trap (or forwards to the target) and
//! checks the invariants the specification sets on the answer.

use ranger::prelude::*;

use crate::builtins::*;
use crate::value::*;
use crate::vm::*;

pub const PH_GET: int = 0;
pub const PH_SET: int = 1;
pub const PH_HAS: int = 2;
pub const PH_DELETE: int = 3;
pub const PH_OWNKEYS: int = 4;
pub const PH_GOPD: int = 5;
pub const PH_DEFINE: int = 6;
pub const PH_GETPROTO: int = 7;
pub const PH_SETPROTO: int = 8;
pub const PH_ISEXT: int = 9;
pub const PH_PREVENTEXT: int = 10;
pub const PH_APPLY: int = 11;
pub const PH_CONSTRUCT: int = 12;
pub const PH_ISARRAY: int = 13;

pub const NF_PROXY: int = 930;
pub const NF_PROXY_REVOKE: int = 931;
pub const NF_PROXY_SETUP: int = 932;
pub const NF_IS_PROXY: int = 933;
pub const NF_IS_REGEXP: int = 934;
pub const NF_IS_PROMISE: int = 935;

impl Vm {
    pub fn is_proxy(&self, o: int) -> bool {
        o >= 0 && self.objs[o as usize].class == C_PROXY
    }

    pub fn setup_proxy(&mut self) {
        let g = self.global;
        let f = self.method(g, "Proxy", NF_PROXY, 2);
        let _ = f;
        self.method(g, "__proxyRevoke", NF_PROXY_REVOKE, 1);
        self.method(g, "__proxySetup", NF_PROXY_SETUP, 1);
        self.method(g, "__isProxy", NF_IS_PROXY, 1);
        self.method(g, "__isRegExp", NF_IS_REGEXP, 1);
        self.method(g, "__isPromise", NF_IS_PROMISE, 1);
    }

    /// Calls hook `which` for proxy `p`: hook(target, handler, ...args).
    pub fn proxy_call(&mut self, p: int, which: int, args: Vec<Val>) -> Val {
        let target = self.objs[p as usize].env as int;
        let handler = self.colds[self.objs[p as usize].cold as usize].home;
        if handler < 0 || target < 0 || self.proxy_hooks < 0 {
            self.throw_type("Cannot perform an operation on a proxy that has been revoked");
            return Val::Undef;
        }
        let hook = self.evecs[self.objs[self.proxy_hooks as usize].elems as usize].v[which as usize].clone();
        let mut all: Vec<Val> = vec![Val::Obj(target), Val::Obj(handler)];
        for a in args.iter() {
            all.push(*a);
        }
        self.call_value(hook, Val::Undef, all)
    }

    /// `p[atom]` (receiver r) for a proxy of a plain object: the trap is
    /// called and its invariants checked here, not in the prelude's hook
    /// (no descriptor object per get); other targets take the hook.
    pub fn proxy_get_atom(&mut self, p: int, atom: int, r: Val) -> Val {
        let target = self.objs[p as usize].env as int;
        let handler = self.colds[self.objs[p as usize].cold as usize].home;
        if handler < 0 || target < 0 || self.proxy_hooks < 0 || self.objs[target as usize].class != C_OBJECT {
            let k = self.key_val(atom);
            return self.proxy_call(p, PH_GET, vec![k, r]);
        }
        let trap = self.get_obj(handler, A_GET, &Val::Obj(handler));
        if self.throwing {
            return Val::Undef;
        }
        if matches!(trap, Val::Undef) || matches!(trap, Val::Null) {
            return self.get_obj(target, atom, &r);
        }
        if !self.is_callable(&trap) {
            self.throw_type("'get' on proxy: trap is not a function");
            return Val::Undef;
        }
        let k = self.key_val(atom);
        let v = self.call_value(trap, Val::Obj(handler), vec![Val::Obj(target), k, r]);
        if self.throwing {
            return Val::Undef;
        }
        let slot = self.find(target, atom);
        if slot >= 0 {
            let attr = self.attr_at(target, slot);
            if (attr & P_FIXED) != 0 {
                if (attr & P_ACCESSOR) != 0 {
                    let pair = obj_of(&self.objs[target as usize].vals[slot as usize]);
                    let g = self.evecs[self.objs[pair as usize].elems as usize].v[0].clone();
                    if matches!(g, Val::Undef) && !matches!(v, Val::Undef) {
                        let n = self.atom_str(atom);
                        self.throw_type(format!("'get' on proxy: property '{}' is a non-configurable accessor property on the proxy target and does not have a getter function, but the trap did not return 'undefined'", n).as_str());
                        return Val::Undef;
                    }
                } else if (attr & P_READONLY) != 0 {
                    let cur = self.objs[target as usize].vals[slot as usize].clone();
                    if !self.same_value(&v, &cur) {
                        let n = self.atom_str(atom);
                        self.throw_type(format!("'get' on proxy: property '{}' is a read-only and non-configurable data property on the proxy target but the proxy did not return its actual value", n).as_str());
                        return Val::Undef;
                    }
                }
            }
        }
        v
    }

    /// `p[atom] = v` (receiver r) for a proxy of a plain object whose
    /// handler has a set trap: the trap is called and its invariants
    /// checked here; other cases take the prelude's hook.
    pub fn proxy_set_atom(&mut self, p: int, atom: int, v: Val, r: Val) -> bool {
        let target = self.objs[p as usize].env as int;
        let handler = self.colds[self.objs[p as usize].cold as usize].home;
        if handler < 0 || target < 0 || self.proxy_hooks < 0 || self.objs[target as usize].class != C_OBJECT {
            let k = self.key_val(atom);
            return self.proxy_bool(p, PH_SET, vec![k, v, r]);
        }
        let trap = self.get_obj(handler, A_SET, &Val::Obj(handler));
        if self.throwing {
            return false;
        }
        if matches!(trap, Val::Undef) || matches!(trap, Val::Null) {
            let k = self.key_val(atom);
            return self.proxy_bool(p, PH_SET, vec![k, v, r]);
        }
        if !self.is_callable(&trap) {
            self.throw_type("'set' on proxy: trap is not a function");
            return false;
        }
        let k = self.key_val(atom);
        let ok = self.call_value(trap, Val::Obj(handler), vec![Val::Obj(target), k, v.clone(), r]);
        if self.throwing || !truthy(&ok) {
            return false;
        }
        let slot = self.find(target, atom);
        if slot >= 0 {
            let attr = self.attr_at(target, slot);
            if (attr & P_FIXED) != 0 {
                if (attr & P_ACCESSOR) != 0 {
                    let pair = obj_of(&self.objs[target as usize].vals[slot as usize]);
                    let s = self.evecs[self.objs[pair as usize].elems as usize].v[1].clone();
                    if matches!(s, Val::Undef) {
                        let n = self.atom_str(atom);
                        self.throw_type(format!("'set' on proxy: trap returned truish for property '{}' which exists in the proxy target as a non-configurable and non-writable accessor property without a setter", n).as_str());
                        return false;
                    }
                } else if (attr & P_READONLY) != 0 {
                    let cur = self.objs[target as usize].vals[slot as usize].clone();
                    if !self.same_value(&v, &cur) {
                        let n = self.atom_str(atom);
                        self.throw_type(format!("'set' on proxy: trap returned truish for property '{}' which exists in the proxy target as a non-configurable and non-writable data property with a different value", n).as_str());
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn proxy_bool(&mut self, p: int, which: int, args: Vec<Val>) -> bool {
        let r = self.proxy_call(p, which, args);
        !self.throwing && truthy(&r)
    }

    /// The keys a proxy reports; `enumerable_only` keeps those whose
    /// descriptor (from the handler) says enumerable.
    pub fn proxy_keys(&mut self, p: int, enumerable_only: bool, symbols: bool) -> Vec<Val> {
        let r = self.proxy_call(p, PH_OWNKEYS, Vec::new());
        let mut out: Vec<Val> = Vec::new();
        if self.throwing {
            return out;
        }
        let items = self.array_like_to_vec(&r);
        for k in items {
            let is_sym = self.class_of(&k) == C_SYMBOL;
            if is_sym && !symbols {
                continue;
            }
            if enumerable_only {
                let d = self.proxy_call(p, PH_GOPD, vec![k.clone()]);
                if self.throwing {
                    return Vec::new();
                }
                if !is_obj(&d) {
                    continue;
                }
                let e = self.get(&d, A_ENUMERABLE);
                if !truthy(&e) {
                    continue;
                }
            }
            out.push(k);
        }
        out
    }

    /// Array.isArray through proxies.
    pub fn is_array_val(&mut self, v: &Val) -> bool {
        let o = obj_of(v);
        if o < 0 {
            return false;
        }
        if self.objs[o as usize].class == C_PROXY {
            let t = self.objs[o as usize].env as int;
            if t < 0 {
                self.throw_type("Cannot perform 'IsArray' on a proxy that has been revoked");
                return false;
            }
            return self.is_array_val(&Val::Obj(t));
        }
        self.objs[o as usize].class == C_ARRAY
    }

    pub fn call_native_proxy(&mut self, id: int, args: &Vec<Val>, construct: bool) -> Val {
        let a0 = if args.is_empty() { Val::Undef } else { args[0].clone() };
        let a1 = if args.len() < 2 { Val::Undef } else { args[1].clone() };
        if id == NF_PROXY {
            if !construct {
                self.throw_type("Constructor Proxy requires 'new'");
                return Val::Undef;
            }
            let t = obj_of(&a0);
            let h = obj_of(&a1);
            if t < 0 || h < 0 {
                self.throw_type("Cannot create proxy with a non-object as target or handler");
                return Val::Undef;
            }
            let p = self.alloc(C_PROXY, -1);
            self.put_env(p, t);
            self.put_home(p, h);
            if self.is_callable(&a0) {
                self.objs[p as usize].pos = 1;
            }
            // writes that miss look along the chain, where a proxy may be
            self.any_setter = true;
            self.proxy_made = true;
            return Val::Obj(p);
        }
        if id == NF_PROXY_REVOKE {
            let p = obj_of(&a0);
            if self.is_proxy(p) {
                self.put_env(p, -1);
                self.put_home(p, -1);
            }
            return Val::Undef;
        }
        if id == NF_IS_PROXY {
            return Val::Bool(self.is_proxy(obj_of(&a0)));
        }
        if id == NF_IS_REGEXP {
            let o = obj_of(&a0);
            return Val::Bool(o >= 0 && self.objs[o as usize].class == C_REGEXP);
        }
        if id == NF_IS_PROMISE {
            let o = obj_of(&a0);
            return Val::Bool(o >= 0 && self.objs[o as usize].class == C_PROMISE);
        }
        if id == NF_PROXY_SETUP {
            let h = obj_of(&a0);
            self.proxy_hooks = h;
            if h >= 0 {
                self.roots.push(h);
            }
            return Val::Undef;
        }
        Val::Undef
    }

    /// A property key as the hooks take it: a string or a symbol.
    pub fn prop_key(&mut self, v: &Val) -> Val {
        if self.class_of(v) == C_SYMBOL {
            return v.clone();
        }
        let (i, a) = self.to_key(v);
        if self.throwing {
            return Val::Undef;
        }
        if i >= 0 {
            return self.string_val(format!("{}", i));
        }
        self.key_val(a)
    }

    /// The Object and Reflect built-ins on a proxy `p` (the first argument,
    /// or `this` for the Object.prototype methods); None for the others.
    pub fn proxy_native(&mut self, id: int, p: int, args: &Vec<Val>) -> Option<Val> {
        let a1 = if args.len() < 2 { Val::Undef } else { args[1].clone() };
        let a2 = if args.len() < 3 { Val::Undef } else { args[2].clone() };
        let pv = Val::Obj(p);
        if id == NF_O_GETPROTO || id == NF_REFLECT_GETPROTO {
            let rv = self.proxy_call(p, PH_GETPROTO, Vec::new());
            return Some(rv);
        }
        if id == NF_O_SETPROTO {
            let va1: Vec<Val> = vec![a1];
            let ok = self.proxy_bool(p, PH_SETPROTO, va1);
            if !ok && !self.throwing {
                self.throw_type("'setPrototypeOf' on proxy: trap returned falsish");
            }
            return Some(pv);
        }
        if id == NF_O_DEFPROP || id == NF_REFLECT_DEFPROP {
            let k = self.prop_key(&a1);
            if self.throwing {
                return Some(Val::Undef);
            }
            let va2: Vec<Val> = vec![k.clone(), a2];
            let ok = self.proxy_bool(p, PH_DEFINE, va2);
            if id == NF_REFLECT_DEFPROP {
                return Some(Val::Bool(ok));
            }
            if !ok && !self.throwing {
                let n = self.to_string(&k);
                self.throw_type(format!("'defineProperty' on proxy: trap returned falsish for property '{}'", n).as_str());
            }
            return Some(pv);
        }
        if id == NF_O_OWNDESC {
            let k = self.prop_key(&a1);
            if self.throwing {
                return Some(Val::Undef);
            }
            let va3: Vec<Val> = vec![k];
            let rv = self.proxy_call(p, PH_GOPD, va3);
            return Some(rv);
        }
        if id == NF_O_ISEXT {
            let rb = self.proxy_bool(p, PH_ISEXT, Vec::new());
            return Some(Val::Bool(rb));
        }
        if id == NF_O_PREVENTEXT {
            let ok = self.proxy_bool(p, PH_PREVENTEXT, Vec::new());
            if !ok && !self.throwing {
                self.throw_type("'preventExtensions' on proxy: trap returned falsish");
            }
            return Some(pv);
        }
        if id == NF_OP_HASOWN || id == NF_O_HASOWN || id == NF_OP_PROPENUM {
            let kv = if id == NF_O_HASOWN { a1 } else if args.is_empty() { Val::Undef } else { args[0].clone() };
            let k = self.prop_key(&kv);
            if self.throwing {
                return Some(Val::Undef);
            }
            let va4: Vec<Val> = vec![k];
            let d = self.proxy_call(p, PH_GOPD, va4);
            if self.throwing {
                return Some(Val::Undef);
            }
            if id == NF_OP_PROPENUM {
                if !is_obj(&d) {
                    return Some(Val::Bool(false));
                }
                let e = self.get(&d, A_ENUMERABLE);
                return Some(Val::Bool(truthy(&e)));
            }
            return Some(Val::Bool(is_obj(&d)));
        }
        if id == NF_REFLECT_HAS {
            let k = self.prop_key(&a1);
            let va5: Vec<Val> = vec![k];
            let rb = self.proxy_bool(p, PH_HAS, va5);
            return Some(Val::Bool(rb));
        }
        if id == NF_REFLECT_GET {
            let k = self.prop_key(&a1);
            let r = if args.len() > 2 { a2 } else { pv };
            let va6: Vec<Val> = vec![k, r];
            let rv = self.proxy_call(p, PH_GET, va6);
            return Some(rv);
        }
        if id == NF_REFLECT_SET {
            let k = self.prop_key(&a1);
            let r = if args.len() > 3 { args[3].clone() } else { pv };
            let va7: Vec<Val> = vec![k, a2, r];
            let rb = self.proxy_bool(p, PH_SET, va7);
            return Some(Val::Bool(rb));
        }
        if id == NF_REFLECT_DELETE {
            let k = self.prop_key(&a1);
            let va8: Vec<Val> = vec![k];
            let rb = self.proxy_bool(p, PH_DELETE, va8);
            return Some(Val::Bool(rb));
        }
        if id == NF_A_ISARRAY {
            return Some(Val::Bool(self.is_array_val(&pv)));
        }
        None
    }
}

// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Hostile input oracles (audit A1, A2). A host that stores snapshots or
// code-cache units outside the process (disk, a database, another machine)
// must survive one that was changed on the way: it is refused, or it
// restores and runs; it never panics or crashes the host.
//
// The old `snapshot-corrupt` check changed 1–4 random characters of a
// 500 kB text and only counted a panic in `from_snapshot`. These change
// the fields that index things (free list, frame pc / bp / env, handler
// sp, stack values, object protos and shapes, opcodes and operands), the
// way a forged text would, and then RUN the restored engine, which is
// where an index that passed the checks is used. In the checked build
// (debug assertions on, the worker default) an unchecked index that is
// out of range asserts, so the finding has a source line instead of a
// SIGSEGV.

use crate::gen::Rng;
use crate::stage;
use cerxes::Engine;
use std::time::Instant;

const BOUNDARY: [i64; 16] = [-1, -2, 0, 1, 2, 3, 7, 64, 255, 4096, 65535, 1_000_000, 50_000_000, i32::MAX as i64, i32::MIN as i64, -1_000_000];

fn run_budget(e: &mut Engine, ms: u128) {
    let t0 = Instant::now();
    while e.is_suspended() && t0.elapsed().as_millis() < ms {
        e.resume();
    }
}

/// The program stopped mid-run (in 0.02 ms slices) or after it, its state
/// changed one field at a time, written, restored and run on.
pub fn snapshot(src: &str, seed: u64) -> Option<String> {
    let mut rng = Rng::new(seed ^ 0x5A5A_1234);
    let mut e = Engine::new();
    e.set_syntax(false, false);
    // stop somewhere inside the program: a suspended snapshot has frames,
    // a stack and handlers, which an idle one has not
    let mid = rng.chance(70);
    e.set_slice(if mid { 0.02 } else { 20.0 });
    e.eval(src);
    if mid {
        let n = rng.below(40);
        for _ in 0..n {
            if !e.is_suspended() {
                break;
            }
            e.resume();
        }
    } else {
        run_budget(&mut e, 1500);
    }
    let what = mutate_vm(&mut e, &mut rng);
    // writing the damaged state may itself fail (we broke the heap by
    // hand): that is not a finding
    let text = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.snapshot())) {
        Ok(t) => t,
        Err(_) => return None,
    };
    if text.is_empty() {
        return None;
    }
    // half of the time also damage the text itself: a number token
    // replaced or made huge (the format's varints, unit::put_num)
    let text = if rng.chance(50) { mutate_text(&text, &mut rng) } else { text };
    stage::set("hostile-snapshot restore");
    let mut e2 = Engine::from_snapshot(&text);
    if !e2.error.is_empty() {
        return None;
    }
    stage::set("hostile-snapshot run");
    e2.set_slice(5.0);
    run_budget(&mut e2, 500);
    if !e2.is_suspended() {
        e2.eval("var __h = [typeof v0, typeof f0, Object.keys(globalThis).length]; JSON.stringify(__h)");
        run_budget(&mut e2, 300);
        let _ = e2.snapshot();
    }
    let _ = what;
    None
}

/// One field of the engine state set to a boundary value.
fn mutate_vm(e: &mut Engine, rng: &mut Rng) -> String {
    let v = BOUNDARY[rng.below(BOUNDARY.len())];
    let vm = &mut e.vm;
    let nf = vm.frames.len();
    let no = vm.objs.len();
    match rng.below(10) {
        0 => {
            vm.free_list.push(v as _);
            format!("free_list += {}", v)
        }
        1 if nf > 0 => {
            let i = rng.below(nf);
            match rng.below(5) {
                0 => vm.frames[i].pc = v as _,
                1 => vm.frames[i].bp = v as _,
                2 => vm.frames[i].env = v as _,
                3 => vm.frames[i].fobj = v as _,
                _ => vm.frames[i].ret = v as _,
            }
            format!("frame {} field = {}", i, v)
        }
        2 if !vm.handlers.is_empty() => {
            let i = rng.below(vm.handlers.len());
            match rng.below(3) {
                0 => vm.handlers[i].sp = v as _,
                1 => vm.handlers[i].catch_pc = v as _,
                _ => vm.handlers[i].env = v as _,
            }
            format!("handler {} = {}", i, v)
        }
        3 if !vm.stack.is_empty() => {
            let i = rng.below(vm.stack.len());
            vm.stack[i] = cerxes::value::Val::Obj(v as _);
            format!("stack {} = Obj({})", i, v)
        }
        4 | 5 if no > 0 => {
            let i = rng.below(no);
            if vm.objs[i].free {
                return String::from("none");
            }
            match rng.below(4) {
                0 => vm.objs[i].proto = v as _,
                1 => vm.objs[i].shape = v as _,
                2 => vm.objs[i].env = v as _,
                _ => vm.objs[i].func = v as _,
            }
            format!("obj {} field = {}", i, v)
        }
        6 | 7 if !vm.protos.is_empty() => {
            let p = rng.below(vm.protos.len());
            let n = vm.protos[p].code.len();
            if n == 0 {
                vm.protos[p].nslots = v as _;
                return format!("proto {} nslots = {}", p, v);
            }
            let i = rng.below(n);
            let op = &mut vm.protos[p].code[i];
            match rng.below(3) {
                0 => op.a = v as i32,
                1 => op.b = v as i32,
                _ => op.code = rng.below(256) as i32,
            }
            format!("proto {} op {} = {}", p, i, v)
        }
        8 => {
            vm.global = v as _;
            format!("global = {}", v)
        }
        _ => {
            let k = rng.below(3);
            match k {
                0 => vm.array_proto = v as _,
                1 => vm.object_proto = v as _,
                _ => vm.function_proto = v as _,
            }
            format!("proto id {} = {}", k, v)
        }
    }
}

/// A varint of the text (unit::put_num: '0'..'O' ends a number,
/// 'P'..'o' continues it) replaced or made huge.
fn mutate_text(text: &str, rng: &mut Rng) -> String {
    let mut b: Vec<char> = text.chars().collect();
    for _ in 0..1 + rng.below(3) {
        // find a number's last char near a random place
        let start = rng.below(b.len());
        let mut i = start;
        let mut found = false;
        for _ in 0..64 {
            if i >= b.len() {
                break;
            }
            let c = b[i] as u32;
            if (48..80).contains(&c) {
                found = true;
                break;
            }
            i += 1;
        }
        if !found {
            continue;
        }
        match rng.below(3) {
            0 => b[i] = char::from_u32(48 + rng.below(32) as u32).unwrap_or('0'),
            1 => {
                // bigger: continuation digits in front
                for _ in 0..1 + rng.below(6) {
                    b.insert(i, 'o');
                }
            }
            _ => b[i] = '1', // -1 in the zigzag code
        }
    }
    b.into_iter().collect()
}

/// A code-cache unit (unit::encode) with one opcode or operand changed,
/// read back and run, as a host that keeps compiled scripts would.
pub fn unit(src: &str, seed: u64, regs: bool) -> Option<String> {
    let mut rng = Rng::new(seed ^ 0xC0DE);
    let mut e = Engine::new();
    e.vm.regs = regs;
    e.set_syntax(false, false);
    let u = e.compile(src);
    if !e.error.is_empty() || u.entry < 0 {
        return None;
    }
    let text = cerxes::unit::encode(&u, src);
    if text.is_empty() {
        return None;
    }
    let text = if rng.chance(30) {
        mutate_text(&text, &mut rng)
    } else {
        let mut u2 = cerxes::unit::decode(&text, src);
        if u2.entry < 0 || u2.protos.is_empty() {
            return None;
        }
        for _ in 0..1 + rng.below(2) {
            let pi = rng.below(u2.protos.len());
            let n = u2.protos[pi].code.len();
            if n == 0 {
                continue;
            }
            let oi = rng.below(n);
            let v = BOUNDARY[rng.below(BOUNDARY.len())] as i32;
            let op = &mut u2.protos[pi].code[oi];
            match rng.below(4) {
                0 => op.a = v,
                1 => op.b = v,
                2 => op.c = v,
                _ => op.code = rng.below(256) as i32,
            }
        }
        cerxes::unit::encode(&u2, src)
    };
    stage::set("hostile-unit decode");
    let u3 = cerxes::unit::decode(&text, src);
    if u3.entry < 0 {
        return None;
    }
    stage::set("hostile-unit run");
    e.set_slice(5.0);
    e.run(u3);
    run_budget(&mut e, 300);
    None
}

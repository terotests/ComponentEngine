// CErXes: the host's limit on an engine's memory (Engine::set_heap_limit),
// measured with a probe on this test's counting allocator. One test
// function, so no other test's allocations move the counter.
use cerxes::Engine;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

struct Counting;
static HELD: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        HELD.fetch_add(l.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        HELD.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        HELD.fetch_add(n as isize - l.size() as isize, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn held() -> f64 {
    HELD.load(Ordering::Relaxed) as f64
}

const MB: f64 = 1048576.0;

/// Scripts that keep taking memory, in plain code and under natives.
const HOGS: &[&str] = &[
    "var a = []; for (;;) a.push(1)",
    "var a = []; for (;;) a.push({ x: a.length })",
    "var s = 'x'; for (;;) s += s",
    "var m = new Map(); for (var i = 0; ; i++) m.set(i, 'v' + i)",
    "var a = []; for (;;) a.push(new Array(1000).fill(1))",
    "try { var a = []; for (;;) a.push('k' + a.length) } catch (e) { 'caught' } finally { 'finally' }",
    "JSON.stringify({ get a() { var b = []; for (;;) b.push([1, 2, 3]); } })",
    "[2, 1].sort(function () { var b = []; for (;;) b.push({}); })",
    "new Array(1e7).fill(0).map(function (_, i) { return { i: i }; }).length",
];

fn engine(regs: bool, limit: f64) -> Engine {
    let mut e = Engine::new();
    e.vm.regs = regs;
    e.set_syntax(false, false);
    e.set_heap_limit(limit);
    e
}

#[test]
fn heap_limit() {
    cerxes::builtins::set_mem_probe(held);

    // every hog stops near the limit, past every catch and finally
    for regs in [true, false] {
        for src in HOGS {
            let mut e = engine(regs, 64.0 * MB);
            let before = held();
            let t0 = Instant::now();
            let r = e.eval(src);
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let grew = (held() - before) / MB;
            assert!(e.heap_exceeded(), "regs {}: {} => {}", regs, src, r);
            assert!(r.starts_with("Uncaught Error: script heap limit exceeded"), "regs {}: {} => {}", regs, src, r);
            // one allocation is not refused: a string that doubles holds
            // the old copy and the new one at the check after it
            assert!(grew < 64.0 * 4.0, "regs {}: {} grew {} MB", regs, src, grew);
            assert!(ms < 20000.0, "regs {}: {} took {} ms", regs, src, ms);
            assert_eq!((e.vm.stops, e.vm.heap_stops), (1, 1), "regs {}: {}", regs, src);
        }
    }

    // under the limit nothing changes; what the calls keep is counted
    let mut e = engine(true, 64.0 * MB);
    assert_eq!(e.eval("var keep = []; for (var i = 0; i < 100000; i++) keep.push({ i: i }); keep.length"), "100000");
    assert!(!e.timed_out());
    let used = e.heap_used() / MB;
    assert!(used > 1.0 && used < 64.0, "heap_used {} MB", used);

    // stopped, the engine keeps what it grew to (its object table does not
    // shrink), so the host drops it; the stop leaves it consistent: plain
    // code that takes no memory still runs
    assert!(e.eval("for (;;) keep.push({ i: keep.length })").starts_with("Uncaught Error: script heap limit exceeded"));
    assert!(e.heap_used() > 64.0 * MB);
    assert_eq!(e.eval("keep = null; 1 + 1"), "2");
    drop(e);

    // the time limit still says which limit it was
    let mut t = engine(true, 64.0 * MB);
    t.set_time_limit(50.0);
    let r = t.eval("for (;;) {}");
    assert!(r.starts_with("Uncaught Error: script time limit exceeded"), "{}", r);
    assert!(t.timed_out() && !t.heap_exceeded());
    assert_eq!((t.vm.stops, t.vm.heap_stops), (1, 0));

    // no limit: a big heap is fine
    let mut u = engine(true, 0.0);
    assert_eq!(u.eval("var b = []; for (var i = 0; i < 2000000; i++) b.push(i); b.length"), "2000000");
}

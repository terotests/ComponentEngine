// A script driven by host calls only (`Engine::call`, as the playground
// calls `__frame` once a frame) must still be collected: before
// `Vm::call_from_host` the collector never ran inside such a call, and the
// EVG demo's heap grew until the page ran out of memory.
use cer::Engine;

#[test]
fn host_calls_collect() {
    let mut e = Engine::new();
    e.eval("var kept = []; function frame() { var a = []; for (var i = 0; i < 2000; i++) a.push({ i: i, s: 'x' + i }); kept.push(a[1999]); return a.length; }");
    assert!(e.error.is_empty(), "{}", e.error);
    let runs = e.gc_runs();
    for _ in 0..400 {
        assert_eq!(e.call("frame"), "2000");
    }
    assert!(e.gc_runs() > runs, "no collection during 400 host calls");
    // 800 000 objects were made; what survives is `kept` and the engine
    assert!(e.heap_size() < 450000, "heap {} after the calls", e.heap_size());
    // what the script keeps survived every collection
    assert_eq!(e.eval("kept.length + ':' + kept[0].i + ':' + kept[399].s"), "400:1999:x1999");
}

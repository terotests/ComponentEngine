// __hostSync(text): the embedder's synchronous answer to a script
// (Vm::set_host_sync). A script calls it in the middle of its work and goes
// on with the host's text; without a host it throws a TypeError.
use cerxes::vm::HostSync;
use cerxes::Engine;

struct Echo {
    calls: i64,
}

impl HostSync for Echo {
    fn call(&mut self, request: &str) -> String {
        self.calls += 1;
        format!("{{\"echo\":{},\"n\":{}}}", request, self.calls)
    }
}

#[test]
fn host_answers_synchronously() {
    let mut e = Engine::new();
    e.vm.set_host_sync(Box::new(Echo { calls: 0 }));
    let r = e.eval("var a = JSON.parse(__hostSync(JSON.stringify({m: 'x'}))); var b = JSON.parse(__hostSync('2')); a.echo.m + ' ' + a.n + ' ' + b.echo + ' ' + b.n");
    assert_eq!(e.error, "");
    assert_eq!(r, "x 1 2 2");
}

#[test]
fn no_host_throws() {
    let mut e = Engine::new();
    let r = e.eval("var t = ''; try { __hostSync('x'); } catch (err) { t = err instanceof TypeError ? 'TypeError' : 'other'; } t");
    assert_eq!(r, "TypeError");
}

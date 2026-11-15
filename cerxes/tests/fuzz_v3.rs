// CErXes: what fuzzer v3's second round fixed (other engines' tests):
// allocation failures turned into RangeErrors, conformance, BigInt speed.
use cerxes::Engine;
use std::time::Instant;

fn run(regs: bool, src: &str) -> String {
    let mut e = Engine::new();
    e.vm.regs = regs;
    e.set_syntax(false, false);
    e.eval(src)
}

fn both(src: &str, want: &str) {
    for regs in [true, false] {
        assert_eq!(run(regs, src), want, "regs {}: {}", regs, src);
    }
}

#[test]
fn big_requests_are_range_errors() {
    // JSC regexp-prototype-exec-on-too-long-rope: 8 bytes a unit of 400 M
    both("try { /x/.exec('x'.repeat(400000000)); 'no' } catch (e) { String(e) }", "RangeError: Out of memory");
    // Hermes regexp-exec-fast-path: an empty group repeated 10^9 times
    both("try { 'b'.search(/(){999999999}/); 'no' } catch (e) { e.name }", "RangeError");
    // JSC unlinked-metadata-table-finalize-overflow: a 178 MB source
    both("try { new Function('a', 'a();'.repeat(44739242)); 'no' } catch (e) { e.name }", "RangeError");
    // SpiderMonkey bug 2017678: 2000 keys of 2 MB
    both("var n = 'a'.repeat(2000000), a = []; for (var i = 0; i < 300; i++) { var o = {}; o[n] = 1; a.push(o); } try { JSON.stringify(a); 'no' } catch (e) { e.name }", "RangeError");
    // Hermes array-join-truncation: length 2^32 + 1
    both("try { Array.prototype.join.call({ length: 4294967297 }); 'no' } catch (e) { e.name }", "RangeError");
}

#[test]
fn conformance_from_the_seeds() {
    both("var o = { x: 1 }; o[Symbol.unscopables] = { x: true }; var x = 'outer'; with (o) { x }", "outer");
    both("var k = 'outer'; with ([]) { typeof keys }", "undefined");
    both("var hit = 0; Object.defineProperty(Array.prototype, 3, { set: function () { hit++; }, configurable: true }); var a = [0, 1, 2]; a[3] = 9; var b = [0, 1, 2]; b.push(5); delete Array.prototype[3]; [hit, a.length, a.hasOwnProperty(3), b.length].join()", "2,3,false,4");
    both("var s = new String('ab'); try { Object.defineProperty(s, 'length', { value: 5 }); 'no' } catch (e) { e.name }", "TypeError");
    both("var s = new String('ab'); Object.defineProperty(s, 'length', { value: 2 }); Object.defineProperty(s, 0, { value: 'a', enumerable: true }); try { Object.defineProperty(s, 1, { value: 'z' }); 'no' } catch (e) { e.name }", "TypeError");
    both("[/(?<=\\u{1F600})a/u.test('\\u{1F600}a'), /(?<=\\u{10400})x/iu.test('\\u{10428}x'), /(?<=\\uDE00)x/u.test('\\u{1F600}x')].join()", "true,true,false");
    both("var re = Object.freeze(/a/g); try { 'aa'.replace(re, 'b'); 'no' } catch (e) { e.name }", "TypeError");
    both("var n = 0, re = /a/; re.lastIndex = { valueOf: function () { ++n; return 0; } }; re.exec('a'); re.test('a'); 'a'.match(re); 'a'.replace(re, 'b'); 'a'.search(re); n", "4");
}

#[test]
fn bigint_on_big_numbers() {
    let t0 = Instant::now();
    // q * y + r == x on 2^20-bit and 2^19-bit numbers (Newton division),
    // a 2^19-bit square (Karatsuba), hex both ways (divide and conquer)
    both("var x = (1n << 1048576n) - 12345n, y = (1n << 524288n) + 98765n; var q = x / y, r = x % y; [q * y + r === x, r < y, BigInt('0x' + y.toString(16)) === y, (y * y) >> 1048576n].join()", "true,true,true,1");
    assert!(t0.elapsed().as_secs_f64() < 30.0, "took {} s", t0.elapsed().as_secs_f64());
    // the same arithmetic on sizes around the thresholds, against itself
    both("var ok = true, s = 7; function r() { s = (s * 1103515245 + 12345) % 2147483648; return s; } function big(n) { var t = ''; for (var i = 0; i < n; i++) t += (r() % 65536).toString(16).padStart(4, '0'); return BigInt('0x1' + t); } [3, 39, 41, 199, 201, 450].forEach(function (n) { [2, 40, 200, 230].forEach(function (m) { var a = big(n + m), b = big(m); var q = a / b, rr = a % b; ok = ok && q * b + rr === a && rr < b && (a & b) + (a | b) === a + b && ((a * b) >> 7n) === (a * b) / 128n; }); }); ok", "true");
}

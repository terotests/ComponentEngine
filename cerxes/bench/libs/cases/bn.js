import BN from "bn.js";
function fact(n) { let r = new BN(1); for (let i = 2; i <= n; i++) r = r.muln(i); return r; }
const p = new BN("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2f", 16);
export default {
  check: {
    fact: () => fact(100).toString(10),
    ops: () => [new BN("123456789012345678901234567890").div(new BN(97)).toString(), new BN(-5).umod(new BN(3)).toString(), new BN(1).shln(130).toString(16), new BN("deadbeef", 16).xor(new BN(0xffff)).toString(16), new BN(10).pow(new BN(30)).sqr().bitLength()],
    modpow: () => { const red = BN.red(p); return new BN(3).toRed(red).redPow(new BN(12345678)).fromRed().toString(16); },
    inv: () => new BN(12345).invm(p).toString(16),
  },
  bench: { fact: () => fact(400).bitLength(), modpow: () => { const red = BN.red(p); return new BN(7).toRed(red).redPow(p.subn(2)).fromRed().bitLength(); } },
};

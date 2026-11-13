import Big from "big.js";
function series(n) { let s = new Big(0); for (let k = 1; k <= n; k++) s = s.plus(new Big(1).div(k * k)); return s; }
export default {
  check: {
    arith: () => [new Big("0.1").plus("0.2").toString(), new Big("123.456").times("-7.89").toFixed(5), new Big(2).sqrt().toPrecision(30), new Big("1e-7").toString(), new Big(10).pow(25).toExponential(3)],
    cmp: () => [new Big("1.000").eq(1), new Big("-0").toString(), new Big(7).mod(3).toString(), new Big("12.345").round(1, Big.roundHalfEven).toString()],
    series: () => series(200).toFixed(18),
    error: () => { try { new Big("abc"); return "ok"; } catch (e) { return e.message; } },
  },
  bench: { series: () => series(300).toFixed(5) },
};

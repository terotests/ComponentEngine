import Decimal from "decimal.js";
const D = Decimal.clone({ precision: 50, rounding: Decimal.ROUND_HALF_EVEN });
export default {
  check: {
    arith: () => [new D("0.1").plus("0.2").toString(), new D(1).div(7).toString(), new D(2).sqrt().toString(), new D("123456789.123456789").times("987654321.987654321").toString(), new D(10).pow(-20).toExponential(), new D("1e1000").plus(1).minus("1e1000").toString()],
    transcendental: () => [D.exp(1).toString(), D.ln(10).toString(), D.sin(1).toString(), D.atan(1).times(4).toString(), new D(2).pow("0.5").toFixed(30)],
    format: () => [new D("1234.5678").toFixed(2), new D("1234.5678").toSignificantDigits(3).toString(), new D("-0").isNegative(), new D(255).toHexadecimal(), new D("0.000123").toFraction(), new D("12.5").round().toString(), new D("1e-7").toString()],
  },
  bench: {
    series: () => { let s = new D(0); for (let k = 0; k < 200; k++) s = s.plus(new D(1).div(new D(16).pow(k)).times(new D(4).div(8 * k + 1).minus(new D(2).div(8 * k + 4)).minus(new D(1).div(8 * k + 5)).minus(new D(1).div(8 * k + 6)))); return s.toString().length; },
    transcendental: () => D.exp(D.ln(3).times("1.5")).toString().length,
  },
};

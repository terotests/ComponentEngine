import BigNumber from "bignumber.js";
const B = BigNumber.clone({ DECIMAL_PLACES: 40 });
export default {
  check: {
    arith: () => [new B("0.1").plus("0.2").toString(), new B(1).div(7).toString(), new B(2).sqrt().toFixed(40), new B("123456789.123456789").times("987654321.987654321").toString(), new B(2).pow(200).toString(), new B("1e-30").toString(), new B(-7).mod(3).toString()],
    format: () => [new B("1234567.891").toFormat(2), new B(255).toString(16), new B("ff.8", 16).toString(), new B("0.000123").toFraction(), new B(12.5).integerValue(BigNumber.ROUND_HALF_EVEN).toString(), new B(NaN).isNaN(), new B("-0").isNegative()],
  },
  bench: {
    factorial: () => { let f = new B(1); for (let i = 2; i <= 300; i++) f = f.times(i); return f.toString().length; },
    division: () => { let s = new B(0); for (let k = 1; k < 200; k++) s = s.plus(new B(1).div(k * k)); return s.toString().length; },
  },
};

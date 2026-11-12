import { v5, v3, validate, parse, stringify, version } from "uuid";
const NS = "6ba7b810-9dad-11d1-80b4-00c04fd430c8";
export default {
  check: {
    v5: () => [v5("hello", NS), v5("", NS), v5("äö€", NS)],
    v3: () => v3("hello", NS),
    parse: () => [Array.from(parse(NS)), stringify(parse(NS)), version(v5("x", NS)), validate("nope"), validate(NS)],
  },
  bench: { v5: () => { let s = ""; for (let i = 0; i < 200; i++) s = v5("name" + i, NS); return s; } },
};

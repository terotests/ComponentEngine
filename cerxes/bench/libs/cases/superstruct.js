import { object, string, number, array, optional, enums, refine, union, literal, validate, is, assert } from "superstruct";
const Email = refine(string(), "email", (v) => /^[^@]+@[^@]+$/.test(v));
const User = object({ id: number(), name: string(), email: Email, role: enums(["admin", "user"]), tags: array(string()), address: optional(object({ city: string(), zip: union([string(), number()]) })), kind: literal("user") });
const data = Array.from({ length: 500 }, (_x, i) => ({ id: i, name: "N" + i, email: i % 9 ? "a" + i + "@x.y" : "bad", role: i % 2 ? "admin" : "user", tags: ["a"], address: i % 3 ? { city: "C", zip: i } : undefined, kind: "user" }));
export default {
  check: {
    valid: () => data.slice(0, 20).map((d) => is(d, User)),
    errors: () => { const [e] = validate({ id: "x", name: 1, email: "q", role: "x", tags: [1], kind: "u" }, User); return e.failures().map((f) => f.path.join(".") + ":" + f.type); },
    assert: () => { try { assert({ id: 1 }, User); return "ok"; } catch (e) { return e.message; } },
  },
  bench: { validate: () => data.filter((d) => is(d, User)).length },
};

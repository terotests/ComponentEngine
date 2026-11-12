import { z } from "zod";
const User = z.object({ id: z.number().int().positive(), email: z.string().email(), name: z.string().min(2).max(40), role: z.enum(["admin", "user"]), tags: z.array(z.string()).max(5), born: z.coerce.date().optional(), address: z.object({ city: z.string(), zip: z.string().regex(/^\d{5}$/) }).partial(), meta: z.record(z.string(), z.union([z.number(), z.boolean()])) }).strict();
const Shape = z.discriminatedUnion("kind", [z.object({ kind: z.literal("c"), r: z.number() }), z.object({ kind: z.literal("s"), a: z.number() })]).transform((s) => (s.kind === "c" ? Math.PI * s.r * s.r : s.a * s.a));
const good = Array.from({ length: 200 }, (_, i) => ({ id: i + 1, email: `u${i}@ex.com`, name: "name" + i, role: i % 2 ? "admin" : "user", tags: ["a", "b"], born: "1990-01-0" + ((i % 9) + 1), address: { city: "H", zip: "0010" + (i % 10) }, meta: { a: 1, b: true } }));
const bad = { id: -1.5, email: "x", name: "a", role: "root", tags: [1], address: { zip: "1" }, meta: { q: "s" }, extra: 1 };
export default {
  check: {
    good: () => User.parse(good[3]),
    bad: () => { const r = User.safeParse(bad); return r.success ? "ok?" : r.error.issues.map((i) => i.path.join(".") + ":" + i.code); },
    union: () => [Shape.parse({ kind: "c", r: 2 }), Shape.parse({ kind: "s", a: 3 }), Shape.safeParse({ kind: "q" }).success],
    misc: () => [z.string().trim().toLowerCase().parse("  HeLLo "), z.number().catch(7).parse("x"), z.tuple([z.string(), z.number()]).rest(z.boolean()).parse(["a", 1, true, false]), z.bigint().safeParse(1).success, z.array(z.number()).nonempty().safeParse([]).success, z.string().uuid().safeParse("123e4567-e89b-12d3-a456-426614174000").success],
    jsonSchema: () => z.toJSONSchema(User.omit({ born: true })),
  },
  bench: {
    parseGood: () => { let n = 0; for (const u of good) n += User.parse(u).id; return n; },
    parseBad: () => { let n = 0; for (let i = 0; i < 100; i++) n += User.safeParse(bad).error.issues.length; return n; },
    defineSchema: () => z.object({ a: z.string(), b: z.array(z.object({ c: z.number(), d: z.enum(["x", "y"]) })), e: z.union([z.string(), z.number()]).optional() }).parse({ a: "s", b: [{ c: 1, d: "x" }] }).a,
  },
};

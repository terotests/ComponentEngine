import { pipe, flow } from "fp-ts/function";
import * as O from "fp-ts/Option";
import * as E from "fp-ts/Either";
import * as A from "fp-ts/Array";
import * as RA from "fp-ts/ReadonlyArray";
import * as T from "fp-ts/Task";
import * as TE from "fp-ts/TaskEither";
import * as N from "fp-ts/number";
import * as S from "fp-ts/string";
import * as Ord from "fp-ts/Ord";
import * as M from "fp-ts/Map";
import * as Mon from "fp-ts/Monoid";
import * as SG from "fp-ts/Semigroup";
import * as St from "fp-ts/State";
import * as R from "fp-ts/Record";
const parseNum = (s) => { const n = Number(s); return isNaN(n) ? E.left("bad: " + s) : E.right(n); };
const people = Array.from({ length: 500 }, (_, i) => ({ name: "p" + ((i * 31) % 500), age: (i * 7) % 90 }));
const byAge = pipe(N.Ord, Ord.contramap((p) => p.age));
const byName = pipe(S.Ord, Ord.contramap((p) => p.name));
export default {
  check: {
    option: () => [pipe(O.some(3), O.map((x) => x * 2), O.chain((x) => (x > 5 ? O.some(x) : O.none)), O.getOrElse(() => 0)), pipe(O.fromNullable(null), O.alt(() => O.some("alt"))), pipe([O.some(1), O.none, O.some(3)], A.compact), O.sequenceArray([O.some(1), O.some(2)])],
    either: () => [pipe(["1", "2", "x", "4"], A.traverse(E.Applicative)(parseNum)), pipe(["1", "2", "3"], E.traverseArray(parseNum), E.map(A.reduce(0, (a, b) => a + b))), pipe(E.right(1), E.chainW(() => E.left({ e: 1 })), E.mapLeft((e) => e.e + 1)), pipe(["a", "1", "b"], A.map(parseNum), A.separate)],
    ord: () => [pipe(people, A.sortBy([byAge, byName]), A.takeLeft(4)), pipe(people, A.uniq({ equals: (a, b) => a.age === b.age })).length, Mon.concatAll(Mon.struct({ n: N.MonoidSum, s: S.Monoid }))([{ n: 1, s: "a" }, { n: 2, s: "b" }])],
    task: async () => { const t = pipe(TE.right(2), TE.chain((x) => TE.tryCatch(() => Promise.resolve(x * 10), String)), TE.chainFirst((x) => (x > 100 ? TE.left("big") : TE.right(x))), TE.map((x) => x + 1)); const seq = await pipe([1, 2, 3], A.map((x) => T.delay(30 - x * 10)(T.of(x))), T.sequenceArray)(); return [await t(), seq, await TE.left("e")()]; },
    state: () => { const inc = St.modify((n) => n + 1); const prog = pipe(inc, St.chain(() => inc), St.chain(() => St.get()), St.map((n) => n * 10)); return prog(5); },
    records: () => [pipe({ a: 1, b: 2, c: 3 }, R.filter((n) => n > 1), R.map((n) => n * 2)), pipe(people.slice(0, 20), RA.reduce(new Map(), (m, p) => M.upsertAt(N.Eq)(p.age % 5, p.name)(m)), M.keys(N.Ord)), SG.concatAll(SG.max(N.Ord))(0)([3, 9, 2])],
  },
  bench: {
    sortUniq: () => pipe(people, A.sortBy([byAge, byName]), A.uniq({ equals: (a, b) => a.name === b.name }), A.map((p) => p.age), A.reduce(0, (a, b) => a + b)),
    eitherTraverse: () => { const xs = Array.from({ length: 500 }, (_, i) => String(i)); return pipe(xs, E.traverseArray(parseNum), E.map((a) => a.length), E.getOrElse(() => -1)); },
    optionChain: () => { let s = 0; for (let i = 0; i < 2000; i++) s += pipe(O.some(i), O.filter((x) => x % 3 !== 0), O.map((x) => x * 2), O.getOrElse(() => 0)); return s; },
  },
};

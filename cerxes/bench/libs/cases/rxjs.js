import { from, of, range, Subject, BehaviorSubject, ReplaySubject, merge, concat, zip, combineLatest, timer, interval, defer, throwError, firstValueFrom, lastValueFrom, asyncScheduler, queueScheduler, scheduled } from "rxjs";
import { map, filter, scan, reduce, mergeMap, switchMap, concatMap, exhaustMap, debounceTime, throttleTime, bufferCount, take, takeUntil, distinctUntilChanged, groupBy, toArray, catchError, retry, share, shareReplay, startWith, pairwise, delay, tap, windowCount, auditTime, sampleTime, timeout, finalize } from "rxjs/operators";
const collect = (obs) => lastValueFrom(obs.pipe(toArray()));
export default {
  check: {
    sync: () => collect(range(1, 50).pipe(filter((x) => x % 3 === 0), map((x) => x * x), scan((a, x) => a + x, 0), pairwise(), bufferCount(3), take(4))),
    groups: () => collect(from(["a1", "b2", "a3", "c4", "b5", "a6"]).pipe(groupBy((s) => s[0]), mergeMap((g) => g.pipe(reduce((a, s) => a + s.slice(1), g.key + ":")))) ),
    timeOps: () => collect(merge(timer(0, 10).pipe(take(10), map((i) => "t" + i)), timer(25, 30).pipe(take(3), map((i) => "u" + i))).pipe(debounceTime(5), distinctUntilChanged())),
    switchConcat: () => Promise.all([collect(interval(10).pipe(take(4), switchMap((i) => timer(15).pipe(map(() => "s" + i))))), collect(of(30, 10, 20).pipe(concatMap((d) => timer(d).pipe(map(() => d))))), collect(interval(5).pipe(take(10), exhaustMap((i) => timer(12).pipe(map(() => "e" + i)))))]),
    subjects: () => { const log = []; const s = new Subject(); const b = new BehaviorSubject(0); const r = new ReplaySubject(2); s.subscribe((v) => log.push("s" + v)); [1, 2, 3].forEach((v) => { s.next(v); b.next(v); r.next(v); }); b.subscribe((v) => log.push("b" + v)); r.subscribe((v) => log.push("r" + v)); s.complete(); s.next(9); return log; },
    errors: () => { let n = 0; return collect(defer(() => (++n < 3 ? throwError(() => new Error("e" + n)) : of("ok" + n))).pipe(retry(5), catchError((e) => of(e.message)))); },
    combine: () => collect(combineLatest([timer(0, 20).pipe(take(3)), timer(5, 30).pipe(take(2))]).pipe(map(([a, b]) => a + "/" + b))),
    shared: () => { let subs = 0; const src = defer(() => { subs++; return interval(10).pipe(take(3)); }).pipe(shareReplay(1)); return Promise.all([collect(src), collect(src)]).then((r) => [r, subs]); },
    zip: () => collect(zip(of(1, 2, 3), from("abc"), range(10, 5)).pipe(map((a) => a.join("")))),
  },
  bench: {
    syncPipeline: () => firstValueFrom(range(0, 5000).pipe(map((x) => x * 2), filter((x) => x % 3 !== 0), scan((a, x) => (a + x) % 1000003, 0), reduce((a, x) => a ^ x, 0))),
    subjects: () => { const s = new Subject(); let n = 0; for (let i = 0; i < 20; i++) s.pipe(map((x) => x + i), filter((x) => x & 1)).subscribe(() => n++); for (let i = 0; i < 500; i++) s.next(i); s.complete(); return n; },
    mergeMap: () => lastValueFrom(range(0, 300).pipe(mergeMap((i) => of(i, i + 1, i + 2).pipe(map((x) => x * 2))), toArray())).then((a) => a.length),
  },
};

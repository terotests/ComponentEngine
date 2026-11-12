import { observable, computed, autorun, reaction, action, runInAction, makeAutoObservable, when, configure, toJS } from "mobx";
configure({ enforceActions: "never" });
class Store { todos = []; filter = "all"; constructor() { makeAutoObservable(this); } get visible() { return this.todos.filter((t) => this.filter === "all" || (this.filter === "done") === t.done); } get stats() { return { n: this.todos.length, done: this.todos.filter((t) => t.done).length }; } add(title) { this.todos.push({ title, done: false }); } toggle(i) { this.todos[i].done = !this.todos[i].done; } }
export default {
  check: {
    reactions: () => { const s = new Store(); const log = []; const d1 = autorun(() => log.push("A" + s.visible.length + "/" + s.stats.done)); const d2 = reaction(() => s.stats.n, (n, prev) => log.push("R" + prev + ">" + n)); action(() => { s.add("a"); s.add("b"); })(); s.toggle(0); s.filter = "done"; runInAction(() => { s.add("c"); s.toggle(2); }); d1(); d2(); s.add("z"); return [log, toJS(s.todos)]; },
    computedCache: () => { let calls = 0; const o = observable({ a: 1, b: 2 }); const c = computed(() => (calls++, o.a + o.b)); const log = []; const d = autorun(() => log.push(c.get())); o.a = 1; o.b = 3; o.a = 5; d(); return [log, calls]; },
    collections: () => { const m = observable.map({ a: 1 }); const st = observable.set([1]); const arr = observable([3, 1, 2]); const log = []; autorun(() => log.push([m.size, st.size, arr.slice().sort().join("")].join(":"))); m.set("b", 2); st.add(2); arr.push(0); arr.splice(0, 1); return log; },
    when: async () => { const o = observable({ ready: false }); setTimeout(() => runInAction(() => (o.ready = true)), 20); await when(() => o.ready); return o.ready; },
    structural: () => { const o = observable({ v: { x: 1 } }); let n = 0; reaction(() => ({ ...o.v }), () => n++, { equals: (a, b) => JSON.stringify(a) === JSON.stringify(b) }); o.v = { x: 1 }; o.v = { x: 2 }; return n; },
  },
  bench: {
    store: () => { const s = new Store(); let k = 0; const d = autorun(() => { k += s.visible.length + s.stats.done; }); for (let i = 0; i < 200; i++) { s.add("t" + i); if (i % 3 === 0) s.toggle(i >> 1); } d(); return k; },
  },
};

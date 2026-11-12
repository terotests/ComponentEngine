import { reactive, ref, computed, effect, watch, shallowReactive, readonly, toRaw, isReactive, triggerRef, shallowRef, effectScope, stop } from "@vue/reactivity";
export default {
  check: {
    effects: () => { const s = reactive({ a: 1, list: [1, 2], nested: { b: 2 }, m: new Map([["k", 1]]), set: new Set() }); const log = []; const c = computed(() => s.a * 2 + s.list.length + s.nested.b + (s.m.get("k") || 0) + s.set.size); effect(() => log.push(c.value)); s.a = 2; s.list.push(3); s.nested.b = 10; s.m.set("k", 5); s.set.add(1); s.set.add(1); delete s.nested.b; return [log, isReactive(s.nested), toRaw(s).a]; },
    readonly: () => { const r = readonly(reactive({ a: { b: 1 } })); r.a.b = 2; return r.a.b; },
    scope: () => { const sc = effectScope(); const x = ref(0); const log = []; sc.run(() => { effect(() => log.push("e" + x.value)); }); x.value++; sc.stop(); x.value++; const sr = shallowRef({ v: 1 }); const r2 = []; effect(() => r2.push(sr.value.v)); sr.value.v = 2; triggerRef(sr); return [log, r2]; },
    arrays: () => { const a = reactive([3, 1, 2]); const log = []; effect(() => log.push(a.join(","))); a.sort(); a.reverse(); a.splice(1, 1, 9, 8); a.length = 2; const o = {}; const b = reactive([o]); return [log, b.includes(o), b.indexOf(o)]; },
  },
  bench: {
    effects: () => { const s = reactive({ items: Array.from({ length: 200 }, (_, i) => ({ v: i })) }); let total = 0; const c = computed(() => s.items.reduce((a, it) => a + it.v, 0)); const e = effect(() => { total = c.value; }); for (let i = 0; i < 200; i++) s.items[i].v++; stop(e); return total; },
  },
};

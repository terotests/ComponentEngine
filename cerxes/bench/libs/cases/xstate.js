import { createMachine, createActor, assign, setup, fromPromise, raise, sendTo, fromCallback } from "xstate";
const traffic = setup({ actions: { log: assign({ log: ({ context, event }) => [...context.log, event.type] }) }, guards: { isLong: ({ context }) => context.n > 2 } }).createMachine({
  id: "light", initial: "green", context: { n: 0, log: [] },
  states: {
    green: { on: { TIMER: { target: "yellow", actions: "log" } } },
    yellow: { on: { TIMER: { target: "red", actions: "log" } } },
    red: { initial: "walk", entry: assign({ n: ({ context }) => context.n + 1 }), states: { walk: { on: { PED: "wait" } }, wait: { on: { PED: "stop" } }, stop: { type: "final" } }, onDone: [{ guard: "isLong", target: "off" }, { target: "green" }], on: { TIMER: { target: "green", actions: "log" } } },
    off: { type: "final" },
  },
});
const fetcher = setup({ actors: { load: fromPromise(async ({ input }) => { await new Promise((r) => setTimeout(r, 20)); if (input.id < 0) throw new Error("neg"); return { id: input.id, v: input.id * 2 }; }) } }).createMachine({
  initial: "idle", context: { id: 0, data: null, err: null, tries: 0 },
  states: {
    idle: { on: { FETCH: { target: "loading", actions: assign({ id: ({ event }) => event.id }) } } },
    loading: { entry: assign({ tries: ({ context }) => context.tries + 1 }), invoke: { src: "load", input: ({ context }) => ({ id: context.id }), onDone: { target: "ok", actions: assign({ data: ({ event }) => event.output }) }, onError: { target: "idle", actions: assign({ err: ({ event }) => event.error.message }) } } },
    ok: { type: "final" },
  },
});
const parallel = createMachine({ type: "parallel", context: { c: 0 }, states: { bold: { initial: "off", states: { off: { on: { B: "on" } }, on: { on: { B: "off" } } } }, list: { initial: "none", states: { none: { on: { L: "bullets", N: "numbers" } }, bullets: { on: { L: "none" } }, numbers: { on: { N: "none" } } } } }, on: { "*": { actions: assign({ c: ({ context }) => context.c + 1 }) } } });
function runTraffic() { const a = createActor(traffic).start(); const seq = ["TIMER", "TIMER", "PED", "PED", "TIMER", "TIMER", "PED", "TIMER", "TIMER", "TIMER", "PED", "PED", "TIMER", "TIMER", "PED", "PED"]; const states = []; for (const t of seq) { a.send({ type: t }); states.push(JSON.stringify(a.getSnapshot().value)); } return [states, a.getSnapshot().context, a.getSnapshot().status]; }
export default {
  check: {
    traffic: runTraffic,
    parallel: () => { const a = createActor(parallel).start(); for (const t of ["B", "L", "N", "L", "N", "B", "B"]) a.send({ type: t }); const s = a.getSnapshot(); return [s.value, s.context, s.matches({ bold: "on" }), s.can({ type: "L" })]; },
    async: () => new Promise((res) => { const a = createActor(fetcher); const seen = []; a.subscribe((s) => seen.push(s.value)); a.start(); a.send({ type: "FETCH", id: -1 }); setTimeout(() => a.send({ type: "FETCH", id: 21 }), 50); a.subscribe({ complete: () => res([seen, a.getSnapshot().context]) }); }),
  },
  bench: {
    traffic: () => runTraffic()[0].length,
    parallel: () => { const a = createActor(parallel).start(); for (let i = 0; i < 300; i++) a.send({ type: "BLN"[i % 3] }); return a.getSnapshot().context.c; },
  },
};

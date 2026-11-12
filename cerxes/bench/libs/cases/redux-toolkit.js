import { configureStore, createSlice, createEntityAdapter, createSelector, createAsyncThunk, combineReducers, nanoid, createListenerMiddleware } from "@reduxjs/toolkit";
const todos = createEntityAdapter({ sortComparer: (a, b) => a.title.localeCompare(b.title) });
const fetchMore = createAsyncThunk("todos/fetchMore", async (n) => { await new Promise((r) => setTimeout(r, 10)); return Array.from({ length: n }, (_, i) => ({ id: "f" + i, title: "fetched " + i, done: false })); });
const slice = createSlice({ name: "todos", initialState: todos.getInitialState({ status: "idle", filter: "all" }), reducers: { add: todos.addOne, toggle(state, a) { const t = state.entities[a.payload]; t.done = !t.done; }, rename: { reducer(state, a) { todos.updateOne(state, { id: a.payload.id, changes: { title: a.payload.title } }); }, prepare(id, title) { return { payload: { id, title: title.trim() } }; } }, setFilter(state, a) { state.filter = a.payload; } }, extraReducers: (b) => { b.addCase(fetchMore.pending, (s) => { s.status = "loading"; }).addCase(fetchMore.fulfilled, (s, a) => { s.status = "idle"; todos.upsertMany(s, a.payload); }); } });
const counter = createSlice({ name: "counter", initialState: { v: 0 }, reducers: { inc: (s) => { s.v++; } } });
const sel = todos.getSelectors((s) => s.todos);
const selVisible = createSelector([sel.selectAll, (s) => s.todos.filter], (all, f) => all.filter((t) => f === "all" || (f === "done" ? t.done : !t.done)).map((t) => t.title));
function mk() { const lm = createListenerMiddleware(); const log = []; lm.startListening({ actionCreator: counter.actions.inc, effect: (a, api) => { log.push("inc:" + api.getState().counter.v); } }); const store = configureStore({ reducer: combineReducers({ todos: slice.reducer, counter: counter.reducer }), middleware: (g) => g().prepend(lm.middleware) }); return { store, log }; }
export default {
  check: {
    flow: async () => { const { store, log } = mk(); const A = slice.actions; ["b", "a", "c"].forEach((t, i) => store.dispatch(A.add({ id: "t" + i, title: t, done: false }))); store.dispatch(A.toggle("t1")); store.dispatch(A.rename("t2", "  zed  ")); store.dispatch(counter.actions.inc()); store.dispatch(counter.actions.inc()); const p = store.dispatch(fetchMore(3)); const mid = store.getState().todos.status; await p; store.dispatch(A.setFilter("open")); return [mid, store.getState().todos.ids, selVisible(store.getState()), store.getState().counter, log, A.rename("x", " y ").payload]; },
    frozen: () => { const { store } = mk(); store.dispatch(slice.actions.add({ id: "q", title: "q", done: false })); try { store.getState().todos.entities.q.done = true; return "mutable"; } catch (e) { return e.constructor.name; } },
    memo: () => { const { store } = mk(); const a = selVisible(store.getState()); store.dispatch(counter.actions.inc()); return [a === selVisible(store.getState()), selVisible.recomputations()]; },
    nanoid: () => nanoid().length,
  },
  bench: {
    dispatch: () => { const { store } = mk(); for (let i = 0; i < 300; i++) { store.dispatch(slice.actions.add({ id: "t" + i, title: "title " + ((i * 37) % 300), done: false })); if (i % 3 === 0) store.dispatch(slice.actions.toggle("t" + (i >> 1))); } return selVisible(store.getState()).length; },
  },
};

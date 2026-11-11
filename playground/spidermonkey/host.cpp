// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The playground's SpiderMonkey engine: Mozilla's engine (MPL-2.0) built for
// wasm32-wasi by the Bytecode Alliance (spidermonkey-wasi-embedding, the
// static library StarlingMonkey links), with this host around it. There is
// no JIT in WebAssembly; the engine runs its Portable Baseline Interpreter,
// which has inline caches (`pbl` below turns it on).
//
// One JSContext is kept in the instance and a run gets the current global:
//
//   sm_alloc(n) / sm_free(p)   room for the script text
//   sm_eval(p, n, pbl)         0 when the script ran, 1 when it threw;
//                              pbl < 0 runs the generic interpreter
//   sm_error()                 the error's text after a 1
//   sm_reset()                 a new global: a fresh realm
//
// print and console.log write to stdout (WASI fd_write).
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>

#include <jsapi.h>
#include <jsfriendapi.h>

#include <js/CompilationAndEvaluation.h>
#include <js/Conversions.h>
#include <js/Exception.h>
#include <js/GCAPI.h>
#include <js/Initialization.h>
#include <js/RealmOptions.h>
#include <js/SourceText.h>
#include <mozilla/Utf8.h>

static JSContext *cx = nullptr;
static JS::PersistentRootedObject *global = nullptr;
static std::string error;

static const JSClassOps globalOps = {
    nullptr, nullptr, nullptr, nullptr, nullptr, nullptr, nullptr,
    nullptr, nullptr, JS_GlobalObjectTraceHook};
static const JSClass globalClass = {
    "global", JSCLASS_GLOBAL_FLAGS, &globalOps};

static bool text(JSContext *cx, JS::HandleValue v, std::string &out) {
  JS::RootedString s(cx, JS::ToString(cx, v));
  if (!s)
    return false;
  JS::UniqueChars c = JS_EncodeStringToUTF8(cx, s);
  if (!c)
    return false;
  out += c.get();
  return true;
}

static bool print(JSContext *cx, unsigned argc, JS::Value *vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  std::string line;
  for (unsigned i = 0; i < args.length(); i++) {
    if (i)
      line += ' ';
    if (!text(cx, args[i], line))
      return false;
  }
  line += '\n';
  fwrite(line.data(), 1, line.size(), stdout);
  fflush(stdout);
  args.rval().setUndefined();
  return true;
}

static bool now(JSContext *, unsigned argc, JS::Value *vp) {
  JS::CallArgs args = JS::CallArgsFromVp(argc, vp);
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  args.rval().setDouble(ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6);
  return true;
}

static bool install() {
  JS::RealmOptions options;
  JS::RootedObject g(
      cx,
      JS_NewGlobalObject(
          cx, &globalClass, nullptr, JS::FireOnNewGlobalHook, options));
  if (!g)
    return false;
  if (!global)
    global = new JS::PersistentRootedObject(cx);
  *global = g;
  JSAutoRealm ar(cx, g);
  if (!JS::InitRealmStandardClasses(cx) ||
      !JS_DefineFunction(cx, g, "print", print, 0, 0))
    return false;
  JS::RootedObject console(cx, JS_NewPlainObject(cx));
  JS::RootedObject perf(cx, JS_NewPlainObject(cx));
  if (!console || !perf)
    return false;
  if (!JS_DefineFunction(cx, console, "log", print, 0, 0) ||
      !JS_DefineFunction(cx, perf, "now", now, 0, 0) ||
      !JS_DefineProperty(cx, g, "console", console, 0) ||
      !JS_DefineProperty(cx, g, "performance", perf, 0))
    return false;
  return true;
}

/// The pending exception as "Name: message", and cleared.
static void take_error() {
  error = "error";
  JS::RootedValue exn(cx);
  if (!JS_GetPendingException(cx, &exn))
    return;
  JS_ClearPendingException(cx);
  std::string s;
  if (text(cx, exn, s))
    error = s;
  else
    JS_ClearPendingException(cx);
}

extern "C" {

__attribute__((export_name("sm_alloc"))) char *sm_alloc(size_t n) {
  return static_cast<char *>(malloc(n ? n : 1));
}

__attribute__((export_name("sm_free"))) void sm_free(char *p) {
  free(p);
}

__attribute__((export_name("sm_reset"))) int sm_reset() {
  if (!cx) {
    if (!JS_Init())
      return 1;
    // the default limit is 32 MB, which Octane's Splay alone passes
    cx = JS_NewContext(1024u * 1024 * 1024);
    if (!cx || !js::UseInternalJobQueues(cx) || !JS::InitSelfHostedCode(cx))
      return 1;
  }
  if (!install())
    return 1;
  // Every global is its own zone, too small to start a collection by
  // itself: without this the old realms pile up until the heap limit.
  JS_GC(cx);
  return 0;
}

__attribute__((export_name("sm_eval"))) int sm_eval(
    const char *src, size_t n, int pbl) {
  error.clear();
  if (!cx && sm_reset())
    return 1;
  // pbl < 0: the generic interpreter; 0: PBL with the engine's own warm-up
  // threshold; above 0: PBL after that many calls or loop iterations
  JS_SetGlobalJitCompilerOption(
      cx, JSJITCOMPILER_PORTABLE_BASELINE_ENABLE, pbl >= 0 ? 1 : 0);
  if (pbl > 0)
    JS_SetGlobalJitCompilerOption(
        cx, JSJITCOMPILER_PORTABLE_BASELINE_WARMUP_THRESHOLD, pbl);
  JSAutoRealm ar(cx, *global);
  JS::CompileOptions options(cx);
  options.setFileAndLine("script.js", 1);
  JS::SourceText<mozilla::Utf8Unit> source;
  JS::RootedValue rval(cx);
  bool ok = source.init(cx, src, n, JS::SourceOwnership::Borrowed) &&
      JS::Evaluate(cx, options, source, &rval);
  if (ok) {
    js::RunJobs(cx);
    ok = !JS_IsExceptionPending(cx);
  }
  if (!ok)
    take_error();
  return ok ? 0 : 1;
}

__attribute__((export_name("sm_error"))) const char *sm_error() {
  return error.c_str();
}
}

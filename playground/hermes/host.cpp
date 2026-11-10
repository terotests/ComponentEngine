// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The playground's Hermes engine: one Hermes runtime (JSI) kept alive in the
// wasm instance, like the other engines' VMs, so a run is compile + execute
// and not the start and teardown of a whole VM. build.sh adds this file to
// Hermes's tools as the hermes-playground target.
//
//   hermes_eval(src)  0 when the script ran, 1 when it threw (hermes_error())
//   hermes_reset()    a new runtime: a fresh realm
#include "hermes/hermes.h"

#include <emscripten/emscripten.h>

#include <cstdio>
#include <memory>
#include <string>

using namespace facebook;

static std::unique_ptr<jsi::Runtime> rt;
static std::string error;

static void install(jsi::Runtime &r) {
  auto print = jsi::Function::createFromHostFunction(
      r,
      jsi::PropNameID::forAscii(r, "print"),
      0,
      [](jsi::Runtime &r, const jsi::Value &, const jsi::Value *args, size_t n)
          -> jsi::Value {
        std::string line;
        for (size_t i = 0; i < n; i++) {
          if (i)
            line += ' ';
          line += args[i].toString(r).utf8(r);
        }
        line += '\n';
        fwrite(line.data(), 1, line.size(), stdout);
        fflush(stdout);
        return jsi::Value::undefined();
      });
  r.global().setProperty(r, "print", print);
  auto now = jsi::Function::createFromHostFunction(
      r,
      jsi::PropNameID::forAscii(r, "now"),
      0,
      [](jsi::Runtime &, const jsi::Value &, const jsi::Value *, size_t)
          -> jsi::Value { return jsi::Value(emscripten_get_now()); });
  auto perf = jsi::Object(r);
  perf.setProperty(r, "now", now);
  r.global().setProperty(r, "performance", perf);
  // what the other engines have and Hermes's runtime does not
  r.evaluateJavaScript(
      std::make_shared<jsi::StringBuffer>(
          std::string("var console = { log: print };")),
      "prelude.js");
}

extern "C" {

EMSCRIPTEN_KEEPALIVE void hermes_reset() {
  rt.reset();
  rt = facebook::hermes::makeHermesRuntime(
      ::hermes::vm::RuntimeConfig::Builder().withMicrotaskQueue(true).build());
  install(*rt);
}

EMSCRIPTEN_KEEPALIVE int hermes_eval(const char *src) {
  error.clear();
  try {
    rt->evaluateJavaScript(
        std::make_shared<jsi::StringBuffer>(std::string(src)), "script.js");
    rt->drainMicrotasks();
    return 0;
  } catch (const jsi::JSError &e) {
    std::string name;
    try {
      const jsi::Value &v = e.value();
      if (v.isObject()) {
        jsi::Value n = v.getObject(*rt).getProperty(*rt, "name");
        if (n.isString())
          name = n.getString(*rt).utf8(*rt);
      }
    } catch (...) {
    }
    error = name.empty() ? e.getMessage() : name + ": " + e.getMessage();
  } catch (const jsi::JSIException &e) {
    error = e.what();
  } catch (const std::exception &e) {
    error = e.what();
  }
  return 1;
}

EMSCRIPTEN_KEEPALIVE const char *hermes_error() {
  return error.c_str();
}
}

int main() {
  hermes_reset();
  return 0;
}

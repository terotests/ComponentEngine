// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Appended to the C++ that rgrc writes for ce_wasm.rgr, so it sees CeWasm.
// Built as a WASI reactor: the page calls _initialize once, then
// ce_alloc / ce_eval / ce_free. The script's output is stdout.

#include <cstdio>
#include <cstdlib>
#include <iostream>

extern "C" __attribute__((export_name("ce_alloc"))) void* ce_alloc(int n) {
  return std::malloc(n > 0 ? n : 1);
}

extern "C" __attribute__((export_name("ce_free"))) void ce_free(void* p) {
  std::free(p);
}

// Built with -fno-exceptions (patch-cpp.mjs): a script's own exceptions are
// the engine's values and come back as printed lines.
extern "C" __attribute__((export_name("ce_eval"))) int ce_eval(const char* p, int n) {
  CeWasm::run(std::string(p, (size_t)n));
  std::cout.flush();
  std::fflush(stdout);
  return 0;
}

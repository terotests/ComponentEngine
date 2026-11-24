// SPDX-License-Identifier: AGPL-3.0-or-later
// The host's half of CEr's native JIT (cer/src/njit.rs) for the C++ build:
// include this before the file rgrc writes. njit.rs makes the machine code
// as bytes; this file puts them into executable memory and calls them.
//
//   njit_install(bytes) -> handle (or -2)
//   njit_call(handle, sp, kv, pc, budget) -> pc
//
// Memory: one mapping per function, written while it is read-write, then
// made read-execute (W^X). Apple silicon: MAP_JIT with the thread's write
// protection toggled around the copy.
#pragma once
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <vector>
#include <sys/mman.h>
#include <unistd.h>
#if defined(__APPLE__)
#include <pthread.h>
#include <libkern/OSCacheControl.h>
#endif

namespace njit_rt {
typedef long long (*Fn)(void* sp, void* kv, long long pc, long long budget);
struct Code {
  void* p;
  size_t n;
};
inline std::vector<Code>& codes() {
  static std::vector<Code> v;
  return v;
}
inline bool& disabled() {
  static bool off = false;
  return off;
}
}  // namespace njit_rt

/// The machine njit.rs writes for: 1 x86-64, 2 AArch64, 0 none (the JIT
/// stays off). CER_NJIT=0 in the environment turns it off too.
inline int njit_arch() {
  const char* e = getenv("CER_NJIT");
  if (e && e[0] == '0') return 0;
#if defined(__x86_64__)
  return 1;
#elif defined(__aarch64__)
  return 2;
#else
  return 0;
#endif
}

/// The bytes (each 0..255) as an executable function; -2 when the system
/// refuses executable memory (the JIT then stays off for that function).
inline int njit_install(const std::vector<int>& bytes) {
  if (njit_rt::disabled() || bytes.empty()) return -2;
#if !(defined(__x86_64__) || defined(__aarch64__))
  return -2;
#else
  size_t page = (size_t)sysconf(_SC_PAGESIZE);
  size_t n = (bytes.size() + page - 1) / page * page;
#if defined(__APPLE__)
  void* p = mmap(nullptr, n, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANON | MAP_JIT, -1, 0);
#else
  void* p = mmap(nullptr, n, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
#endif
  if (p == MAP_FAILED) {
    njit_rt::disabled() = true;
    return -2;
  }
#if defined(__APPLE__)
  pthread_jit_write_protect_np(0);
#endif
  unsigned char* d = (unsigned char*)p;
  for (size_t i = 0; i < bytes.size(); i++) d[i] = (unsigned char)bytes[i];
#if defined(__APPLE__)
  pthread_jit_write_protect_np(1);
  sys_icache_invalidate(p, bytes.size());
#else
  if (mprotect(p, n, PROT_READ | PROT_EXEC) != 0) {
    munmap(p, n);
    njit_rt::disabled() = true;
    return -2;
  }
#if defined(__aarch64__)
  __builtin___clear_cache((char*)p, (char*)p + bytes.size());
#endif
#endif
  njit_rt::codes().push_back({p, n});
  return (int)njit_rt::codes().size() - 1;
#endif
}

inline int njit_call(int h, void* sp, void* kv, int pc, int budget) {
  njit_rt::Fn f = (njit_rt::Fn)njit_rt::codes()[(size_t)h].p;
  return (int)f(sp, kv, pc, budget);
}

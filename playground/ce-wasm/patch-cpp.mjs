// SPDX-License-Identifier: AGPL-3.0-or-later
//
// wasi-sdk's libc++abi has no exception support, so the engine is built with
// -fno-exceptions. rgrc's C++ runtime throws in two places, neither on a
// path a working script takes: cpp_str_to_int catches std::stoll's errors
// (rewritten here with strtoll, same result), and rg_ordered_map::at throws
// on a missing key (an engine bug; it aborts instead). The engine's own
// JavaScript exceptions do not use C++ exceptions.
//
//   node patch-cpp.mjs <in.cpp> <glue.cpp> <out.cpp>
import fs from "node:fs";

const [src, glue, out] = process.argv.slice(2);
let s = fs.readFileSync(src, "utf8");

const start = s.indexOf("std::optional<int> cpp_str_to_int(std::string s) {");
const end = s.indexOf("\n}\n", start);
if (start < 0 || end < 0) throw new Error("cpp_str_to_int not found");
s =
  s.slice(0, start) +
  `std::optional<int> cpp_str_to_int(std::string s) {
    std::optional<int> result;
    const char* p = s.c_str();
    while (*p == ' ' || (*p >= '\\t' && *p <= '\\r')) p++;
    const char* q = p;
    if (*q == '+' || *q == '-') q++;
    if (*q < '0' || *q > '9') return result;
    char* endp = nullptr;
    long long wide = std::strtoll(p, &endp, 10);
    if (wide > 2147483647LL) { wide = 2147483647LL; }
    if (wide < -2147483648LL) { wide = -2147483648LL; }
    result = (int)wide;
    return result;
}` +
  s.slice(end + 2);

const before = s.length;
s = s.replace(/throw std::out_of_range\("rg_ordered_map::at"\);/g, "std::abort();");
if (s.length === before) throw new Error("rg_ordered_map::at throw not found");
if (/\btry\s*\{/.test(s) || /\bthrow\s+std::/.test(s)) throw new Error("a C++ try/throw is left");

fs.writeFileSync(out, s + "\n" + fs.readFileSync(glue, "utf8"));

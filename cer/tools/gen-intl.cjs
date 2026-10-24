#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Writes cer/src/intl_data.rs, the CLDR and collation tables behind Intl,
// from the engine's committed ones (engine/LocaleData.rgr, LocalePlural.rgr,
// UnicodeCollate.rgr, UnicodeTailor.rgr, whose headers say what every table
// holds and which generator made it), and three facts per locale they lack
// from the host's ICU (see supplement()).
//
//   node cer/tools/gen-intl.cjs
"use strict";

const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "..", "..");
const OUT = path.join(ROOT, "cer", "src", "intl_data.rs");

function source(file) {
  return fs.readFileSync(path.join(ROOT, "engine", file), "utf8");
}
function body(text, file, name, type) {
  const m = new RegExp("static sfn " + name + ":\\[" + type + "\\] \\(\\) \\{\\s*return \\(\\[\\]([\\s\\S]*?)\\)\\s*\\}").exec(text);
  if (!m) throw new Error("no table " + name + " in " + file);
  return m[1];
}
function ints(file, name) {
  return body(source(file), file, name, "int").trim().split(/\s+/).map(Number);
}
function strings(file, name) {
  const out = [];
  const re = /"([^"]*)"/g;
  let m;
  const b = body(source(file), file, name, "string");
  while ((m = re.exec(b))) {
    if (m[1].indexOf("\n") >= 0) throw new Error("a newline in " + name);
    out.push(m[1]);
  }
  return out;
}
function count(file, name) {
  const m = new RegExp("static sfn " + name + ":int \\(\\) \\{\\s*return (\\d+)").exec(source(file));
  if (!m) throw new Error("no " + name + " in " + file);
  return Number(m[1]);
}

// a table as Rust string pieces: numbers separated by spaces, strings one a
// line (none holds a newline)
function rustInts(name, nums, doc) {
  const lines = [];
  for (let i = 0; i < nums.length; i += 24) lines.push(nums.slice(i, i + 24).join(" "));
  return `/// ${doc}\npub fn ${name}() -> String {\n    let mut s = String::new();\n` +
    lines.map((l) => `    s.push_str("${l} ");\n`).join("") + "    s\n}\n";
}
function rustStrings(name, list, doc) {
  const esc = (s) => s.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
  const lines = [];
  for (let i = 0; i < list.length; i += 12) lines.push(list.slice(i, i + 12).map((s) => esc(s) + "\\n").join(""));
  return `/// ${doc}\npub fn ${name}() -> String {\n    let mut s = String::new();\n` +
    lines.map((l) => `    s.push_str("${l}");\n`).join("") + "    s\n}\n";
}

const tags = strings("LocaleData.rgr", "tags");
const ptags = strings("LocalePlural.rgr", "tags");
if (tags.join() !== ptags.join()) throw new Error("LocaleData and LocalePlural list different locales");

// A few facts the engine's tables do not hold, read from the host's ICU
// (Node's, which must be a full-ICU build), six per locale: the NaN symbol;
// whether the default time pads the hour (the engine's patterns were
// recorded at 15:04, where H and HH look alike); the AM and PM words; the
// short and medium date patterns (dateStyle), as literals and {field}s --
// {y} {yy} {M} {MM} {MMM} {d} {dd}; the month names standing alone
// (Finnish tammikuu, where a date says tammikuuta).
function supplement() {
  if (new Intl.NumberFormat("fi").resolvedOptions().locale !== "fi") throw new Error("this Node has no full ICU");
  const out = [];
  const t = new Date(Date.UTC(2024, 0, 5, 0, 30, 9));
  const utc = (o) => Object.assign({ timeZone: "UTC" }, o);
  const find = (parts, type) => parts.find((p) => p.type === type).value;
  function datePattern(tag, style) {
    let s = "";
    for (const p of new Intl.DateTimeFormat(tag, utc({ dateStyle: style })).formatToParts(t)) {
      if (p.type === "literal") {
        if (/[{}|]/.test(p.value)) throw new Error("a brace or bar in a literal of " + tag);
        s += p.value;
      } else if (p.type === "year") s += p.value.length === 2 ? "{yy}" : "{y}";
      else if (p.type === "month") s += /^\d+$/.test(p.value) ? (p.value.length === 2 ? "{MM}" : "{M}") : "{MMM}";
      else if (p.type === "day") s += p.value.length === 2 ? "{dd}" : "{d}";
      else throw new Error("a " + p.type + " in the " + style + " date of " + tag);
    }
    return s;
  }
  for (const tag of tags) {
    const nan = find(new Intl.NumberFormat(tag).formatToParts(NaN), "nan");
    const hour = find(new Intl.DateTimeFormat(tag, utc({ timeStyle: "medium" })).formatToParts(t), "hour");
    const period = (h) => find(new Intl.DateTimeFormat(tag, utc({ hour: "numeric", hour12: true })).formatToParts(new Date(Date.UTC(2024, 0, 5, h))), "dayPeriod");
    const months = [];
    for (let m = 0; m < 12; m++) months.push(new Intl.DateTimeFormat(tag, utc({ month: "long" })).format(new Date(Date.UTC(2024, m, 5))));
    out.push(nan, hour.length === 2 ? "HH" : "H", period(3) + "|" + period(15), datePattern(tag, "short"), datePattern(tag, "medium"), months.join("|"));
  }
  return out;
}

// The engine's tailorings, without the entries that order nothing
// differently from the root collation. Its generator recorded every element
// that sorts between one letter and the next -- Swedish "ch", "ss", "ll"
// among them, which sort there as two letters, not as one -- and an entry
// like that misfiles "ch" after "cote". An entry is kept when the locale
// orders it against some probe (a letter, the letter and "a", the letter and
// "zzz", where the locale does not tailor the probe itself) differently from
// the root, by the host's ICU.
function longestIncreasing(a) {
  const best = a.map(() => 1), prev = a.map(() => -1);
  for (let i = 0; i < a.length; i++) for (let j = 0; j < i; j++) if (a[j] < a[i] && best[j] + 1 > best[i]) { best[i] = best[j] + 1; prev[i] = j; }
  let at = best.indexOf(Math.max(...best));
  const out = [];
  while (at >= 0) { out.unshift(a[at]); at = prev[at]; }
  return out;
}
function tailorings() {
  const L = ints("UnicodeTailor.rgr", "locales"), E = ints("UnicodeTailor.rgr", "entries");
  const probes = [], letters = [];
  for (let c = 97; c <= 122; c++) {
    const ch = String.fromCharCode(c);
    letters.push(ch);
    probes.push(ch, ch + "a", ch + "zzz");
  }
  const root = new Intl.Collator("und");
  const outL = [], outE = [];
  let dropped = 0;
  for (let i = 0; i < L.length;) {
    const n = L[i], tag = String.fromCharCode(...L.slice(i + 1, i + 1 + n)), off = L[i + 1 + n], cnt = L[i + 2 + n];
    const loc = new Intl.Collator(tag);
    // probes the locale itself tailors (Danish aa, Azerbaijani x) say
    // nothing about an entry
    // the letters the locale moves (Latvian y, after i): those outside the
    // longest run that keeps its root order
    const order = letters.slice().sort(loc.compare).map((x) => letters.indexOf(x));
    const run = longestIncreasing(order).map((k) => letters[k]);
    const stable = probes.filter((x) => run.every((y) => Math.sign(loc.compare(x, y)) === Math.sign(root.compare(x, y))));
    const start = outE.length;
    let kept = 0;
    for (let k = 0, p = off; k < cnt; k++) {
      const len = E[p + 2], el = String.fromCodePoint(...E.slice(p + 3, p + 3 + len)).normalize("NFC");
      // a letter after z keeps its place among the others there (Icelandic þ
      // before æ), which the root weights cannot give it
      const afterZ = E[p] === 122 && [...el].length === 1;
      const differs = afterZ || stable.some((x) => Math.sign(loc.compare(el, x)) !== Math.sign(root.compare(el, x)));
      if (differs) { outE.push(...E.slice(p, p + 3 + len)); kept++; } else dropped++;
      p += 3 + len;
    }
    outL.push(n, ...L.slice(i + 1, i + 1 + n), start, kept);
    i += 3 + n;
  }
  return { locales: outL, entries: outE, dropped };
}
const tailor = tailorings();

const tables = [
  ["supplement", "s", supplement(), "6 per locale, from the host's ICU: the NaN symbol, H or HH (the default time's hour), AM|PM, the short and medium date patterns, the stand-alone month names joined by bars"],
  ["tags", "s", tags, "one lowercased BCP 47 tag per locale; every other locale table is in this order"],
  ["num_strings", "s", strings("LocaleData.rgr", "numStrings"), "22 per locale: decimal, group, minus, percent; affixes (decimal, percent, currency letters, currency symbol; positive then negative, prefix then suffix, U+00A4 for the symbol); AM, PM"],
  ["num_ints", "i", ints("LocaleData.rgr", "numInts"), "5 per locale: primary and secondary grouping size, minimum grouping digits, hour12, year offset"],
  ["names", "s", strings("LocaleData.rgr", "names"), "38 per locale: long months, short months, long weekdays, short weekdays (Sunday first)"],
  ["pattern_ints", "i", ints("LocaleData.rgr", "patternInts"), "6 date patterns per locale (date, long date, full date, short time, time, date and time): field count, then (field, literal index, width)"],
  ["pattern_strings", "s", strings("LocaleData.rgr", "patternStrings"), "the literals of the date patterns"],
  ["currency_strings", "s", strings("LocaleData.rgr", "currencyStrings"), "(code, symbol) for each carried currency, per locale"],
  ["currency_ints", "i", ints("LocaleData.rgr", "currencyInts"), "1 when the currency's symbol is letters in that locale"],
  ["currency_count", "i", [count("LocaleData.rgr", "currencyCount")], "how many currencies each locale carries"],
  ["plural_keys", "s", strings("LocalePlural.rgr", "keyList"), "every plural lookup key: s<n> below 1001, b|i%100|i%10|millions, d|i|v|f"],
  ["plural_entries", "i", ints("LocalePlural.rgr", "entries"), "(offset, count, fallback category) per locale, cardinal then ordinal"],
  ["plural_pairs", "i", ints("LocalePlural.rgr", "pairs"), "(key id, category id): 0 zero, 1 one, 2 two, 3 few, 4 many, 5 other"],
  ["list_separators", "s", strings("LocalePlural.rgr", "listSeparators"), "(pair, middle, end) for conjunction then disjunction, per locale"],
  ["punct_runs", "i", ints("UnicodeCollate.rgr", "punctRuns"), "(start, end, rank of start): punctuation and symbols in collation order"],
  ["expansions", "i", ints("UnicodeCollate.rgr", "expansions"), "(code point, count, points…): characters that compare as a sequence"],
  ["mark_ranks", "i", ints("UnicodeCollate.rgr", "markRanks"), "(code point, rank): combining marks in secondary order"],
  ["base_letters", "i", ints("UnicodeCollate.rgr", "baseLetters"), "(code point, secondary rank, count, base points…): letters filed under a simpler one"],
  ["tailor_locales", "i", tailor.locales, "(tag length, tag points…, entry offset, entry count) per tailored locale"],
  ["tailor_entries", "i", tailor.entries, "(anchor, offset, element length, element points…): an element sorts after its anchor letter"],
];

let out = `// SPDX-License-Identifier: AGPL-3.0-or-later
//! The tables of Intl: CLDR number, date, currency, plural and list data
//! for ${tags.length} locales, and the collation weights and per-locale
//! tailorings of localeCompare. Numbers are separated by spaces, strings
//! end with a newline; decoded on first use.
//!
//! GENERATED by cer/tools/gen-intl.cjs from engine/LocaleData.rgr,
//! engine/LocalePlural.rgr, engine/UnicodeCollate.rgr and
//! engine/UnicodeTailor.rgr; supplement() from the host's ICU.

`;
const names = [];
for (const [name, kind, list, doc] of tables) {
  out += (kind === "s" ? rustStrings(name, list, doc) : rustInts(name, list, doc)) + "\n";
  names.push(name);
}
// the table a name answers, for the Intl prelude's lookups
out += `/// a table by name, empty for an unknown one\npub fn table(name: &str) -> String {\n`;
for (const [name] of tables) out += `    if name == "${name}" {\n        return ${name}();\n    }\n`;
out += "    String::new()\n}\n\n";
out += `/// whether the table of that name holds strings\npub fn is_strings(name: &str) -> bool {\n`;
out += "    " + tables.filter((t) => t[1] === "s").map((t) => `name == "${t[0]}"`).join(" || ") + "\n}\n";
fs.writeFileSync(OUT, out);
console.log("wrote " + path.relative(ROOT, OUT) + " (" + out.length + " bytes; " + tailor.dropped + " tailorings that order nothing left out)");

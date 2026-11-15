// SPDX-License-Identifier: AGPL-3.0-or-later
//! Time zones: the offset from UTC of a zone of the IANA database
//! (tzdata.rs) at a moment, local time back to UTC, and the en-US names of
//! a zone's offsets. A zone's past is its list of transitions; after the
//! last one its POSIX TZ rule (`EET-2EEST,M3.5.0/3,M10.5.0/4`) gives the
//! daylight saving changes of any year.
//!
//! The local zone of Date is UTC until the host names another
//! (Vm::set_time_zone). Without the `tzdata` feature the only zone is UTC.

use ranger::prelude::*;

/// When a POSIX rule's change happens in a year: Mm.w.d (kind 0), Jn
/// (kind 1: day 1..365, no February 29) or n (kind 2: day 0..365), at
/// `secs` local time.
pub struct TzWhen {
    pub kind: int,
    pub m: int,
    pub w: int,
    pub d: int,
    pub n: int,
    pub secs: int,
}

impl TzWhen {
    pub fn new() -> TzWhen {
        TzWhen { kind: 0, m: 0, w: 0, d: 0, n: 0, secs: 7200 }
    }
}

pub struct TzZone {
    /// the name asked for, as the database writes it
    pub name: String,
    /// the offset (seconds east of UTC) before the first transition
    pub first: int,
    /// the transitions: when (seconds since 1970), the offset after
    pub at: Vec<double>,
    pub off: Vec<int>,
    /// the POSIX rule after the last transition
    pub has_rule: bool,
    pub std_off: int,
    pub has_dst: bool,
    pub dst_off: int,
    pub start: TzWhen,
    pub end: TzWhen,
    /// en-US names: [offset, long, short] for each offset named
    pub name_offs: Vec<int>,
    pub long_names: Vec<String>,
    pub short_names: Vec<String>,
}

impl TzZone {
    pub fn utc() -> TzZone {
        let mut z = TzZone::empty(String::from("UTC"));
        z.has_rule = true;
        z.name_offs.push(0);
        z.long_names.push(String::from("Coordinated Universal Time"));
        z.short_names.push(String::from("UTC"));
        z
    }

    pub fn empty(name: String) -> TzZone {
        TzZone {
            name: name,
            first: 0,
            at: Vec::new(),
            off: Vec::new(),
            has_rule: false,
            std_off: 0,
            has_dst: false,
            dst_off: 0,
            start: TzWhen::new(),
            end: TzWhen::new(),
            name_offs: Vec::new(),
            long_names: Vec::new(),
            short_names: Vec::new(),
        }
    }
}

pub struct Tz {
    pub loaded: bool,
    /// the database's lines
    pub lines: Vec<String>,
    /// each line's names, lower case, a comma before and after each
    pub keys: Vec<String>,
    /// the zones read so far; 0 is UTC
    pub zones: Vec<TzZone>,
    /// the zone read for a line: -1 not yet
    pub zone_of_line: Vec<int>,
    /// Date's local zone (an index in zones)
    pub local: int,
}

impl Tz {
    pub fn new() -> Tz {
        let mut zs: Vec<TzZone> = Vec::new();
        zs.push(TzZone::utc());
        Tz { loaded: false, lines: Vec::new(), keys: Vec::new(), zones: zs, zone_of_line: Vec::new(), local: 0 }
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let text = crate::tzdata::zones();
        for line in text.split('\n') {
            if line.is_empty() {
                continue;
            }
            let semi = match line.find(';') {
                Some(i) => i,
                None => 0,
            };
            let names = &line[0..semi];
            self.keys.push(format!(",{},", names.to_lowercase()));
            self.lines.push(String::from(line));
            self.zone_of_line.push(-1);
        }
    }

    /// the zone of a name (any case), read on first use; -1 for none
    pub fn find(&mut self, name: &str) -> int {
        let low = name.to_lowercase();
        if low == "utc" || low == "etc/utc" || low == "gmt" || low == "etc/gmt" {
            return 0;
        }
        self.load();
        let key = format!(",{},", low);
        let mut li: usize = 0;
        while li < self.keys.len() {
            if self.keys[li].contains(key.as_str()) {
                break;
            }
            li += 1;
        }
        if li >= self.keys.len() {
            return -1;
        }
        // the spelling the database has
        let line = self.lines[li].clone();
        let semi = match line.find(';') {
            Some(i) => i,
            None => 0,
        };
        let mut spelled = String::from(name);
        for n in line[0..semi].split(',') {
            if n.to_lowercase() == low {
                spelled = String::from(n);
            }
        }
        // a link to UTC is UTC, as browsers resolve it
        if line.starts_with("Etc/UTC,") {
            return 0;
        }
        let zi = self.zone_of_line[li];
        // (each spelling is a zone of its own: its name is what it was asked as)
        let mut k: usize = 1;
        while k < self.zones.len() {
            if self.zones[k].name == spelled {
                return k as int;
            }
            k += 1;
        }
        let z = parse_zone(spelled, line.as_str());
        self.zones.push(z);
        let at = (self.zones.len() as int) - 1;
        if zi < 0 {
            self.zone_of_line[li] = at;
        }
        at
    }

    /// every zone's first name, as Intl.supportedValuesOf gives them
    pub fn names(&mut self) -> Vec<String> {
        self.load();
        let mut out: Vec<String> = Vec::new();
        for line in self.lines.iter() {
            let mut n = String::new();
            for c in line.chars() {
                if c == ',' || c == ';' {
                    break;
                }
                n.push(c);
            }
            if n != "Etc/UTC" {
                out.push(n);
            }
        }
        out.push(String::from("UTC"));
        out.sort();
        out
    }

    pub fn name_of(&self, z: int) -> String {
        self.zones[z as usize].name.clone()
    }

    /// the offset of zone z at t (ms since 1970, UTC), in ms
    pub fn offset(&self, z: int, t: double) -> double {
        if t != t {
            return 0.0;
        }
        (zone_offset(&self.zones[z as usize], t / 1000.0) as double) * 1000.0
    }

    /// UTC (ms) of a local time t of zone z: a time a change skips is taken
    /// with the offset before it (an hour later), one a change repeats is
    /// the earlier (ECMA-262 21.4.1.26 LocalTime / UTC)
    pub fn to_utc(&self, z: int, t: double) -> double {
        if t != t {
            return t;
        }
        let zone = &self.zones[z as usize];
        let s = t / 1000.0;
        let before = zone_offset(zone, s - 86400.0) as double;
        let after = zone_offset(zone, s + 86400.0) as double;
        let mut best: double = 0.0;
        let mut found = false;
        // the offsets around it: one that maps back to t
        let cand = vec![before, after];
        for o in cand.iter() {
            let u = s - *o;
            if (zone_offset(zone, u) as double) == *o {
                if !found || u < best {
                    best = u;
                    found = true;
                }
            }
        }
        if !found {
            // in a gap: the offset before the change
            best = s - before;
        }
        best * 1000.0
    }

    /// the en-US long name of zone z's offset at t (Date.prototype.toString)
    /// and its short name ("" when it is GMT+h)
    pub fn names_at(&self, z: int, t: double) -> (String, String) {
        let zone = &self.zones[z as usize];
        let o = zone_offset(zone, t / 1000.0);
        // (before 1970 a browser names the zone as it is named now, by its
        // January name)
        if t < 0.0 && !zone.long_names.is_empty() {
            return (zone.long_names[0].clone(), zone.short_names[0].clone());
        }
        let mut i: usize = 0;
        while i < zone.name_offs.len() {
            if zone.name_offs[i] == o {
                return (zone.long_names[i].clone(), zone.short_names[i].clone());
            }
            i += 1;
        }
        // (an offset it no longer has: its January name)
        if !zone.long_names.is_empty() {
            return (zone.long_names[0].clone(), String::new());
        }
        (String::new(), String::new())
    }
}

/// a base-36 number of the database, '-' before a negative one
fn b36(s: &str) -> double {
    let mut v: double = 0.0;
    let mut neg = false;
    for c in s.chars() {
        let d: int = if c == '-' {
            neg = true;
            -1
        } else if c >= '0' && c <= '9' {
            (c as int) - 48
        } else if c >= 'a' && c <= 'z' {
            (c as int) - 87
        } else {
            -1
        };
        if d >= 0 {
            v = v * 36.0 + (d as double);
        }
    }
    if neg {
        -v
    } else {
        v
    }
}

fn parse_zone(name: String, line: &str) -> TzZone {
    let parts: Vec<&str> = line.split(';').collect();
    let mut z = TzZone::empty(name);
    if parts.len() < 4 {
        return z;
    }
    parse_rule(&mut z, parts[1]);
    z.first = b36(parts[2]) as int;
    // from 1800-01-01
    let mut at: double = -5364662400.0;
    if !parts[3].is_empty() {
        for tr in parts[3].split(',') {
            let colon = match tr.find(':') {
                Some(i) => i,
                None => 0,
            };
            at += b36(&tr[0..colon]);
            z.at.push(at);
            z.off.push(b36(&tr[(colon + 1)..]) as int);
        }
    }
    if parts.len() > 4 {
        let ns: Vec<&str> = parts[4].split('|').collect();
        let mut i: usize = 0;
        while i + 3 <= ns.len() {
            z.name_offs.push(b36(ns[i]) as int);
            z.long_names.push(String::from(ns[i + 1]));
            z.short_names.push(String::from(ns[i + 2]));
            i += 3;
        }
    }
    z
}

/// a POSIX TZ string: std offset [dst [offset] [,start[/time],end[/time]]]
fn parse_rule(z: &mut TzZone, rule: &str) {
    let cs: Vec<char> = rule.chars().collect();
    if cs.is_empty() {
        return;
    }
    let mut i: usize = skip_name(&cs, 0);
    let (o, ni) = posix_secs(&cs, i);
    i = ni;
    // POSIX counts west of UTC: EET-2 is two hours east
    z.std_off = -o;
    z.has_rule = true;
    if i >= cs.len() {
        return;
    }
    i = skip_name(&cs, i);
    z.has_dst = true;
    z.dst_off = z.std_off + 3600;
    if i < cs.len() && cs[i] != ',' {
        let (d, nd) = posix_secs(&cs, i);
        z.dst_off = -d;
        i = nd;
    }
    if i < cs.len() && cs[i] == ',' {
        i = parse_when(&cs, i + 1, &mut z.start);
    }
    if i < cs.len() && cs[i] == ',' {
        parse_when(&cs, i + 1, &mut z.end);
    }
}

fn skip_name(cs: &Vec<char>, at: usize) -> usize {
    let mut i = at;
    if i < cs.len() && cs[i] == '<' {
        while i < cs.len() && cs[i] != '>' {
            i += 1;
        }
        return i + 1;
    }
    while i < cs.len() && ((cs[i] >= 'A' && cs[i] <= 'Z') || (cs[i] >= 'a' && cs[i] <= 'z')) {
        i += 1;
    }
    i
}

/// [+-]h[:mm[:ss]] as seconds, and where it ends
fn posix_secs(cs: &Vec<char>, at: usize) -> (int, usize) {
    let mut i = at;
    let mut sign: int = 1;
    if i < cs.len() && (cs[i] == '+' || cs[i] == '-') {
        if cs[i] == '-' {
            sign = -1;
        }
        i += 1;
    }
    let mut total: int = 0;
    let mut unit: int = 3600;
    let mut part: int = 0;
    while i < cs.len() && ((cs[i] >= '0' && cs[i] <= '9') || cs[i] == ':') {
        if cs[i] == ':' {
            total += part * unit;
            part = 0;
            unit = unit / 60;
        } else {
            part = part * 10 + ((cs[i] as int) - 48);
        }
        i += 1;
    }
    total += part * unit;
    (sign * total, i)
}

fn parse_when(cs: &Vec<char>, at: usize, w: &mut TzWhen) -> usize {
    let mut i = at;
    let mut nums: Vec<int> = Vec::new();
    if i < cs.len() && cs[i] == 'M' {
        w.kind = 0;
        i += 1;
    } else if i < cs.len() && cs[i] == 'J' {
        w.kind = 1;
        i += 1;
    } else {
        w.kind = 2;
    }
    let mut cur: int = 0;
    while i < cs.len() && ((cs[i] >= '0' && cs[i] <= '9') || cs[i] == '.') {
        if cs[i] == '.' {
            nums.push(cur);
            cur = 0;
        } else {
            cur = cur * 10 + ((cs[i] as int) - 48);
        }
        i += 1;
    }
    nums.push(cur);
    if w.kind == 0 && nums.len() >= 3 {
        w.m = nums[0];
        w.w = nums[1];
        w.d = nums[2];
    } else {
        w.n = nums[0];
    }
    w.secs = 7200;
    if i < cs.len() && cs[i] == '/' {
        let (s, ni) = posix_secs(cs, i + 1);
        w.secs = s;
        i = ni;
    }
    i
}

fn days_from_civil(y: int, m: int, d: int) -> int {
    let yy = if m <= 2 { y - 1 } else { y };
    let era = if yy >= 0 { yy } else { yy - 399 } / 400;
    let yoe = yy - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn year_of_days(z0: int) -> int {
    let z = z0 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400;
    if m <= 2 {
        y + 1
    } else {
        y
    }
}

fn is_leap(y: int) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// the local seconds since 1970 of a rule's change in year y
fn when_in(w: &TzWhen, y: int) -> double {
    let mut day: int;
    if w.kind == 0 {
        // the d-th weekday of week w (5: the last) of month m
        let first = days_from_civil(y, w.m, 1);
        let wd_first = ((first % 7) + 11) % 7;
        day = first + ((w.d - wd_first + 7) % 7) + (w.w - 1) * 7;
        let next = if w.m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, w.m + 1, 1) };
        while day >= next {
            day -= 7;
        }
    } else if w.kind == 1 {
        day = days_from_civil(y, 1, 1) + w.n - 1;
        if is_leap(y) && w.n >= 60 {
            day += 1;
        }
    } else {
        day = days_from_civil(y, 1, 1) + w.n;
    }
    (day as double) * 86400.0 + (w.secs as double)
}

fn rule_offset(z: &TzZone, s: double) -> int {
    if !z.has_dst {
        return z.std_off;
    }
    let y = year_of_days(((s + (z.std_off as double)) / 86400.0).floor() as int);
    // the changes in UTC: the start in standard time, the end in daylight
    let start = when_in(&z.start, y) - (z.std_off as double);
    let end = when_in(&z.end, y) - (z.dst_off as double);
    let dst = if start < end { s >= start && s < end } else { !(s >= end && s < start) };
    if dst {
        z.dst_off
    } else {
        z.std_off
    }
}

/// the offset (seconds) of a zone at s seconds since 1970, UTC
pub fn zone_offset(z: &TzZone, s: double) -> int {
    let n = z.at.len();
    if n == 0 {
        if z.has_rule {
            return rule_offset(z, s);
        }
        return z.first;
    }
    if s < z.at[0] {
        return z.first;
    }
    if s >= z.at[n - 1] && z.has_rule {
        return rule_offset(z, s);
    }
    // the last transition at or before s
    let mut lo: usize = 0;
    let mut hi: usize = n - 1;
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if z.at[mid] <= s {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    z.off[lo]
}

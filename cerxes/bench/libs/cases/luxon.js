import { DateTime, Duration, Interval, Info } from "luxon";
const b = DateTime.fromISO("2024-02-27T13:45:07.250", { zone: "utc" });
export default {
  check: {
    format: () => [b.toISO(), b.toFormat("cccc, LLLL d, yyyy 'at' HH:mm:ss.SSS"), b.toFormat("kkkk-'W'WW-c ooo"), b.setLocale("fi").toFormat("cccc d. LLLL yyyy"), b.toLocaleString(DateTime.DATETIME_FULL)],
    arith: () => [b.plus({ months: 1, days: 3 }).toISODate(), b.diff(DateTime.fromISO("2000-01-01", { zone: "utc" }), ["years", "months", "days"]).toObject(), b.startOf("week").toISO(), b.endOf("quarter").toISO()],
    durations: () => [Duration.fromObject({ hours: 25, minutes: 61 }).shiftTo("days", "hours", "minutes").toObject(), Duration.fromISO("P1Y2M10DT2H30M").as("hours"), Interval.fromDateTimes(b, b.plus({ days: 10 })).splitBy({ days: 3 }).length],
    parse: () => [DateTime.fromFormat("27.02.2024 14:05", "dd.MM.yyyy HH:mm", { zone: "utc" }).toISO(), DateTime.fromRFC2822("Tue, 01 Nov 2016 13:23:12 +0630").toUTC().toISO(), DateTime.fromISO("2024-13-01").invalidReason],
    info: () => [Info.months("long", { locale: "en" }).slice(0, 3), Info.weekdays("short", { locale: "fi" }).slice(0, 3)],
  },
  bench: {
    format: () => { let n = 0, d = b; for (let i = 0; i < 300; i++) { d = d.plus({ hours: 7 }); n += d.toFormat("yyyy-MM-dd HH:mm cccc").length; } return n; },
    parse: () => { let n = 0; for (let i = 0; i < 200; i++) n += DateTime.fromISO(`2024-0${(i % 9) + 1}-1${i % 10}T0${i % 10}:30:00`, { zone: "utc" }).ordinal; return n; },
  },
};

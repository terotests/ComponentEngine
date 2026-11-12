import { format, addDays, addMonths, differenceInCalendarDays, eachDayOfInterval, parseISO, startOfWeek, endOfMonth, isWeekend, formatDistance, parse, intervalToDuration, formatISO, getISOWeek, subYears, compareAsc } from "date-fns";
import { fi } from "date-fns/locale";
const base = new Date(Date.UTC(2024, 1, 27, 13, 45, 7));
export default {
  check: {
    format: () => [format(base, "yyyy-MM-dd HH:mm:ss"), format(base, "EEEE do MMMM yyyy, h:mm a"), format(base, "QQQ wo II R"), format(base, "PPPPpppp")],
    finnish: () => [format(base, "EEEE d. MMMM yyyy", { locale: fi }), formatDistance(base, addDays(base, 40), { locale: fi })],
    arith: () => [addMonths(new Date(Date.UTC(2024, 0, 31)), 1).toISOString(), differenceInCalendarDays(new Date(2025, 0, 1), base), eachDayOfInterval({ start: base, end: addDays(base, 5) }).filter(isWeekend).length, startOfWeek(base, { weekStartsOn: 1 }).toISOString(), endOfMonth(base).toISOString(), getISOWeek(new Date(2021, 0, 3)), subYears(base, 4).toISOString()],
    parse: () => [parseISO("2023-12-31T23:59:59.123+02:00").toISOString(), parse("27/02/2024 14:05", "dd/MM/yyyy HH:mm", new Date(0)).toISOString(), formatISO(parse("1999 Q3", "yyyy QQQ", new Date(0)))],
    duration: () => [intervalToDuration({ start: 0, end: 1e10 }), formatDistance(0, 3.2e9), [base, new Date(0), addDays(base, -1)].sort(compareAsc).map((d) => d.getTime())],
  },
  bench: {
    format: () => { let n = 0; for (let i = 0; i < 300; i++) n += format(addDays(base, i), "EEEE do MMMM yyyy HH:mm").length; return n; },
    parse: () => { let n = 0; for (let i = 0; i < 200; i++) n += parse(`${(i % 28) + 1}/0${(i % 9) + 1}/2024 1${i % 10}:05`, "dd/MM/yyyy HH:mm", base).getTime() % 7; return n; },
    interval: () => eachDayOfInterval({ start: base, end: addDays(base, 2000) }).filter(isWeekend).length,
  },
};

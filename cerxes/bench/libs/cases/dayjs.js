import dayjs from "dayjs";
import utc from "dayjs/plugin/utc";
import advancedFormat from "dayjs/plugin/advancedFormat";
import relativeTime from "dayjs/plugin/relativeTime";
import duration from "dayjs/plugin/duration";
import customParseFormat from "dayjs/plugin/customParseFormat";
import isoWeek from "dayjs/plugin/isoWeek";
import weekOfYear from "dayjs/plugin/weekOfYear";
dayjs.extend(weekOfYear);
dayjs.extend(utc); dayjs.extend(advancedFormat); dayjs.extend(relativeTime); dayjs.extend(duration); dayjs.extend(customParseFormat); dayjs.extend(isoWeek);
const b = dayjs.utc("2024-02-27T13:45:07.250Z");
export default {
  check: {
    format: () => [b.format(), b.format("dddd, MMMM Do YYYY h:mm:ss A [week] wo Q X x"), b.add(1, "month").endOf("month").format("YYYY-MM-DD HH:mm:ss.SSS"), b.isoWeek(), b.startOf("isoWeek").toISOString()],
    arith: () => [b.diff(dayjs.utc("2000-01-01"), "day"), b.diff(dayjs.utc("2000-01-01"), "month", true), b.subtract(3, "year").day(), b.isBefore(b.add(1, "ms")), dayjs.utc("2024-01-31").add(1, "month").format("YYYY-MM-DD")],
    parse: () => [dayjs.utc("27.02.2024 14:05", "DD.MM.YYYY HH:mm").toISOString(), dayjs.utc("1999-13-01", "YYYY-MM-DD", true).isValid(), dayjs.utc("2024-02-30").format("YYYY-MM-DD")],
    relative: () => [b.from(b.add(45, "minute")), b.to(b.subtract(400, "day")), dayjs.duration(123456789).toISOString(), dayjs.duration({ hours: 25, minutes: 61 }).humanize()],
  },
  bench: {
    chain: () => { let d = b, n = 0; for (let i = 0; i < 500; i++) { d = d.add(1, "day").startOf("day").add(i % 24, "hour"); n += d.format("YYYY-MM-DD HH:mm Do").length; } return n; },
    parse: () => { let n = 0; for (let i = 0; i < 300; i++) n += dayjs.utc(`${(i % 28) + 1}.0${(i % 9) + 1}.2024`, "DD.MM.YYYY").valueOf() % 13; return n; },
  },
};

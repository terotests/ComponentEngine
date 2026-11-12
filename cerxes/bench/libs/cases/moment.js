import moment from "moment";
import "moment/locale/fi";
import "moment/locale/de";
const b = moment.utc("2024-02-27T13:45:07.250Z");
export default {
  check: {
    format: () => [b.format(), b.format("dddd, MMMM Do YYYY, h:mm:ss a [w]wo Qo DDDo X"), b.clone().locale("fi").format("LLLL"), b.clone().locale("de").format("dddd D. MMMM YYYY"), b.clone().add(1, "month").endOf("month").toISOString(), b.isoWeekday()],
    relative: () => [b.from(b.clone().add(3, "hours")), b.clone().locale("fi").to(b.clone().subtract(2, "years")), moment.duration(123456789).humanize(), moment.duration("P1Y2M3DT4H").asDays(), b.calendar(b.clone().add(1, "day"))],
    parse: () => [moment.utc("27.02.2024 14:05", "DD.MM.YYYY HH:mm").toISOString(), moment.utc("2024-02-30", "YYYY-MM-DD", true).isValid(), moment.utc("Feb 3rd 2021", "MMM Do YYYY").format("L"), b.diff(moment.utc("2000-01-01"), "months", true)],
  },
  bench: {
    format: () => { let n = 0; const d = b.clone(); for (let i = 0; i < 300; i++) n += d.add(7, "hours").format("dddd, MMMM Do YYYY HH:mm").length; return n; },
    parse: () => { let n = 0; for (let i = 0; i < 200; i++) n += moment.utc(`${(i % 28) + 1}.0${(i % 9) + 1}.2024`, "DD.MM.YYYY").dayOfYear(); return n; },
  },
};

import validator from "validator";
const samples = ["foo@bar.com", "Foo.Bar+tag@sub.example.co.uk", "bad@", "http://user:pw@example.com:8080/p?q=1#h", "ftp://x", "192.168.1.300", "2001:db8::ff00:42:8329", "4111 1111 1111 1111", "FI2112345600000785", "550e8400-e29b-41d4-a716-446655440000", "2024-02-29T12:00:00Z", "#ff00aa", "+358401234567", "SGVsbG8=", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.sig", "978-3-16-148410-0", "12:30", "Ab1!abcd"];
const fns = ["isEmail", "isURL", "isIP", "isCreditCard", "isIBAN", "isUUID", "isISO8601", "isHexColor", "isMobilePhone", "isBase64", "isJWT", "isISBN", "isTime", "isStrongPassword"];
export default {
  check: {
    matrix: () => fns.map((f) => samples.map((s) => (validator[f](s) ? 1 : 0)).join("")),
    sanitize: () => [validator.escape("<a href='x'>&</a>"), validator.normalizeEmail("Foo.Bar+tag@GoogleMail.com"), validator.toDate("2024-02-27").toISOString(), validator.trim("  x  "), validator.blacklist("abc-def", "\\-"), validator.isLength("äö😀", { min: 3, max: 3 })],
  },
  bench: {
    matrix: () => { let n = 0; for (const f of fns) for (const s of samples) if (validator[f](s)) n++; return n; },
  },
};

import Ajv from "ajv";
const ajv = new Ajv({ allErrors: true });
const schema = {
  type: "object", required: ["id", "name", "tags"], additionalProperties: false,
  properties: {
    id: { type: "integer", minimum: 1 }, name: { type: "string", minLength: 2, pattern: "^[A-Z]" },
    email: { type: "string" }, tags: { type: "array", items: { type: "string" }, uniqueItems: true, maxItems: 5 },
    nested: { type: "object", properties: { a: { enum: [1, 2, 3] }, b: { anyOf: [{ type: "null" }, { type: "number", multipleOf: 0.5 }] } } },
  },
};
const validate = ajv.compile(schema);
const data = Array.from({ length: 500 }, (_x, i) => ({ id: i % 50 === 0 ? 0 : i, name: i % 7 ? "Name" + i : "x", tags: i % 11 ? ["a", "b" + (i % 3)] : ["a", "a"], nested: { a: (i % 4) || 1, b: i % 5 ? i / 2 : null } }));
export default {
  check: {
    valid: () => data.slice(0, 30).map((d) => validate(d)),
    errors: () => (validate({ id: -1, name: 3, tags: ["a", "a", 1], extra: 1 }), validate.errors.map((e) => e.instancePath + " " + e.keyword)),
    count: () => data.filter((d) => validate(d)).length,
  },
  bench: { compile: () => typeof new Ajv().compile(schema), validate: () => data.filter((d) => validate(d)).length },
};

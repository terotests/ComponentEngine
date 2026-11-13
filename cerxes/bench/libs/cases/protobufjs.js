import protobuf from "protobufjs";
const root = protobuf.parse(`syntax = "proto3"; package t;
message Point { double x = 1; double y = 2; }
message Item { int32 id = 1; string name = 2; repeated string tags = 3; Point at = 4; int64 big = 5; bool ok = 6; bytes raw = 7; enum Kind { A = 0; B = 1; } Kind kind = 8; map<string, int32> counts = 9; }
message List { repeated Item items = 1; }`).root;
const List = root.lookupType("t.List");
const items = Array.from({ length: 300 }, (_x, i) => ({ id: i, name: "item" + i, tags: ["a", "b" + (i % 4)], at: { x: i / 3, y: -i }, big: i * 1000003, ok: i % 2 === 0, raw: new Uint8Array([i & 255, 1, 2]), kind: i % 2, counts: { a: i, b: i * 2 } }));
const msg = List.fromObject({ items });
const buf = List.encode(msg).finish();
export default {
  check: {
    encode: () => Array.from(List.encode(List.fromObject({ items: items.slice(0, 2) })).finish()),
    decode: () => List.toObject(List.decode(buf), { longs: String, enums: String, bytes: String }).items.slice(0, 3),
    size: () => buf.length,
    verify: () => [List.verify({ items: [{ id: "x" }] }), List.verify({ items: [{ id: 1 }] })],
  },
  bench: { encode: () => List.encode(List.fromObject({ items })).finish().length, decode: () => List.decode(buf).items.length },
};

import { buildSchema, graphqlSync, parse, validate, print } from "graphql";
const schema = buildSchema(`
  type Query { users(first: Int = 10): [User!]! user(id: ID!): User }
  type User { id: ID! name: String! friends: [User!]! posts(limit: Int): [Post!]! }
  type Post { id: ID! title: String! score: Float }
`);
const users = Array.from({ length: 60 }, (_x, i) => ({ id: String(i), name: "U" + i }));
const root = {
  users: ({ first }) => users.slice(0, first).map(wrap),
  user: ({ id }) => wrap(users[+id]),
};
function wrap(u) { return { ...u, friends: () => [1, 2, 3].map((k) => wrap(users[(+u.id + k * 7) % users.length])).slice(0, 2), posts: ({ limit }) => Array.from({ length: limit ?? 2 }, (_x, j) => ({ id: u.id + "-" + j, title: "P" + j, score: j * 1.5 })) }; }
const q = `query Q($n: Int) { users(first: $n) { id name friends { name friends { id } } posts(limit: 3) { title score } } }`;
export default {
  check: {
    exec: () => graphqlSync({ schema, source: q, rootValue: root, variableValues: { n: 3 } }),
    errors: () => validate(schema, parse("{ users { nope } user { id } }")).map((e) => e.message),
    print: () => print(parse(q)),
  },
  bench: {
    exec: () => graphqlSync({ schema, source: q, rootValue: root, variableValues: { n: 40 } }).data.users.length,
    parseValidate: () => validate(schema, parse(q)).length,
  },
};

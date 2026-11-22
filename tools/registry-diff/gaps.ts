// The scenarios of the gaps closed on 2026-11-19 (docs/port/behaviours/registry.md, REG-46 to REG-50): Protobuf
// normalization over a corpus of tricky schemas, JSON Schema numbers past Java's BigDecimal, and data contracts
// (metadata and rule sets with a schema, and the settings that default and override them). As in scenarios.ts, what
// each step answers is whatever the reference answers, recorded at run time.
import { type Step, reg } from "./lib.ts";

const get = (p: string): Step => ({ m: "GET", p });
const post = (p: string, b: string): Step => ({ m: "POST", p, b });
const put = (p: string, b: string): Step => ({ m: "PUT", p, b });
const del = (p: string): Step => ({ m: "DELETE", p });
const pb = (text: string, extra: Record<string, unknown> = {}) => reg(text, { schemaType: "PROTOBUF", ...extra });

// ---------- Protobuf normalization ----------

/** Tricky `.proto` schemas: each one is registered normalized, read back, looked up and checked. */
export const protos: Record<string, string> = {
  // Options of every kind, a map, oneofs out of order, reserved ranges and names, an aliased enum, an extend, a service.
  mixed: `syntax = "proto3";
package shop.v1;
import "google/protobuf/timestamp.proto";
import "google/protobuf/descriptor.proto";
option java_package = 'com.shop';
option go_package = "x";
extend google.protobuf.FieldOptions { Rule rule = 50001; repeated string tags = 50002; }
message Rule { int32 min = 1; int32 max = 2; Inner inner = 3; message Inner { string a = 1; } }
message Order {
  reserved 20 to 25, 9;
  reserved "zz", "aa";
  option deprecated = true;
  map<string, Line> lines = 5;
  string id = 1 [json_name = "ID", deprecated = true, (rule).min = 1, (rule).max = 0x10, (tags) = "b", (tags) = "a"];
  google.protobuf.Timestamp at = 3 [(rule) = { max: 2.50 min: 1 inner { a: "q" } }];
  oneof kind { string b = 12; int64 a = 11; }
  oneof other { bool z = 7; }
  double d = 2;
  message Line { Status s = 1; }
  enum Status { option allow_alias = true; reserved 9, 3 to 4; NEW = 0; DONE = 2; OK = 2; ALSO = 1; }
}
service Svc { option deprecated = true; rpc Get (Order) returns (stream Order.Line); }
`,
  // proto2: no syntax line once normalized; groups, extension ranges, defaults of every type, nested and repeated
  // extends of one message made one.
  proto2: `syntax = "proto2";
package legacy;
message Base {
  extensions 500 to max;
  extensions 100 to 199, 50;
  required int32 id = 3 [default = 0x10];
  optional double ratio = 2 [default = 1.23456];
  optional float f = 9 [default = -inf];
  optional float g = 10 [default = 1e20];
  optional string s = 1 [default = "a\\x41\\n'q'" "tail"];
  optional Color c = 4 [default = RED];
  optional bool b = 5 [default = true, deprecated = true];
  repeated int32 packed = 7 [packed = true];
  optional group Result = 8 { required string url = 2; optional string title = 1; }
  enum Color { RED = 1; GREEN = 0; }
  extend Base { optional int32 inner_ext = 501; }
}
extend Base { optional string tag = 502; optional int32 weight = 500; }
extend Base { repeated int64 more = 503; }
`,
  // Names relative to nested scopes, absolute names, a package-relative name, rpc options and streams.
  names: `syntax = "proto3";
package a.b;
message Outer {
  message Inner { message Leaf { int32 x = 1; } Leaf leaf = 1; }
  Inner.Leaf direct = 1;
  .a.b.Outer.Inner absolute = 2;
  b.Outer up = 3;
  repeated Inner inners = 4;
}
message Other { Outer.Inner.Leaf l = 1; a.b.Other self = 2; }
service S {
  rpc R (Outer) returns (b.Other) { option idempotency_level = NO_SIDE_EFFECTS; option deprecated = true; }
  rpc Q (stream .a.b.Other) returns (Other);
}
`,
  // Custom options: dotted names, a repeated one given twice, a message literal with lists, an empty list, a one-item list.
  options: `syntax = "proto3";
package opts;
import "google/protobuf/descriptor.proto";
message Rule { int32 min = 1; int32 max = 2; repeated string tags = 3; }
extend google.protobuf.MessageOptions { Rule rule = 51000; repeated Rule rules = 51001; string label = 51002; }
extend google.protobuf.FieldOptions { int32 width = 52000; }
message M {
  option (rule).min = 1;
  option (rule).tags = "x";
  option (rule).tags = "y";
  option (rules) = { min: 1 tags: ["a"] };
  option (rules) = { max: 2, tags: [] };
  option (label) = 'L\\'s';
  int32 v = 1 [(width) = 010, deprecated = false];
}
`,
  // A non-repeated custom option set twice on one message field: 8.x merges the literals deep, 7.7.1 makes a list.
  merge: `syntax = "proto3";
package mg;
import "google/protobuf/descriptor.proto";
message Sub { int32 a = 1; int32 b = 2; }
message Rule { int32 min = 1; Sub sub = 2; repeated int32 tags = 3; }
extend google.protobuf.MessageOptions { Rule rule = 51000; }
message M {
  option (rule).sub.a = 5;
  option (rule).sub.b = 6;
  option (rule).min = 1;
  option (rule).min = 2;
  option (rule).tags = 1;
  option (rule).tags = 1;
  int32 v = 1;
}
`,
  // Numbers in options as Java's DecimalFormat writes them, an extension key in a message literal.
  numbers: `syntax = "proto2";
package nums;
import "google/protobuf/descriptor.proto";
message Limits { optional double lo = 1; optional double hi = 2; repeated float steps = 3; optional int64 big = 4; extensions 100 to 200; }
extend Limits { optional int32 extra = 100; }
extend google.protobuf.FieldOptions { optional Limits limits = 53000; optional double scale = 53001; }
message N {
  optional double a = 1 [(scale) = 0.0625];
  optional double b = 2 [(scale) = 2.50e0];
  optional double c = 3 [(limits) = { lo: -0.0001 hi: 1.23456789 steps: [0.1, 0.5, 1e3] big: 0x7fffffffffffffff [nums.extra]: 7 }];
  optional double d = 4 [(scale) = 123456789.123456];
  optional double e = 5 [(scale) = -inf];
  optional double f = 6 [(scale) = nan];
  optional float g = 7 [default = 3.14159];
  optional double h = 8 [(scale) = 1e20];
}
`,
  // Enum values out of order, aliases, value options, reserved names and ranges.
  enums: `syntax = "proto3";
package en;
import "google/protobuf/descriptor.proto";
extend google.protobuf.EnumValueOptions { int32 weight = 54000; }
enum E {
  option allow_alias = true;
  option deprecated = true;
  reserved "OLD", "GONE";
  reserved 10 to max, 5, 7 to 8;
  A = 0;
  C = 3 [deprecated = true, (weight) = 2];
  B = 1;
  BB = 1 [deprecated = true];
  NEG = -1;
}
message Holder { E e = 1; }
`,
  // proto2 without a syntax line, as Confluent stores a normalized proto2 schema.
  nosyntax: `package q;
message A { optional int32 b = 2; required string a = 1 [default = 'x']; }
`,
  // A file without a package, maps of messages and scalars.
  nopkg: `syntax = "proto3";
message A { map<string, B> bs = 3; map<int64, string> names = 2; B b = 1; }
message B { A a = 1; repeated A as = 2; }
`,
};

/** The same schemas written otherwise: comments, spacing and every order Confluent sorts. */
const reordered: Record<string, string> = {
  mixed: `// reordered
syntax = "proto3";
package shop.v1;
import "google/protobuf/descriptor.proto";
import "google/protobuf/timestamp.proto";
option go_package = "x";
option java_package = "com.shop";
extend google.protobuf.FieldOptions { repeated string tags = 50002; Rule rule = 50001; }
message Rule { int32 max = 2; Inner inner = 3; int32 min = 1; message Inner { string a = 1; } }
message Order {
  reserved "aa", "zz";
  reserved 9, 20 to 25;
  option deprecated = true;
  double d = 2;
  string id = 1 [(tags) = "b", (rule).max = 16, deprecated = true, (tags) = "a", (rule).min = 1, json_name = "ID"];
  google.protobuf.Timestamp at = 3 [(rule) = { min: 1, inner: { a: "q" }, max: 2.5 }];
  map<string, Line> lines = 5;
  oneof other { bool z = 7; }
  oneof kind { int64 a = 11; string b = 12; }
  message Line { Status s = 1; }
  enum Status { option allow_alias = true; reserved 3 to 4, 9; NEW = 0; OK = 2; ALSO = 1; DONE = 2; }
}
service Svc { option deprecated = true; rpc Get (.shop.v1.Order) returns (stream .shop.v1.Order.Line); }
`,
};

const protoNormalize: Record<string, Step[]> = Object.fromEntries(
  Object.entries(protos).map(([n, text]) => {
    const s = `pn-${n}`;
    const steps: Step[] = [
      post(`/subjects/${s}/versions?normalize=true`, pb(text)),
      get(`/subjects/${s}/versions/latest/schema`),
      post(`/subjects/${s}?normalize=true`, pb(text)),
      post(`/subjects/${s}`, pb(text)),
      post(`/compatibility/subjects/${s}/versions/latest?normalize=true&verbose=true`, pb(text)),
      get(`/schemas/ids/@1?format=serialized`),
      // Registered as written, then looked up and registered again normalized.
      post(`/subjects/pp-${n}/versions`, pb(text)),
      post(`/subjects/pp-${n}?normalize=true`, pb(text)),
      post(`/subjects/pp-${n}/versions?normalize=true`, pb(text)),
      get(`/subjects/pp-${n}/versions`),
      // The setting does what the parameter does.
      put(`/config/pc-${n}`, JSON.stringify({ normalize: true })),
      post(`/subjects/pc-${n}/versions`, pb(text)),
      get(`/subjects/pc-${n}/versions/latest/schema`),
      post(`/subjects/pc-${n}`, pb(text)),
    ];
    if (reordered[n]) {
      steps.push(
        post(`/subjects/${s}?normalize=true`, pb(reordered[n])),
        post(`/subjects/${s}/versions?normalize=true`, pb(reordered[n])),
        get(`/subjects/${s}/versions`),
      );
    }
    return [`protobuf-normalize/${n}`, steps];
  }),
);

protoNormalize["protobuf-normalize/references"] = [
  post("/subjects/zz/versions", pb('syntax = "proto3";\npackage zed;\nmessage Z { int32 z = 1; }\n')),
  post("/subjects/aa/versions", pb('syntax = "proto3";\npackage shop;\nmessage Money { int64 units = 1; }\n')),
  post(
    "/subjects/refs/versions?normalize=true",
    pb(
      'syntax = "proto3";\nimport "z.proto";\nimport public "m.proto";\nimport "google/protobuf/timestamp.proto";\nmessage Paid { shop.Money amount = 1; google.protobuf.Timestamp at = 2; zed.Z z = 3; }\n',
      {
        references: [
          { name: "z.proto", subject: "zz", version: 1 },
          { name: "m.proto", subject: "aa", version: 1 },
        ],
      },
    ),
  ),
  get("/subjects/refs/versions/latest"),
  post(
    "/subjects/refs?normalize=true",
    pb(
      'syntax = "proto3";\nimport public "m.proto";\nimport "z.proto";\nimport "google/protobuf/timestamp.proto";\nmessage Paid { zed.Z z = 3; .shop.Money amount = 1; google.protobuf.Timestamp at = 2; }\n',
      {
        references: [
          { name: "m.proto", subject: "aa", version: 1 },
          { name: "z.proto", subject: "zz", version: 1 },
        ],
      },
    ),
  ),
];

// A schema sent as Confluent's descriptor (a map is its *Entry message there): normalizing writes the map again.
protoNormalize["protobuf-normalize/descriptor"] = [
  post(
    "/subjects/pdesc/versions?normalize=true",
    pb(
      "CgdkZWZhdWx0EgNpbnYixwEKBVN0b2NrEhcKBWl0ZW1zGAIgAzIKaXRlbXNFbnRyeRIXCgVuYW1lcxgBIAMyCm5hbWVzRW50cnkaHgoESXRlbRIJCgNxdHkYASgDEgsKAWsYAjIES2luZBosCgppdGVtc0VudHJ5EgkKA2tleRgBKAkSDwoFdmFsdWUYAjIESXRlbToCOAEaKAoKbmFtZXNFbnRyeRIJCgNrZXkYASgFEgsKBXZhbHVlGAIoCToCOAEiFAoES2luZBIFCgFBEAASBQoBQhABYgZwcm90bzM=",
    ),
  ),
  get("/subjects/pdesc/versions/latest/schema"),
  post(
    "/subjects/pdesc?normalize=true",
    pb('syntax = "proto3";\npackage inv;\nmessage Stock { map<string, Item> items = 2; map<int32, string> names = 1; message Item { int64 qty = 1; Kind k = 2; } enum Kind { A = 0; B = 1; } }\n'),
  ),
];

// Compatibility asked with normalize: a reordered schema with one more field.
protoNormalize["protobuf-normalize/compatibility"] = [
  post("/subjects/pcmp/versions?normalize=true", pb('syntax = "proto3";\npackage c;\nmessage M { string b = 2; int32 a = 1; }\n')),
  post(
    "/compatibility/subjects/pcmp/versions/latest?normalize=true&verbose=true",
    pb('syntax = "proto3";\npackage c;\nmessage M { int32 a = 1; int64 c = 3; string b = 2; }\n'),
  ),
  post(
    "/compatibility/subjects/pcmp/versions/latest?normalize=true&verbose=true",
    pb('syntax = "proto3";\npackage c;\nmessage M { int32 a = 1; bytes b = 2; }\n'),
  ),
  post("/subjects/pcmp/versions?normalize=true", pb('syntax = "proto3";\npackage c;\nmessage M { int64 c = 3; int32 a = 1; string b = 2; }\n')),
  get("/subjects/pcmp/versions/latest/schema"),
  // A type Confluent's resolution does not find is refused whether or not the schema is normalized.
  post("/subjects/pcmp/versions", pb('syntax = "proto3";\npackage c;\nmessage M { int32 a = 1; Nope n = 2; }\n')),
];

// ---------- JSON Schema numbers past BigDecimal ----------

const huge = [
  '{"type":"number","minimum":0.5555e+000011999153157185615783}',
  '{"type":"number","minimum":1e2147483647}',
  '{"type":"number","minimum":1e2147483648}',
  '{"type":"number","minimum":12e2147483647}',
  '{"type":"number","minimum":0.12e2147483649}',
  '{"type":"number","minimum":1e-2147483647}',
  '{"type":"number","minimum":1e-2147483648}',
  '{"type":"number","minimum":-1e-2147483649}',
  '{"type":"number","minimum":0.1e-2147483647}',
  '{"type":"number","minimum":1e-00000000000000000002147483648}',
  '{"type":"number","minimum":1e+00000000000000000005}',
  '{"type":"number","minimum":-1e2147483648}',
  '{"type":"number","default":1e2147483648}',
  '{"type":"number","x-big":1e99999999999}',
  '{"type":"number","multipleOf":1e-2147483647}',
  '{"type":"array","minItems":1e99999999999}',
  '{"type":"array","minItems":1e-2147483649}',
  '{"type":"array","minItems":1e1}',
  '{"type":"array","minItems":1.0}',
  '{"type":"array","minItems":2147483647}',
  '{"type":"array","minItems":2147483648}',
  '{"type":"string","maxLength":2.0}',
  '{"type":"object","minProperties":1E0}',
  '{"$schema":"http://json-schema.org/draft-04/schema#","type":"array","minItems":1.0}',
  '{"$schema":"http://json-schema.org/draft-06/schema#","type":"array","maxItems":3e0}',
  '{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"array","minItems":1.0}',
  '{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"array","minItems":99999999999}',
  '{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"number","minimum":1e-2147483649}',
];

const jsonHuge: Step[] = [
  put("/config", JSON.stringify({ compatibility: "NONE" })),
  ...huge.flatMap((s, i) => [
    post(`/subjects/jh${i}/versions`, reg(s, { schemaType: "JSON" })),
    get(`/subjects/jh${i}/versions/latest/schema`),
  ]),
  // Normalized: the stored text read again, so a double's -0.0 is 0.0.
  post("/subjects/jhn/versions?normalize=true", reg('{"type":"number","minimum":-1e-2147483649,"a":1}', { schemaType: "JSON" })),
  get("/subjects/jhn/versions/latest/schema"),
  // A number Java reads as zero compares as zero.
  post("/subjects/jhc/versions", reg('{"type":"number","minimum":0}', { schemaType: "JSON" })),
  put("/config/jhc", JSON.stringify({ compatibility: "BACKWARD" })),
  post("/compatibility/subjects/jhc/versions/latest?verbose=true", reg('{"type":"number","minimum":1e-2147483649}', { schemaType: "JSON" })),
  post("/compatibility/subjects/jhc/versions/latest?verbose=true", reg('{"type":"number","minimum":1e-2147483647}', { schemaType: "JSON" })),
  // Last: Confluent takes [0.0, 0], which this registry refuses as a repeated value (REG-D29).
  post("/subjects/jhe/versions", reg('{"type":"number","enum":[1e-2147483649, 0]}', { schemaType: "JSON" })),
  get("/subjects/jhe/versions/latest/schema"),
];

// How Confluent compares numbers in const and enum: as the Java values its Jackson makes (an integer, or a BigDecimal of
// its scale), the old one read again from its stored text.
const enumPairs: [string, string][] = [
  ['{"const":7e2}', '{"const":7E+2}'],
  ['{"const":7e2}', '{"const":700}'],
  ['{"const":7e2}', '{"const":7e2}'],
  ['{"const":-77e0}', '{"const":-77e0}'],
  ['{"const":-77e0}', '{"const":-77}'],
  ['{"const":-77}', '{"const":-77e0}'],
  ['{"const":1}', '{"const":1.0}'],
  ['{"const":1.0}', '{"const":1.00}'],
  ['{"const":10e-1}', '{"const":1.0}'],
  ['{"const":1e0}', '{"const":1}'],
  ['{"enum":[1,2]}', '{"enum":[1.0,2]}'],
  ['{"enum":[7e2,1]}', '{"enum":[7E+2,1]}'],
  ['{"enum":[7e2,1]}', '{"enum":[700,1]}'],
];
const enumNumbers: Step[] = [
  put("/config", JSON.stringify({ compatibility: "BACKWARD" })),
  ...enumPairs.flatMap(([x, y], i) => [
    post(`/subjects/en${i}/versions`, reg(x, { schemaType: "JSON" })),
    get(`/subjects/en${i}/versions/latest/schema`),
    post(`/compatibility/subjects/en${i}/versions/latest?verbose=true`, reg(y, { schemaType: "JSON" })),
  ]),
];

export const gaps: Record<string, Step[]> = {
  ...protoNormalize,
  "json-schema-huge-numbers": jsonHuge,
  "json-schema-enum-numbers": enumNumbers,
};

// ---------- Data contracts ----------

const user = { type: "record", name: "User", namespace: "com.example", fields: [{ name: "id", type: "long" }] };
const user2 = { ...user, fields: [...user.fields, { name: "email", type: ["null", "string"], default: null }] };
const userBad = { ...user, fields: [{ name: "id", type: "string" }] };
const meta = { properties: { owner: "a", b: "2" }, tags: { "com.example.User.id": ["PII", "ID"] }, sensitive: ["s2", "s1"] };
const rule = (name: string, extra: Record<string, unknown> = {}) => ({ name, kind: "CONDITION", mode: "WRITE", type: "CEL", expr: "true", ...extra });
const av = (schema: unknown, extra: Record<string, unknown> = {}) => reg(schema as object, extra);

const contracts: Record<string, Step[]> = {
  "data-contracts/register-and-read": [
    post("/subjects/dc/versions", av(user, { metadata: meta })),
    get("/subjects/dc/versions/latest"),
    get("/subjects/dc/versions/1"),
    get("/schemas/ids/@1"),
    get("/schemas/ids/@1/schema"),
    get("/subjects/dc/versions/latest/schema"),
    // The same schema without metadata takes the latest version's: it is that version.
    post("/subjects/dc/versions", av(user)),
    // The same schema with other metadata is a new schema, and a new version.
    post("/subjects/dc/versions", av(user, { metadata: { properties: { owner: "b" } } })),
    get("/subjects/dc/versions"),
    get("/subjects/dc/versions/2"),
    get("/schemas/ids/@2"),
    // Lookups: as sent, with the first version's metadata, with metadata no version has.
    post("/subjects/dc", av(user)),
    post("/subjects/dc", av(user, { metadata: meta })),
    post("/subjects/dc", av(user, { metadata: { properties: { owner: "b" } } })),
    post("/subjects/dc", av(user, { metadata: { properties: { owner: "zzz" } } })),
    // A new schema carries the latest version's metadata forward.
    post("/subjects/dc/versions", av(user2)),
    get("/subjects/dc/versions/latest"),
    get("/schemas/ids/@3"),
    post("/subjects/dc/versions", av(user2)),
    // The same schema and contract under another subject has the same id.
    post("/subjects/dc-other/versions", av(user2, { metadata: { properties: { owner: "b" } } })),
    get("/schemas?subjectPrefix=dc"),
    get("/schemas/ids/@3/versions"),
    // A version asked for: confluent:version is set.
    post("/subjects/dc/versions", av(userBad, { version: 6, metadata: { properties: { owner: "c" } } })),
    put("/config/dc", JSON.stringify({ compatibility: "NONE" })),
    post("/subjects/dc/versions", av(userBad, { version: 6, metadata: { properties: { owner: "c" } } })),
    get("/subjects/dc/versions/latest"),
    post("/subjects/dc/versions", av(user, { version: 7 })),
    get("/subjects/dc/versions/latest"),
    post("/subjects/dc/versions", av(user2)),
    get("/subjects/dc/versions/latest"),
    // An empty metadata and rule set.
    post("/subjects/dc-empty/versions", av(user, { metadata: {} })),
    get("/subjects/dc-empty/versions/latest"),
    post("/subjects/dc-empty/versions", av(user, { metadata: { properties: {} } })),
    get("/subjects/dc-empty/versions"),
  ],

  "data-contracts/latest-with-metadata": [
    post("/subjects/lm/versions", av(user, { metadata: { properties: { owner: "a", env: "x" } } })),
    post("/subjects/lm/versions", av(user2, { metadata: { properties: { owner: "b", env: "x" } } })),
    get("/subjects/lm/metadata?key=owner&value=a"),
    get("/subjects/lm/metadata?key=owner&value=b"),
    get("/subjects/lm/metadata?key=env&value=x"),
    get("/subjects/lm/metadata?key=owner&value=a&key=env&value=x"),
    get("/subjects/lm/metadata?key=owner&value=a&key=env&value=y"),
    get("/subjects/lm/metadata?key=owner&value=nope"),
    get("/subjects/lm/metadata?key=owner"),
    get("/subjects/lm/metadata"),
    get("/subjects/nosuch/metadata?key=owner&value=a"),
    del("/subjects/lm/versions/1"),
    get("/subjects/lm/metadata?key=owner&value=a"),
    get("/subjects/lm/metadata?key=owner&value=a&deleted=true"),
    del("/subjects/lm"),
    get("/subjects/lm/metadata?key=env&value=x"),
    get("/subjects/lm/metadata?key=env&value=x&deleted=true"),
    // A soft-deleted version with metadata, and the lookups that may see it.
    post("/subjects/lm2/versions", av(user, { metadata: { properties: { owner: "a" } } })),
    post("/subjects/lm2/versions", av(user2)),
    del("/subjects/lm2/versions/1"),
    post("/subjects/lm2", av(user, { metadata: { properties: { owner: "a" } } })),
    post("/subjects/lm2?deleted=true", av(user, { metadata: { properties: { owner: "a" } } })),
    post("/subjects/lm2/versions", av(user, { metadata: { properties: { owner: "a" } } })),
    get("/subjects/lm2/versions?deleted=true"),
    get("/subjects/lm2/versions/latest"),
  ],

  "data-contracts/config-defaults-and-overrides": [
    put(
      "/config/cfg",
      JSON.stringify({
        defaultMetadata: { properties: { team: "x", owner: "dflt" } },
        overrideMetadata: { properties: { owner: "forced" }, sensitive: ["k"], tags: { f: ["b", "a"] } },
      }),
    ),
    get("/config/cfg"),
    post("/subjects/cfg/versions", av(user)),
    get("/subjects/cfg/versions/latest"),
    post("/subjects/cfg/versions", av(user)),
    post("/subjects/cfg/versions", av(user, { metadata: { properties: { owner: "mine", other: "y" } } })),
    get("/subjects/cfg/versions/latest"),
    post("/subjects/cfg", av(user)),
    post("/subjects/cfg", av(user, { metadata: { properties: { owner: "mine", other: "y" } } })),
    post("/compatibility/subjects/cfg/versions/latest?verbose=true", av(user2)),
    // The global defaults reach a subject with none of its own.
    put("/config", JSON.stringify({ defaultMetadata: { properties: { global: "g" } } })),
    get("/config"),
    post("/subjects/cfg-global/versions", av(user)),
    get("/subjects/cfg-global/versions/latest"),
    // A subject's own configuration replaces the global one whole.
    put("/config/cfg-own", JSON.stringify({ compatibility: "NONE" })),
    post("/subjects/cfg-own/versions", av(user)),
    get("/subjects/cfg-own/versions/latest"),
    del("/config/cfg"),
    post("/subjects/cfg/versions", av(user2)),
    get("/subjects/cfg/versions/latest"),
    del("/config"),
  ],

  "data-contracts/compatibility-group": [
    put("/config/cg", JSON.stringify({ compatibility: "BACKWARD", compatibilityGroup: "major" })),
    post("/subjects/cg/versions", av(user, { metadata: { properties: { major: "1" } } })),
    post("/compatibility/subjects/cg/versions/latest?verbose=true", av(userBad, { metadata: { properties: { major: "1" } } })),
    post("/subjects/cg/versions", av(userBad, { metadata: { properties: { major: "1" } } })),
    post("/compatibility/subjects/cg/versions/latest?verbose=true", av(userBad, { metadata: { properties: { major: "2" } } })),
    post("/subjects/cg/versions", av(userBad, { metadata: { properties: { major: "2" } } })),
    // Without metadata, the new schema takes the latest version's group.
    post("/subjects/cg/versions", av(user)),
    post("/compatibility/subjects/cg/versions?verbose=true", av(user2)),
    post("/subjects/cg/versions", av(user2, { metadata: { properties: {} } })),
    get("/subjects/cg/versions"),
    put("/config/cg", JSON.stringify({ compatibility: "BACKWARD_TRANSITIVE" })),
    post("/subjects/cg/versions", av({ ...user, fields: [{ name: "id", type: "int" }] }, { metadata: { properties: { major: "2" } } })),
    get("/config/cg"),
  ],

  // What Jackson refuses before anything else, rule sets included (the community image refuses these too).
  "data-contracts/errors": [
    post("/subjects/de/versions", av(user, { ruleSet: { domainRules: [rule("me", { kind: "NOPE" })] } })),
    post("/subjects/de/versions", av(user, { ruleSet: { domainRules: [rule("mf", { mode: "write" })] } })),
    post("/subjects/de/versions", av(user, { metadata: [] })),
    post("/subjects/de/versions", av(user, { metadata: "x" })),
    post("/subjects/de/versions", av(user, { metadata: 5 })),
    post("/subjects/de/versions", av(user, { metadata: { properties: { a: 1, b: true } } })),
    post("/subjects/de/versions", av(user, { metadata: { tags: { a: "x" } } })),
    post("/subjects/de/versions", av(user, { metadata: { unknown: 1, properties: { u: "v" } } })),
    get("/subjects/de/versions/latest"),
    post("/subjects/de/versions", av(user, { ruleSet: [] })),
    post("/subjects/de/versions", av(user, { ruleSet: { domainRules: {} } })),
    put("/config/de", JSON.stringify({ defaultMetadata: [] })),
    put("/config/de", JSON.stringify({ overrideMetadata: { properties: { z: "1", a: "2" }, tags: { t: ["y", "x"] } } })),
    get("/config/de"),
  ],

  // Rule sets, kept as Confluent Enterprise and Cloud keep them; the community image (the reference here) drops them
  // with "RuleSets are only supported by Confluent Enterprise and Confluent Cloud", so every step differs (REG-D30).
  "data-contracts/rule-sets": [
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("a"), rule("a")] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("a")], migrationRules: [rule("a", { mode: "UPGRADE" })] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [{ kind: "CONDITION", mode: "WRITE", type: "CEL" }] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("")] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("1abc")] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("a.b")] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("x".repeat(65))] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [{ name: "nt", kind: "CONDITION", mode: "WRITE" }] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("ma", { onFailure: "ERROR,NONE" })] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { domainRules: [rule("mc", { mode: "UPGRADE" })] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { migrationRules: [rule("md")] } })),
    post("/subjects/rs/versions", av(user, { ruleSet: { encodingRules: [rule("me", { mode: "UPGRADE" })] } })),
    // Valid: defaults filled in, unknown keys read past, sorted tags and params.
    post("/subjects/rs/versions", av(user, { metadata: { properties: { owner: "a" } }, ruleSet: { domainRules: [rule("r1", { params: { z: "1", a: "2" }, tags: ["t2", "t1"], unknown: 1 }), { name: "dflt", type: "CEL" }] } })),
    get("/subjects/rs/versions/latest"),
    get("/schemas/ids/@1"),
    // The next schema carries the rule set forward; one of its own replaces it.
    post("/subjects/rs/versions", av(user2)),
    get("/subjects/rs/versions/latest"),
    post("/subjects/rs", av(user2)),
    post("/subjects/rs/versions", av(user2, { ruleSet: { domainRules: [rule("r2", { mode: "WRITEREAD", onFailure: "ERROR,NONE", disabled: true, doc: "d" })] } })),
    get("/subjects/rs/versions/latest"),
    post("/subjects/rs/versions", av(user, { ruleSet: { encodingRules: [rule("enc", { mode: "WRITEREAD", type: "ENCRYPT_PAYLOAD" })] } })),
    get("/subjects/rs/versions/latest"),
    // Defaults and overrides of a configuration, merged by rule name.
    put("/config/rsc", JSON.stringify({ defaultRuleSet: { domainRules: [rule("d1"), rule("shared", { expr: "1 == 1" })] }, overrideRuleSet: { domainRules: [rule("shared", { expr: "2 == 2" })], migrationRules: [rule("m1", { mode: "UPGRADE", kind: "TRANSFORM", type: "JSONATA", expr: "$" })] } })),
    get("/config/rsc"),
    post("/subjects/rsc/versions", av(user, { ruleSet: { domainRules: [rule("own")] } })),
    get("/subjects/rsc/versions/latest"),
    put("/config/rsc", JSON.stringify({ defaultRuleSet: { domainRules: [rule("a"), rule("a")] } })),
    put("/config/rsc", JSON.stringify({ defaultRuleSet: { domainRules: [] } })),
    get("/config/rsc"),
  ],

  "data-contracts/import": [
    put("/mode/imp", JSON.stringify({ mode: "IMPORT" })),
    post("/subjects/imp/versions", av(user, { id: 7000, version: 1, metadata: { properties: { owner: "a" } } })),
    get("/subjects/imp/versions/1"),
    post("/subjects/imp/versions", av(user, { id: 7000, version: 2, metadata: { properties: { owner: "b" } } })),
    post("/subjects/imp/versions", av(user, { id: 7001, version: 2, metadata: { properties: { owner: "b" } } })),
    get("/subjects/imp/versions"),
    get("/schemas/ids/7001"),
    put("/mode/imp", JSON.stringify({ mode: "READWRITE" })),
    post("/subjects/imp/versions", av(user2)),
    get("/subjects/imp/versions/latest"),
  ],
};

Object.assign(gaps, contracts);

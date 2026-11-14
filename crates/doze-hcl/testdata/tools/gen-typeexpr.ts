// The typeexpr corpus: every branch of hcl's ext/typeexpr getType, plus nesting and odd syntax.
const exprs = [
  "string", "number", "bool", "any", "list", "map", "set", "object", "tuple", "optional", "null", "true", "foo", "String",
  "list(string)", "set(number)", "map(bool)", "list(any)", "map(list(string))", "list(list(list(number)))", "set(map(any))",
  "list()", "list(string, number)", "list(string, number, bool)", "map(string, string)", "set()",
  "string()", "number(1)", "bool(x)", "any()", "any(string)",
  "object({})", "object({a = string})", "object({a = string, b = number})", "object({ a = list(string)\n b = map(any) })",
  "object({a = optional(string)})", "object({a = optional(string, \"x\")})", "object({a = optional()})", "object({a = optional(string, 1, 2)})",
  "object({a = string, a = number})", "object({\"a\" = string})", "object({(a) = string})", "object({a.b = string})", "object({1 = string})",
  "object({a = foo})", "object({a = list()})", "object({a = optional(foo)})", "object({a = object({b = optional(number)})})",
  "object(string)", "object([string])", "object()", "object({}, {})", "object({a = string}, x)",
  "tuple([])", "tuple([string])", "tuple([string, number, bool])", "tuple([list(string), object({a = any})])", "tuple(string)", "tuple({a = string})", "tuple()", "tuple([], [])",
  "tuple([foo])", "tuple([string, list()])",
  "optional(string)", "optional(string, 1)", "foo(string)", "string.x", "var.x", "list[0]", "\"string\"", "(string)", "[string]", "{a = string}",
  "1", "-1", "string + 1", "list(string)[0]", "list(\"string\")", "map(string.foo)", "list(string...)",
  "upper(string)", "a::b(string)", "list(\n  string\n)", "object({\n  a = string\n  b = optional(number)\n})",
  "object({a = optional(string), b = optional(list(object({c = optional(bool)})))})",
  "map(object({name = string, port = optional(number)}))", "set(tuple([string, number]))",
  "list(optional(string))", "map(optional(string))", "tuple([optional(string)])",
  "object({a = optional(string, null)})", "object({a = optional(list(string), [])})", "object({a = optional(number, \"x\")})",
  "object({a = optional(number, var.x)})", "object({for k in x : k => string})", "list(for x in y : x)",
  "list(string) # comment", "map( string )", "object({ a: string })", "object({a = string,})",
  "list(any)", "map(any)", "object({a = any})", "tuple([any])",
  "LIST(string)", "List(string)", "list(String)", "number.x", "list(string).x", "any.x",
  "object({a = string, b = number, a = bool})", "object({a = object({a = object({a = string})})})",
];
const cases = exprs.map((expr, i) => ({ id: `te.${String(i).padStart(3, "0")}`, expr }));
await Bun.write(process.argv[2], JSON.stringify(cases, null, 1) + "\n");

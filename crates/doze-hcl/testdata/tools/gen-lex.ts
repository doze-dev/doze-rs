// Generates lexer/parser stress sources: random sequences of HCL fragments, seeded.
// bun gen-lex.ts <n> <seed> <prefix> > cases.json
const n = Number(process.argv[2] ?? 1000);
let seed = Number(process.argv[3] ?? 1);
const prefix = process.argv[4] ?? "lx";
const rnd = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const pick = <T>(a: T[]): T => a[Math.floor(rnd() * a.length)];
const frags = [
  "x", "a_b", "a-b", "é", "ß", "_", "for", "in", "if", "else", "endif", "endfor", "true", "null",
  "1", "0.5", "1.", "1..2", "1e5", "1e+", "2E-3", "007", "1.e5",
  " ", "  ", "\t", "\n", "\r\n", "\r", "#c\n", "// c\n", "/* c */", "/*", "*/", "#",
  "=", "==", "!=", "=>", "<", "<=", ">", ">=", "!", "&&", "||", "&", "|", "^", "~", "~}", "::", "...", "..", ".",
  ",", ":", "?", ";", "`", "'", "+", "-", "*", "**", "/", "%", "(", ")", "[", "]", "{", "}", "[*]", ".*",
  "\"", "\"a\"", "\"a\\nb\"", "\"\\u00e9\"", "\"\\x\"", "\"\\", "\\", "$", "$$", "${", "$${", "%{", "%%{", "${x}", "%{ if x }", "%{ endif }",
  "%{for a in b}", "%{endfor}", "%{~", "${~", "<<EOT\n", "<<-EOT\n", "EOT\n", "  EOT\n", "EOT", "<<EOT", "<<", "<<-x\r\n", "x\n",
  "“", "”", "\u0301", "👍🏽", "\u00a0", "\u2028", "€", "\u200d",
];
const out: { id: string; src: string }[] = [];
for (let i = 0; i < n; i++) {
  const len = 1 + Math.floor(rnd() * 14);
  let s = rnd() < 0.5 ? "x = " : "";
  for (let j = 0; j < len; j++) s += pick(frags);
  if (rnd() < 0.5) s += "\n";
  out.push({ id: `${prefix}.${String(i).padStart(4, "0")}`, src: s });
}
console.log(JSON.stringify(out, null, 0));

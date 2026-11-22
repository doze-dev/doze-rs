// A small, safe reader of the Markdown a text widget holds: headings, paragraphs, bullet lists, fenced code, and in a line bold,
// italic, code and http(s) links. It makes a tree for the page to draw; nothing here is HTML, so nothing in the text can inject any.

export type Inline = { t: "text"; v: string } | { t: "b" | "i" | "code"; v: string } | { t: "a"; v: string; href: string };
export type Block = { t: "h"; level: 1 | 2 | 3; inline: Inline[] } | { t: "p"; inline: Inline[] } | { t: "ul"; items: Inline[][] } | { t: "pre"; v: string };

const INLINE = /(\*\*[^*]+\*\*|`[^`]+`|\*[^*\s][^*]*\*|\[[^\]]+\]\(https?:\/\/[^)\s]+\))/g;

export function inline(text: string): Inline[] {
  return text.split(INLINE).filter((p) => p !== "").map((p): Inline => {
    if (p.startsWith("**") && p.endsWith("**") && p.length > 4) return { t: "b", v: p.slice(2, -2) };
    if (p.startsWith("`") && p.endsWith("`") && p.length > 2) return { t: "code", v: p.slice(1, -1) };
    if (p.startsWith("*") && p.endsWith("*") && p.length > 2) return { t: "i", v: p.slice(1, -1) };
    const link = /^\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)$/.exec(p);
    if (link) return { t: "a", v: link[1]!, href: link[2]! };
    return { t: "text", v: p };
  });
}

export function blocks(source: string): Block[] {
  const out: Block[] = [];
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  let para: string[] = [];
  let list: string[] | null = null;
  const flush = () => {
    if (para.length) out.push({ t: "p", inline: inline(para.join(" ")) });
    if (list) out.push({ t: "ul", items: list.map(inline) });
    para = [];
    list = null;
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    if (line.startsWith("```")) {
      flush();
      const code: string[] = [];
      for (i++; i < lines.length && !lines[i]!.startsWith("```"); i++) code.push(lines[i]!);
      out.push({ t: "pre", v: code.join("\n") });
      continue;
    }
    const h = /^(#{1,3})\s+(.*)$/.exec(line);
    const li = /^\s*[-*]\s+(.*)$/.exec(line);
    if (h) {
      flush();
      out.push({ t: "h", level: h[1]!.length as 1 | 2 | 3, inline: inline(h[2]!) });
    } else if (li) {
      if (para.length) flush();
      (list ??= []).push(li[1]!);
    } else if (line.trim() === "") flush();
    else {
      if (list) flush();
      para.push(line.trim());
    }
  }
  flush();
  return out;
}

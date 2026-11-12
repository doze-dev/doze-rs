// A tiny DOM for the board tooling, built on bun's HTMLRewriter (no npm packages).
//
// HTMLRewriter is a streaming tokenizer, not a tree builder, so this file turns its events into a tree:
// elements are pushed on a stack at their start tag and popped at their end tag (a void element has no end tag;
// an element whose end tag is missing is closed when its parent's end tag arrives). The boards are balanced
// (the canvas build checks it) and so is maud's output, so the simple rules are enough.
//
// Used by board-signature.ts (boards and rendered pages) and a11y-lint.ts.

export interface El {
  tag: string;
  attrs: Record<string, string>;
  children: Array<El | string>;
  parent: El | null;
}

const VOID = new Set([
  "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr",
]);

const ENTITIES: Record<string, string> = {
  amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", nbsp: " ", middot: "·", hellip: "…",
  ndash: "–", mdash: "—", rarr: "→", larr: "←", times: "×", copy: "©",
};

export function decodeEntities(s: string): string {
  if (!s.includes("&")) return s;
  return s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, e: string) => {
    if (e[0] === "#") {
      const code = e[1] === "x" || e[1] === "X" ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10);
      return Number.isFinite(code) && code > 0 && code < 0x110000 ? String.fromCodePoint(code) : m;
    }
    return ENTITIES[e.toLowerCase()] ?? m;
  });
}

/** Parse a document or a fragment. The result is a synthetic `#root` element. */
export function parse(html: string): El {
  const root: El = { tag: "#root", attrs: {}, children: [], parent: null };
  let top = root;
  new HTMLRewriter()
    .on("*", {
      element(e) {
        const el: El = { tag: e.tagName.toLowerCase(), attrs: {}, children: [], parent: top };
        for (const [k, v] of e.attributes) el.attrs[k.toLowerCase()] = decodeEntities(v);
        top.children.push(el);
        if (VOID.has(el.tag)) return;
        top = el;
        try {
          e.onEndTag(() => {
            // Close any child left open by a missing end tag, then this element.
            let cur: El | null = top;
            while (cur && cur !== el) cur = cur.parent;
            if (cur === el) top = el.parent ?? root;
          });
        } catch {
          top = el.parent ?? root; // no end tag possible (void by another name)
        }
      },
      text(t) {
        if (!t.text) return;
        const last = top.children[top.children.length - 1];
        const piece = top.tag === "script" || top.tag === "style" ? t.text : decodeEntities(t.text);
        if (typeof last === "string") top.children[top.children.length - 1] = last + piece;
        else top.children.push(piece);
      },
    })
    .transform(html);
  return root;
}

export const classes = (el: El): string[] => (el.attrs.class ?? "").split(/\s+/).filter(Boolean);
export const hasClass = (el: El, c: string): boolean => classes(el).includes(c);

/** Every element below `el` in document order (not `el` itself). */
export function* descendants(el: El): Generator<El> {
  for (const c of el.children) {
    if (typeof c === "string") continue;
    yield c;
    yield* descendants(c);
  }
}

export function find(el: El, pred: (e: El) => boolean): El | null {
  for (const d of descendants(el)) if (pred(d)) return d;
  return null;
}

export function findAll(el: El, pred: (e: El) => boolean): El[] {
  const out: El[] = [];
  for (const d of descendants(el)) if (pred(d)) out.push(d);
  return out;
}

export function closest(el: El, pred: (e: El) => boolean): El | null {
  for (let c: El | null = el; c; c = c.parent) if (pred(c)) return c;
  return null;
}

export const isInside = (el: El, pred: (e: El) => boolean): boolean => closest(el.parent ?? el, pred) !== null && el.parent !== null;

/**
 * Raw text of a subtree. `sub` is asked about every descendant element: return a string to put in place of the
 * element and its content (an empty string drops it), or undefined to descend into it. Scripts, styles and
 * SVG never contribute text.
 */
export function rawText(el: El, sub?: (e: El) => string | undefined): string {
  let out = "";
  for (const c of el.children) {
    if (typeof c === "string") {
      out += c;
      continue;
    }
    if (c.tag === "script" || c.tag === "style" || c.tag === "svg") continue;
    const s = sub?.(c);
    if (s !== undefined) out += s;
    else out += rawText(c, sub) + (BLOCK.has(c.tag) ? " " : "");
  }
  return out;
}

// A block-level element ends a word: "<div>a</div><div>b</div>" reads "a b".
const BLOCK = new Set(["div", "p", "li", "dt", "dd", "tr", "td", "th", "h1", "h2", "h3", "h4", "h5", "h6", "pre", "br", "table", "ul", "ol", "dl", "section"]);

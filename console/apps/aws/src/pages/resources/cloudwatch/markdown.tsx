import { type Block, type Inline, blocks } from "./markdown-parse";

const span = (x: Inline, i: number) =>
  x.t === "b" ? <strong key={i} className="font-semibold text-bright">{x.v}</strong>
  : x.t === "i" ? <em key={i}>{x.v}</em>
  : x.t === "code" ? <code key={i} className="rounded bg-muted px-1 py-0.5 font-mono text-[0.92em]">{x.v}</code>
  : x.t === "a" ? <a key={i} href={x.href} target="_blank" rel="noreferrer noopener" className="text-link hover:underline">{x.v}</a>
  : <span key={i}>{x.v}</span>;

const block = (b: Block, i: number) =>
  b.t === "h" ? <p key={i} className={b.level === 1 ? "text-[17px] font-semibold text-bright" : "text-[15px] font-semibold text-bright"}>{b.inline.map(span)}</p>
  : b.t === "ul" ? <ul key={i} className="ml-5 list-disc">{b.items.map((it, j) => <li key={j}>{it.map(span)}</li>)}</ul>
  : b.t === "pre" ? <pre key={i} className="overflow-auto rounded-md bg-well px-3 py-2 font-mono text-[12.5px]">{b.v}</pre>
  : <p key={i}>{b.inline.map(span)}</p>;

/** The Markdown of a text widget, drawn. */
export function Markdown({ source }: { source: string }) {
  return <div className="grid gap-3 text-[14px] leading-6 text-foreground" data-testid="markdown">{blocks(source).map(block)}</div>;
}

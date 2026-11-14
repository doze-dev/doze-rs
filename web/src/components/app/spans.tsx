import type { Span } from "@/api/types/Span";

/** A sentence the server built: plain runs, names in the monospace face, a figure in bold. Text only (React escapes it). */
export function Spans({ spans }: { spans: Span[] }) {
  return (
    <>
      {spans.map((s, i) =>
        s.kind === "mono" ? (
          <code key={i} className="font-mono text-[0.92em] text-foreground">
            {s.text}
          </code>
        ) : s.kind === "strong" ? (
          <b key={i} className="font-semibold text-foreground">
            {s.text}
          </b>
        ) : (
          <span key={i}>{s.text}</span>
        ),
      )}
    </>
  );
}

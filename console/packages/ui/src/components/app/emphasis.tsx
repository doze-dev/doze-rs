import { Fragment } from "react";
import { emphasise, type Targets } from "../../lib/emphasis";

/** A sentence with its resource names (monospace, bold) and its figures (bold) set apart, as the summary sentences of the boards are.
 * Plain text where nothing matches; see {@link emphasise} for what matches. */
export function Emphasis({ text, names, bold }: { text: string } & Targets) {
  return (
    <>
      {emphasise(text, { names, bold }).map((p, i) =>
        p.kind === "name" ? (
          <span key={i} className="font-mono font-semibold text-bright" data-slot="emphasis-name">{p.text}</span>
        ) : p.kind === "bold" ? (
          <strong key={i} className="font-semibold text-bright" data-slot="emphasis-bold">{p.text}</strong>
        ) : (
          <Fragment key={i}>{p.text}</Fragment>
        ),
      )}
    </>
  );
}

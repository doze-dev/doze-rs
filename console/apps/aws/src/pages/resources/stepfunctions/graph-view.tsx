// A state machine drawn as an SVG from its definition (graph.ts lays it out), the path an execution took coloured by status
// (history.ts reads it). A state is a button: choosing one shows its input, output and error beside the drawing.
import { useMemo, useState } from "react";
import { cn } from "@doze/ui/lib/utils";
import { END, START, type Box, type Edge, type Graph, layout, type Definition } from "./graph";
import type { EdgeRef } from "./design-model";
import type { Run, StepStatus } from "./history";

const TONE: Record<StepStatus | "idle", { box: string; text: string }> = {
  succeeded: { box: "fill-tint-good stroke-good", text: "fill-good" },
  failed: { box: "fill-bad/10 stroke-bad", text: "fill-bad" },
  running: { box: "fill-tint-cool stroke-cool", text: "fill-cool" },
  cancelled: { box: "fill-tint-warn stroke-warn", text: "fill-warn" },
  idle: { box: "fill-card stroke-border", text: "fill-muted-foreground" },
};

const WORD: Record<StepStatus, string> = { succeeded: "Succeeded", failed: "Failed", running: "In progress", cancelled: "Cancelled" };

/** A path through the points: straight down, a gentle S when it moves sideways; a loop as drawn. */
function pathOf(e: Edge): string {
  const [a, ...rest] = e.points;
  if (!a) return "";
  if (e.back || rest.length !== 1) return `M${a[0]},${a[1]} ${rest.map((p) => `L${p[0]},${p[1]}`).join(" ")}`;
  const b = rest[0];
  const mid = (a[1] + b[1]) / 2;
  return `M${a[0]},${a[1]} C${a[0]},${mid} ${b[0]},${mid} ${b[0]},${b[1]}`;
}

/** Design mode: where a state may be dropped (the middle of an edge), what is dragged or chosen, and what dropping does. */
export type Design = { active: boolean; onDrop: (edge: EdgeRef, offer: string) => void; armed: string | null };
type Look = { run?: Run; selected?: string; onSelect?: (name: string) => void; design?: Design; scope?: string };

function nameOf(g: Graph, id: string): string {
  if (id.endsWith(START)) return START;
  if (id.endsWith(END)) return END;
  return g.boxes.find((b) => b.id === id)?.name ?? id;
}

/** The edge as the definition has it, for dropping a state on it; `null` for one nothing may be dropped on (a catch, a loop, what follows a state that ends). */
function refOf(g: Graph, e: Edge, scope: string): EdgeRef | null {
  if (e.kind === "catch" || e.back) return null;
  const from = nameOf(g, e.from);
  const to = nameOf(g, e.to);
  const same = g.edges.filter((x) => x.kind === e.kind && x.from === e.from);
  const nth = same.indexOf(e);
  if (e.kind === "start") return { scope, from: null, to: to === END ? null : to, kind: "start", nth: 0 };
  const fromType = g.boxes.find((b) => b.id === e.from)?.type;
  if (e.kind === "end") return fromType === "Succeed" || fromType === "Fail" ? null : { scope, from, to: null, kind: "end", nth: 0 };
  return { scope, from, to: to === END ? null : to, kind: e.kind, nth };
}

function DropZone({ e, edge, design }: { e: Edge; edge: EdgeRef; design: Design }) {
  const [over, setOver] = useState(false);
  const [a, ...rest] = e.points;
  const b = rest[rest.length - 1];
  if (!a || !b) return null;
  const x = (a[0] + b[0]) / 2;
  const y = (a[1] + b[1]) / 2;
  return (
    <g
      transform={`translate(${x},${y})`}
      data-testid="drop-zone"
      data-edge={`${edge.from ?? "start"}>${edge.to ?? "end"}`}
      className={cn("cursor-copy", !design.active && !design.armed && !over && "opacity-0 hover:opacity-100")}
      onDragOver={(ev) => { ev.preventDefault(); setOver(true); }}
      onDragLeave={() => setOver(false)}
      onDrop={(ev) => { ev.preventDefault(); setOver(false); const id = ev.dataTransfer.getData("text/plain"); if (id) design.onDrop(edge, id); }}
      onClick={() => design.armed && design.onDrop(edge, design.armed)}
    >
      <circle r={13} className={cn("stroke-primary", over ? "fill-primary" : "fill-card")} strokeWidth={1.5} strokeDasharray={over ? undefined : "3 3"} />
      <path d="M-5,0 H5 M0,-5 V5" className={over ? "stroke-primary-foreground" : "stroke-primary"} strokeWidth={1.75} strokeLinecap="round" />
    </g>
  );
}

function EdgeLine({ g, e, run, design, scope = "" }: { g: Graph; e: Edge } & Look) {
  const from = nameOf(g, e.from);
  const to = nameOf(g, e.to);
  const status = (n: string) => run?.steps.get(n)?.status;
  const taken = !!run && (run.taken.has(`${from}>${to}`) || (e.kind === "start" && !!status(to)) || (e.kind === "end" && status(from) === "succeeded"));
  return (
    <g data-edge={`${from}>${to}`} data-taken={taken || undefined}>
      <path d={pathOf(e)} fill="none" className={cn(taken ? "stroke-good" : "stroke-border", e.kind === "catch" && "[stroke-dasharray:4_4]")} strokeWidth={taken ? 2 : 1.5} markerEnd={`url(#${taken ? "arrow-taken" : "arrow"})`} />
      {e.label && e.kind !== "next" && <EdgeLabel e={e} text={e.label.length > 22 ? `${e.label.slice(0, 21)}…` : e.label} />}
      {design && refOf(g, e, scope) && <DropZone e={e} edge={refOf(g, e, scope) as EdgeRef} design={design} />}
    </g>
  );
}

/** Where an edge's words go, beside its line and never on it: a loop's at its far side, a lane's along the lane (turned, on its
 * outer side), a straight one's just under its state, on the side its line does not bend to. A halo of the card keeps them
 * legible where another line passes. */
function EdgeLabel({ e, text }: { e: Edge; text: string }) {
  const [a, b, c, d] = e.points;
  const look = "fill-muted-foreground stroke-card font-mono text-[10.5px] [paint-order:stroke]";
  if (!a || !b) return null;
  if (e.back) {
    const y = ((b[1] ?? 0) + (c?.[1] ?? b[1])) / 2;
    return <text x={b[0] - 5} y={y} textAnchor="end" strokeWidth={3} className={look}>{text}</text>;
  }
  if (c && d) {
    const x = c[0] - 5;
    const y = (c[1] + d[1]) / 2;
    return <text x={x} y={y} transform={`rotate(-90 ${x} ${y})`} textAnchor="middle" strokeWidth={3} className={look} data-label="lane">{text}</text>;
  }
  // On the side the line does not bend to; but the lines of a choice leave side by side and bend apart, so each keeps to its own outer side
  // (on the inner sides the two labels would meet under the box).
  const apart = e.kind === "choice" || e.kind === "default";
  const right = apart ? b[0] > a[0] + 1 : !(b[0] > a[0] + 1);
  return <text x={a[0] + (right ? 6 : -6)} y={a[1] + 13} textAnchor={right ? "start" : "end"} strokeWidth={3} className={look}>{text}</text>;
}

function StateBox({ b, run, selected, onSelect, design }: { b: Box } & Look) {
  if (b.id.endsWith(START) || b.id.endsWith(END)) {
    return (
      <g>
        <rect x={b.x} y={b.y} width={b.w} height={b.h} rx={b.h / 2} className="fill-well stroke-border" strokeWidth={1} />
        <text x={b.x + b.w / 2} y={b.y + b.h / 2 + 4} textAnchor="middle" className="fill-foreground text-[12px]">
          {b.name}
        </text>
      </g>
    );
  }
  const step = run?.steps.get(b.name);
  const tone = TONE[step?.status ?? "idle"];
  const on = selected === b.name;
  const container = b.inner.length > 0;
  return (
    <g>
      <g
        role="button"
        tabIndex={0}
        aria-label={`${b.name}, ${b.type}${step ? `, ${WORD[step.status]}` : ""}`}
        aria-pressed={on}
        data-state={b.name}
        data-status={step?.status ?? "idle"}
        className="cursor-pointer outline-none focus-visible:[&>rect]:stroke-ring"
        onClick={() => onSelect?.(b.name)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelect?.(b.name);
          }
        }}
      >
        <rect x={b.x} y={b.y} width={b.w} height={b.h} rx={10} className={cn(tone.box, container && !step && "fill-transparent", container && "[stroke-dasharray:5_4]")} strokeWidth={on ? 2.5 : 1.25} />
        <text x={b.x + 14} y={b.y + (container ? 19 : 20)} className="fill-bright font-mono text-[12.5px] font-medium">
          {b.name}
        </text>
        <text x={b.x + 14} y={b.y + (container ? 19 : 36)} dx={container ? b.name.length * 7.6 + 10 : 0} className={cn("text-[11px]", step ? tone.text : "fill-muted-foreground")}>
          {b.label ?? b.type}
          {step && container ? ` · ${WORD[step.status]}` : ""}
        </text>
        {step && !container && (
          <text x={b.x + b.w - 14} y={b.y + b.h / 2 + 4} textAnchor="end" className={cn("text-[12px] font-semibold", tone.text)} data-testid="state-status">
            <tspan>● </tspan>{WORD[step.status]}
          </text>
        )}
      </g>
      {b.inner.map((p, i) => (
        <g key={p.title} transform={`translate(${p.x},${p.y})`}>
          <GraphBody g={p.graph} run={run} selected={selected} onSelect={onSelect} design={design} scope={`${b.id}/${i}/`} />
        </g>
      ))}
    </g>
  );
}

function GraphBody({ g, ...look }: { g: Graph } & Look) {
  return (
    <>
      {g.edges.map((e) => (
        <EdgeLine key={`${e.from}>${e.to}>${e.kind}`} g={g} e={e} {...look} />
      ))}
      {g.boxes.map((b) => (
        <StateBox key={b.id} b={b} {...look} />
      ))}
    </>
  );
}

/** The drawing: as wide as it needs, scaled down to its card, never up. */
export function GraphView({ definition, targets = false, ...look }: { definition: Definition; targets?: boolean } & Look) {
  const g = useMemo(() => layout(definition, "", { targets }), [definition, targets]);
  const pad = 12;
  return (
    <div className="overflow-x-auto" data-testid="sfn-graph">
      <svg viewBox={`${-pad} ${-pad} ${g.w + 2 * pad} ${g.h + 2 * pad}`} width={g.w + 2 * pad} className="mx-auto block h-auto max-w-full" role="group" aria-label="State machine graph">
        <defs>
          {(["arrow", "arrow-taken"] as const).map((id) => (
            <marker key={id} id={id} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
              <path d="M0,0 L10,5 L0,10 z" className={id === "arrow" ? "fill-border" : "fill-good"} />
            </marker>
          ))}
        </defs>
        <GraphBody g={g} {...look} />
      </svg>
    </div>
  );
}

/** The legend under the drawing. */
export function Legend() {
  return (
    <div className="mt-4 flex flex-wrap items-center gap-x-5 gap-y-2 border-t pt-4 text-[13px]" data-testid="sfn-legend">
      <span className="text-muted-foreground">Legend</span>
      <span className="inline-flex items-center gap-1.5 font-semibold text-good"><span className="size-2 rounded-full bg-current" />Succeeded</span>
      <span className="inline-flex items-center gap-1.5 font-semibold text-bad"><span className="size-2 rounded-full bg-current" />Failed</span>
      <span className="inline-flex items-center gap-1.5 font-semibold text-cool"><span className="size-2 rounded-full bg-current" />In progress</span>
      <span className="inline-flex items-center gap-1.5 font-semibold text-warn"><span className="size-2 rounded-full bg-current" />Cancelled</span>
      <span className="inline-flex items-center gap-1.5 text-muted-foreground"><span className="size-2 rounded-full border border-current" />Not run</span>
    </div>
  );
}

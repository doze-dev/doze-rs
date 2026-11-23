import { Link } from "@tanstack/react-router";
import { ExternalLinkIcon, XIcon } from "lucide-react";
import { createElement, useMemo } from "react";
import { Button } from "@doze/ui/components/ui/button";
import { cn } from "@doze/ui/lib/utils";
import { number } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";
import { iconOf } from "../resources/model";
import { type MapModel, type MapNode, perMinute, targetOf, wiresOf } from "./graph";
import { waitingHeadline } from "./panel-words";

const KIND: Record<string, string> = { queues: "queue", topics: "topic", buckets: "bucket", functions: "function", tables: "table", buses: "event bus", streams: "stream", machines: "state machine" };
export const kindWord = (n: MapNode): string => KIND[n.resKind] ?? (n.kind === "service" ? "service" : "resource");

/** Where the Resources space shows a node: by its kind when the wiring said it, else by name and the member the call named it by. */
export function OpenLink({ n, view, children, ...rest }: { n: MapNode; view?: string; children: React.ReactNode } & Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, "children">) {
  if (n.kind === "service") return <Link to="/resources/$service" params={{ service: n.service }} {...rest}>{children}</Link>;
  if (n.resKind) return <Link to="/resources/$service/$kind/$name" params={{ service: n.service, kind: n.resKind, name: n.name }} search={view ? { view } : {}} {...rest}>{children}</Link>;
  return <Link to="/resources/$service/$second" params={{ service: n.service, second: n.name }} search={n.member ? { m: n.member } : {}} {...rest}>{children}</Link>;
}

function Spark({ counts }: { counts: number[] }) {
  const peak = Math.max(1, ...counts);
  const W = 300;
  const H = 54;
  const bar = W / counts.length;
  return (
    <svg width="100%" viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`Calls per minute: the busiest minute had ${number(Math.max(...counts))}`} data-testid="node-spark" className="rounded-md border bg-muted/40">
      {counts.map((n, i) => (
        <rect key={i} x={i * bar + 0.5} y={H - 2 - (n / peak) * (H - 6)} width={Math.max(1, bar - 1.5)} height={(n / peak) * (H - 6) + (n > 0 ? 0 : 0)} rx={1} fill="var(--primary)" opacity={n > 0 ? 0.8 : 0} />
      ))}
      <line x1="0" x2={W} y1={H - 1} y2={H - 1} stroke="var(--border)" />
    </svg>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline gap-4 border-b py-2.5 text-[13px] last:border-b-0">
      <dt className="w-28 shrink-0 text-muted-foreground">{label}</dt>
      <dd className="min-w-0 flex-1">{children}</dd>
    </div>
  );
}

/** The panel of one node: what it holds now, the calls to it by the minute, what feeds it and where it sends, who calls it, and
 * the verbs of its kind. `onPick` selects another node (a name in the panel). */
export function NodePanel({ node, model, calls, now, onPick, onClose }: { node: MapNode; model: MapModel; calls: readonly CallRecord[]; now: number; onPick: (id: string) => void; onClose: () => void }) {
  const w = wiresOf(model, node.id);
  const mine = useMemo(() => calls.filter((c) => !c.via && targetOf(c)?.id === node.id), [calls, node.id]);
  const counts = useMemo(() => perMinute(mine, now, 30), [mine, now]);
  const name = (n: MapNode) => (
    <button type="button" onClick={() => onPick(n.id)} className="rounded-sm font-mono text-[12.5px] text-bright hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" data-testid="panel-link">
      {n.name}
    </button>
  );
  const into = (label: string) => w.fedBy.filter((x) => x.edge.label === label);
  const out = (label: string) => w.out.filter((x) => x.edge.label === label);
  const reads = w.callers.filter((a) => a.ops.some((o) => /^(Receive|Get|Delete|Read)/.test(o)));
  const writes = w.callers.filter((a) => !reads.includes(a));
  const queue = node.resKind === "queues";
  const headline = queue ? waitingHeadline(node.sentence) : null;
  return (
    <aside className="flex min-w-0 flex-col rounded-xl border bg-card p-5" aria-label={`${node.name}, ${kindWord(node)}`} data-testid="node-panel" data-node={node.id}>
      <div className="flex items-start gap-3">
        <span className="inline-flex size-9 shrink-0 items-center justify-center rounded-[10px] border bg-muted">
          {createElement(iconOf(node.service), { "aria-hidden": true, className: "size-[18px] text-primary" })}
        </span>
        <div className="min-w-0 flex-1">
          <h2 className="truncate font-mono text-[17px] font-semibold text-bright" data-testid="panel-title">
            {node.name}
          </h2>
          <span className="rounded-full border px-2 py-px text-[11px] text-muted-foreground">{kindWord(node)}</span>
        </div>
        <button type="button" onClick={onClose} aria-label="Close the panel" className="rounded-md p-1 text-muted-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring" data-testid="panel-close">
          <XIcon className="size-4" />
        </button>
      </div>
      {headline && (
        <div className="mt-4" data-testid="panel-headline">
          <div className="flex items-baseline gap-3"><span className="text-[64px] leading-none font-light tracking-[-0.03em] text-bright tabular-nums" data-testid="panel-count">{headline.count}</span><span className="text-[20px] font-light text-muted-foreground">waiting</span></div>
          {headline.rest && <p className="mt-2 text-[13.5px] text-muted-foreground" data-testid="panel-sentence">{headline.rest}</p>}
        </div>
      )}
      {!headline && <p className="mt-3 text-[13.5px]" data-testid="panel-sentence">
        {node.sentence ?? (node.calls > 0 ? `${number(node.calls)} ${node.calls === 1 ? "call" : "calls"} held, the newest ${Math.max(0, Math.round((now - node.lastMs) / 1000))} s ago.` : "Nothing has called it.")}
      </p>}
      {node.calls > 0 && (
        <div className="mt-4">
          <div className="mb-1.5 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">Calls to it, per minute</div>
          <Spark counts={counts} />
          <div className="mt-0.5 flex justify-between text-[11px] text-muted-foreground">
            <span>30 min ago</span>
            <span>now</span>
          </div>
        </div>
      )}
      <dl className="mt-4">
        {w.fedBy.some((x) => x.edge.label !== "redrive") && <Row label="Fed by">{w.fedBy.filter((x) => x.edge.label !== "redrive" && x.edge.label !== "polling").map((x) => <span key={x.edge.id} className="mr-3 inline-flex items-baseline gap-1.5">{name(x.node)}{x.edge.note && <span className="text-[12px] text-muted-foreground">{x.edge.note}</span>}</span>)}</Row>}
        {into("redrive").length > 0 && <Row label="Dead-letter queue of">{into("redrive").map((x) => <span key={x.edge.id} className="mr-3 inline-flex items-baseline gap-1.5">{name(x.node)}<span className="text-[12px] text-muted-foreground">{x.edge.note}</span></span>)}</Row>}
        {out("redrive").length > 0 && <Row label="Dead letters to">{out("redrive").map((x) => <span key={x.edge.id} className="mr-3 inline-flex items-baseline gap-1.5">{name(x.node)}<span className="text-[12px] text-muted-foreground">{x.edge.note}</span></span>)}</Row>}
        {w.out.some((x) => x.edge.label !== "redrive") && <Row label="Sends to">{w.out.filter((x) => x.edge.label !== "redrive").map((x) => <span key={x.edge.id} className="mr-3 inline-flex items-baseline gap-1.5">{name(x.node)}<span className="text-[12px] text-muted-foreground">{[x.edge.label, x.edge.note].filter(Boolean).join(" · ")}</span></span>)}</Row>}
        {queue && reads.length > 0 && <Row label="Read by">{reads.map((a) => <span key={a.id} className="mr-3 text-bright">{a.name}</span>)}</Row>}
        {queue && writes.length > 0 && <Row label="Written by">{writes.map((a) => <span key={a.id} className="mr-3 text-bright">{a.name}</span>)}</Row>}
        {!queue && w.callers.length > 0 && <Row label="Called by">{w.callers.map((a) => <span key={a.id} className="mr-3 text-bright">{a.name}</span>)}</Row>}
      </dl>
      <div className={cn("mt-5 flex flex-wrap gap-2")} data-testid="panel-verbs">
        {queue && (
          <>
            <Button asChild size="sm" data-testid="panel-send">
              <OpenLink n={node} view="messages">Send a message</OpenLink>
            </Button>
            <Button asChild size="sm" variant="outline" className="bg-card" data-testid="panel-peek">
              <OpenLink n={node} view="messages">Peek</OpenLink>
            </Button>
          </>
        )}
        <Button asChild size="sm" variant="outline" className="bg-card" data-testid="panel-open">
          <OpenLink n={node}>
            Open {kindWord(node)} <ExternalLinkIcon />
          </OpenLink>
        </Button>
      </div>
      {queue && <p className="mt-4 text-[12.5px] text-muted-foreground">The number is messages waiting. In-flight ones are hidden, not gone.</p>}
    </aside>
  );
}

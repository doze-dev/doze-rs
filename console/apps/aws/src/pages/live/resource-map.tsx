// The resource map: apps on the left, the resources they called and what those are wired to across, a card for each with what it
// holds now, a line for each call route and each configured wire, and a bead along a route for each call as it arrives. SVG for
// the lines and one <circle> added by hand per bead (a frame loop runs only while a bead is in flight); with reduced motion the
// node it reached pulses once instead.
import { createElement, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { BotIcon } from "lucide-react";
import { cn } from "@doze/ui/lib/utils";
import { number } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";
import { iconOf } from "../resources/model";
import { type MapEdge, type MapModel, type MapNode, targetOf } from "./graph";
import { beadColour } from "./map";
import { callerOf } from "./search";

export const CARD_W = 190;
const CARD_H = 62;
const SVG = "http://www.w3.org/2000/svg";
const reduced = () => typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;

function useWidth(ref: React.RefObject<HTMLElement | null>): number {
  const [w, setW] = useState(960);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setW(el.clientWidth);
    const ro = new ResizeObserver(([e]) => e && setW(Math.round(e.contentRect.width)));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return w;
}

/** The mark on a node that something went wrong at: soft IAM, a refusal, a failure, an operation not built. */
export function Marks({ n }: { n: MapNode }) {
  const s = n.seen;
  const dot = (c: string, what: string) => <span key={what} title={what} aria-label={what} className={cn("inline-block size-2 rounded-full", c)} />;
  return (
    <span className="inline-flex gap-1" data-testid="node-marks">
      {s.wouldBeDenied > 0 && dot("bg-warn", `${number(s.wouldBeDenied)} would be denied`)}
      {s.refused > 0 && dot("bg-refused", `${number(s.refused)} refused`)}
      {s.error > 0 && dot("bg-bad", `${number(s.error)} failed`)}
      {s.notBuilt > 0 && dot("bg-muted-foreground", `${number(s.notBuilt)} not built`)}
    </span>
  );
}

/** What a node says under its name. An app: the operations it calls and how many calls in the last minute. A resource: what it
 * holds now, else what was done to it. */
export function nodeLine(n: MapNode): { top: string; bottom: string | null } {
  if (n.kind === "app") return { top: n.ops.slice(0, 2).join(" / ") + (n.ops.length > 2 ? " …" : ""), bottom: `${number(n.lastMinute)} in the last minute` };
  return { top: n.sentence ?? (n.calls > 0 ? `${number(n.calls)} ${n.calls === 1 ? "call" : "calls"}` : ""), bottom: n.sentence && n.calls > 0 ? `${number(n.calls)} ${n.calls === 1 ? "call" : "calls"} · ${n.ops.slice(0, 2).join(" / ")}` : null };
}

function Card({ n, x, selected, dim, onPick }: { n: MapNode; x: number; selected: boolean; dim: boolean; onPick: () => void }) {
  const line = nodeLine(n);
  return (
    <button
      type="button"
      onClick={onPick}
      aria-pressed={selected}
      data-testid={n.kind === "app" ? "map-app" : "map-resource"}
      data-node={n.id}
      className={cn("absolute flex -translate-y-1/2 flex-col justify-center gap-0.5 rounded-xl border bg-card px-3 py-2 text-left shadow-xs outline-none transition-colors hover:border-input focus-visible:ring-2 focus-visible:ring-ring", selected && "border-primary ring-2 ring-primary/30", dim && "opacity-40", n.kind === "service" && "border-dashed")}
      style={{ left: x, top: n.y, width: CARD_W, minHeight: CARD_H }}
    >
      <span className="flex items-center gap-2">
        {createElement(n.kind === "app" ? BotIcon : iconOf(n.service), { "aria-hidden": true, className: "size-4 shrink-0 text-primary" })}
        <span className="min-w-0 flex-1 truncate font-mono text-[13px] font-semibold text-bright" title={n.name}>
          {n.name}
        </span>
        <Marks n={n} />
      </span>
      {line.top && <span className="truncate text-[12px] text-muted-foreground">{line.top}</span>}
      {line.bottom && <span className="truncate text-[11.5px] text-muted-foreground">{line.bottom}</span>}
    </button>
  );
}

type Props = { heads?: string[]; model: MapModel; calls: readonly CallRecord[]; selected: string | null; route: { nodes: Set<string>; edges: Set<string> } | null; onPick: (id: string | null) => void };

/** The map. `calls`: the tape, newest first; each new one sends a bead along its route. */
export function ResourceMap({ heads, model, calls, selected, route, onPick }: Props) {
  const box = useRef<HTMLDivElement>(null);
  const width = useWidth(box);
  const paths = useRef(new Map<string, SVGPathElement>());
  const layer = useRef<SVGGElement>(null);
  const last = useRef<number | null>(null);
  const flying = useRef<{ el: SVGCircleElement; path: SVGPathElement; len: number; start: number }[]>([]);
  const frame = useRef(0);
  const pad = 8;
  const x = (col: number) => pad + (model.columns <= 1 ? 0 : (col * (width - 2 * pad - CARD_W)) / (model.columns - 1));
  const byId = useMemo(() => new Map(model.nodes.map((n) => [n.id, n])), [model.nodes]);
  const geo = (e: MapEdge) => {
    const a = byId.get(e.from)!;
    const b = byId.get(e.to)!;
    const x1 = x(a.col) + CARD_W;
    const x2 = x(b.col);
    const mx = (x1 + x2) / 2;
    return { d: `M${x1} ${a.y}C${mx} ${a.y} ${mx} ${b.y} ${x2} ${b.y}`, mx, my: (a.y + b.y) / 2 };
  };

  useEffect(() => () => cancelAnimationFrame(frame.current), []);
  // A bead for each call newer than the last one seen, along the line from its caller to what it named; none for the calls on
  // screen when the map opens.
  useEffect(() => {
    const newest = calls[0]?.seq ?? 0;
    if (last.current === null) {
      last.current = newest;
      return;
    }
    const after = last.current;
    last.current = Math.max(after, newest);
    const g = layer.current;
    if (!g || document.hidden) return;
    const fresh = calls.filter((c) => c.seq > after && !c.via).reverse();
    const now = performance.now();
    for (const [i, c] of fresh.entries()) {
      const t = targetOf(c);
      const path = t ? paths.current.get(`call|app|${callerOf(c)}|${t.id}||`) : undefined;
      if (!path) continue;
      if (reduced()) {
        const node = box.current?.querySelector(`[data-node="${CSS.escape(t!.id)}"]`);
        node?.classList.remove("live-pulse");
        void node?.getBoundingClientRect();
        node?.classList.add("live-pulse");
        continue;
      }
      if (flying.current.length >= 32) break;
      const el = document.createElementNS(SVG, "circle");
      el.setAttribute("r", "5");
      el.setAttribute("fill", beadColour(c));
      el.setAttribute("stroke", c.would_be_denied ? "var(--warn)" : "var(--card)");
      el.setAttribute("stroke-width", "2");
      el.dataset.bead = c.outcome;
      g.appendChild(el);
      flying.current.push({ el, path, len: path.getTotalLength(), start: now + Math.min(i, 8) * 40 });
    }
    const tick = (t: number) => {
      flying.current = flying.current.filter((b) => {
        const k = (t - b.start) / 1100;
        if (k >= 1 || !b.path.isConnected) {
          b.el.remove();
          return false;
        }
        const p = b.path.getPointAtLength((1 - (1 - Math.max(0, k)) ** 3) * b.len);
        b.el.setAttribute("cx", p.x.toFixed(1));
        b.el.setAttribute("cy", p.y.toFixed(1));
        b.el.setAttribute("opacity", k < 0 ? "0" : "1");
        return true;
      });
      frame.current = flying.current.length > 0 ? requestAnimationFrame(tick) : 0;
    };
    if (frame.current === 0 && flying.current.length > 0) frame.current = requestAnimationFrame(tick);
  }, [calls]);

  const near = (id: string) => selected === null || id === selected || model.edges.some((e) => (e.from === selected && e.to === id) || (e.to === selected && e.from === id));
  return (
    <div className={heads ? "relative pt-6" : "relative"}>
      {heads?.map((h, i) => (
        <span key={h} className="absolute top-0 text-[11.5px] font-semibold tracking-[0.08em] text-muted-foreground uppercase" style={{ left: x(i) }} data-testid="domain-head">
          {h}
        </span>
      ))}
    <div ref={box} className="relative w-full" style={{ height: model.height }} data-testid="resource-map">
      <svg width={width} height={model.height} className="absolute inset-0" role="img" aria-label={`Map: ${model.nodes.length} nodes, ${model.edges.length} lines`}>
        <g>
          {model.edges.map((e) => {
            const on = route?.edges.has(e.id) ?? false;
            const dim = route !== null ? !on : selected !== null && !(e.from === selected || e.to === selected);
            return (
              <path
                key={e.id}
                ref={(el) => {
                  if (el) paths.current.set(e.id, el);
                  else paths.current.delete(e.id);
                }}
                d={geo(e).d}
                fill="none"
                stroke={on ? "var(--primary)" : "var(--muted-foreground)"}
                strokeOpacity={on ? 0.95 : dim ? 0.12 : 0.38}
                strokeWidth={on ? 3.5 : e.kind === "wire" ? 2 : Math.min(5, 1.5 + Math.log10(Math.max(1, e.calls)) * 1.4)}
                strokeDasharray={e.kind === "wire" ? "6 5" : undefined}
                strokeLinecap="round"
                data-testid={e.kind === "wire" ? "map-wire" : "map-line"}
                data-edge={e.id}
              />
            );
          })}
        </g>
        <g ref={layer} />
      </svg>
      {model.edges
        .filter((e) => e.kind === "wire" && e.label)
        .map((e) => {
          const g = geo(e);
          return (
            <span key={e.id} className="pointer-events-none absolute z-10 flex -translate-x-1/2 -translate-y-1/2 flex-col items-center" style={{ left: g.mx, top: g.my }} data-testid="wire-label">
              <span className="rounded-full border bg-card px-2 py-px font-mono text-[11px] text-muted-foreground">{e.label}</span>
              {e.note && <span className="mt-0.5 rounded bg-card/80 px-1 font-mono text-[10.5px] text-muted-foreground">{e.note}</span>}
            </span>
          );
        })}
      {model.nodes.map((n) => (
        <Card key={n.id} n={n} x={x(n.col)} selected={selected === n.id || (route?.nodes.has(n.id) ?? false)} dim={route !== null ? !route.nodes.has(n.id) : !near(n.id)} onPick={() => onPick(selected === n.id ? null : n.id)} />
      ))}
    </div>
    </div>
  );
}

// The transit-line map: callers on the left, services on the right, a line for each who-called-what, and one bead travelling along
// its line for each call as it arrives, coloured by how it ended. SVG and a little DOM, no chart library.
//
// Cheap at 50 calls a second: the lines are React (they change when a flow is new or grows), the beads are not. A bead is one
// <circle> added to the SVG by hand and moved by one requestAnimationFrame loop that runs only while a bead is in flight; at
// most MAX_BEADS fly at once and a call over that draws none (its line still thickens). With reduced motion a bead does not
// travel: the service's station pulses once instead.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { cn } from "@doze/ui/lib/utils";
import { number } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";
import { serviceTitle } from "../../words";
import { flowKeyOf, lineWidth, type MapLayout, type Station } from "./flows";

export const MAX_BEADS = 32;
const TRAVEL_MS = 1100;

/** A bead's colour: the theme's role for the outcome. */
export function beadColour(c: Pick<CallRecord, "outcome">): string {
  switch (c.outcome) {
    case "ok":
      return "var(--primary)";
    case "error":
      return "var(--bad)";
    case "denied":
      return "var(--refused)";
    case "not-built":
      return "var(--muted-foreground)";
  }
}

const SVG = "http://www.w3.org/2000/svg";
const ease = (t: number) => 1 - (1 - t) ** 3;

type Bead = { el: SVGCircleElement; path: SVGPathElement; len: number; start: number };

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

const reducedMotion = () => typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;

/** Sends a bead along its line for each call newer than the last one seen; none for the calls on screen when the map opens. */
function useBeads(calls: readonly CallRecord[], paths: React.RefObject<Map<string, SVGPathElement>>, layer: React.RefObject<SVGGElement | null>, stationOf: (c: CallRecord) => SVGElement | null) {
  const last = useRef<number | null>(null);
  const flying = useRef<Bead[]>([]);
  const frame = useRef(0);

  useEffect(() => () => cancelAnimationFrame(frame.current), []);

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
    const fresh = calls.filter((c) => c.seq > after).reverse();
    if (reducedMotion()) {
      for (const c of fresh.slice(-MAX_BEADS)) {
        const st = stationOf(c);
        if (!st) continue;
        st.classList.remove("live-pulse");
        // Restarts the pulse (a reflow between taking the class off and putting it back).
        void st.getBoundingClientRect();
        st.classList.add("live-pulse");
      }
      return;
    }
    const now = performance.now();
    for (const [i, c] of fresh.entries()) {
      if (flying.current.length >= MAX_BEADS) break;
      const path = paths.current.get(flowKeyOf(c));
      if (!path) continue;
      const el = document.createElementNS(SVG, "circle");
      el.setAttribute("r", "5");
      el.setAttribute("fill", beadColour(c));
      el.setAttribute("stroke", "var(--card)");
      el.setAttribute("stroke-width", "2");
      if (c.would_be_denied) el.setAttribute("stroke", "var(--warn)");
      el.dataset.bead = c.outcome;
      g.appendChild(el);
      // A burst is spread a little along the line, not stacked on one point.
      flying.current.push({ el, path, len: path.getTotalLength(), start: now + Math.min(i, 8) * 40 });
    }
    const tick = (t: number) => {
      flying.current = flying.current.filter((b) => {
        const k = (t - b.start) / TRAVEL_MS;
        if (k >= 1 || !b.path.isConnected) {
          b.el.remove();
          return false;
        }
        const p = b.path.getPointAtLength(ease(Math.max(0, k)) * b.len);
        b.el.setAttribute("cx", p.x.toFixed(1));
        b.el.setAttribute("cy", p.y.toFixed(1));
        b.el.setAttribute("opacity", k < 0 ? "0" : "1");
        return true;
      });
      frame.current = flying.current.length > 0 ? requestAnimationFrame(tick) : 0;
    };
    if (frame.current === 0 && flying.current.length > 0) frame.current = requestAnimationFrame(tick);
  }, [calls, paths, layer, stationOf]);
}

function Label({ station, side, title, active, dim, onPick }: { station: Station; side: "left" | "right"; title: string; active: boolean; dim: boolean; onPick: () => void }) {
  return (
    <button
      type="button"
      onClick={onPick}
      className={cn("absolute flex max-w-[15rem] -translate-y-1/2 flex-col rounded-md px-1.5 py-0.5 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring", side === "left" ? "items-end text-right" : "items-start", dim && "opacity-40", active && "bg-accent")}
      style={side === "left" ? { right: "calc(100% - var(--x1) + 16px)", top: station.y } : { left: "calc(var(--x2) + 16px)", top: station.y }}
      aria-pressed={active}
      aria-label={`${side === "left" ? "Caller" : "Service"} ${title}: ${number(station.calls)} ${station.calls === 1 ? "call" : "calls"}. Show only its calls`}
      data-testid={side === "left" ? "map-caller" : "map-service"}
      data-station={station.id}
    >
      <span className="flex max-w-full items-center gap-1.5">
        <span className={cn("truncate text-[13px] font-medium text-bright", side === "left" && "font-mono text-[12.5px]")}>{title}</span>
        {station.internal && <span className="shrink-0 rounded-full border px-1.5 text-[10.5px] leading-4 text-muted-foreground">service</span>}
      </span>
      <span className="text-[11.5px] text-muted-foreground tabular-nums">
        {number(station.calls)} {station.calls === 1 ? "call" : "calls"}
        {station.error + station.denied > 0 && <span className="text-bad"> · {number(station.error + station.denied)} failed</span>}
      </span>
    </button>
  );
}

/** The map. `calls`: the tape, newest first (each new one sends a bead). `selected`: the flow of the open call, inked. */
export function LiveMap({ layout, calls, titles, selected, caller, service, onCaller, onService }: { layout: MapLayout; calls: readonly CallRecord[]; titles: ReadonlyMap<string, string>; selected: string | null; caller?: string; service?: string; onCaller: (id: string | undefined) => void; onService: (id: string | undefined) => void }) {
  const box = useRef<HTMLDivElement>(null);
  const width = useWidth(box);
  const paths = useRef(new Map<string, SVGPathElement>());
  const layer = useRef<SVGGElement>(null);
  const svc = useRef(new Map<string, SVGCircleElement>());
  const x1 = Math.round(Math.min(280, Math.max(180, width * 0.27)));
  const x2 = Math.max(x1 + 160, width - x1);
  const stationOf = useMemo(() => (c: CallRecord) => svc.current.get(c.service ?? "") ?? null, []);
  useBeads(calls, paths, layer, stationOf);

  const curve = (y1: number, y2: number) => {
    const mx = (x1 + x2) / 2;
    return `M${x1} ${y1}C${mx} ${y1} ${mx} ${y2} ${x2} ${y2}`;
  };
  const dimmed = (from: string, to: string) => (caller !== undefined && from !== caller) || (service !== undefined && to !== service);

  return (
    <div ref={box} className="relative w-full" style={{ height: layout.height, "--x1": `${x1}px`, "--x2": `${x2}px` } as React.CSSProperties} data-testid="live-map">
      <svg width={width} height={layout.height} className="absolute inset-0" role="img" aria-label={`Map: ${layout.callers.length} callers, ${layout.services.length} services, ${layout.lines.length} lines`}>
        <g>
          {layout.lines.map((l) => {
            const on = l.key === selected;
            const dim = dimmed(l.from.id, l.to.id);
            return (
              <path
                key={l.key}
                ref={(el) => {
                  if (el) paths.current.set(l.key, el);
                  else paths.current.delete(l.key);
                }}
                d={curve(l.from.y, l.to.y)}
                fill="none"
                stroke={on ? "var(--bright)" : "var(--muted-foreground)"}
                strokeOpacity={on ? 0.95 : dim ? 0.1 : 0.32}
                strokeWidth={on ? lineWidth(l.flow.calls) + 2 : lineWidth(l.flow.calls)}
                strokeDasharray={l.flow.internal ? "7 6" : undefined}
                strokeLinecap="round"
                data-flow={l.key}
                data-testid="map-line"
              />
            );
          })}
        </g>
        <g ref={layer} />
        {layout.callers.map((s) => (
          <rect key={s.id} x={x1 - 8} y={s.y - 8} width={16} height={16} rx={4} fill="var(--background)" stroke={s.internal ? "var(--cool)" : "var(--muted-foreground)"} strokeWidth={3} opacity={caller !== undefined && caller !== s.id ? 0.35 : 1} />
        ))}
        {layout.services.map((s) => (
          <circle
            key={s.id}
            ref={(el) => {
              if (el) svc.current.set(s.id, el);
              else svc.current.delete(s.id);
            }}
            cx={x2}
            cy={s.y}
            r={8}
            fill="var(--background)"
            stroke={s.error + s.denied > 0 ? "var(--bad)" : "var(--link)"}
            strokeWidth={3}
            opacity={service !== undefined && service !== s.id ? 0.35 : 1}
          />
        ))}
      </svg>
      {layout.callers.map((s) => (
        <Label key={s.id} station={s} side="left" title={s.internal ? (serviceTitle(s.id, titles) ?? s.id) : s.id || "No key"} active={caller === s.id} dim={caller !== undefined && caller !== s.id} onPick={() => onCaller(caller === s.id ? undefined : s.id)} />
      ))}
      {layout.services.map((s) => (
        <Label key={s.id} station={s} side="right" title={serviceTitle(s.id || null, titles) ?? "No service"} active={service === s.id} dim={service !== undefined && service !== s.id} onPick={() => onService(service === s.id ? undefined : s.id)} />
      ))}
      {(layout.hiddenCallers > 0 || layout.hiddenServices > 0) && (
        <p className="absolute bottom-1 left-0 text-[11.5px] text-muted-foreground">
          The busiest are drawn: {layout.hiddenCallers > 0 && `${number(layout.hiddenCallers)} more ${layout.hiddenCallers === 1 ? "caller" : "callers"}`}
          {layout.hiddenCallers > 0 && layout.hiddenServices > 0 && " and "}
          {layout.hiddenServices > 0 && `${number(layout.hiddenServices)} more ${layout.hiddenServices === 1 ? "service" : "services"}`} are in the wire.
        </p>
      )}
    </div>
  );
}

/** The key under the map. */
export function MapLegend() {
  const dot = (c: string) => <svg width="12" height="12" aria-hidden><circle cx="6" cy="6" r="4.5" fill={c} /></svg>;
  return (
    <div className="flex flex-wrap items-center gap-x-5 gap-y-1.5 text-[11.5px] text-muted-foreground" data-testid="map-legend">
      <span className="inline-flex items-center gap-2"><svg width="26" height="8" aria-hidden><path d="M3 4H23" stroke="var(--muted-foreground)" strokeWidth="3" strokeLinecap="round" opacity="0.6" /></svg>calls with an access key</span>
      <span className="inline-flex items-center gap-2"><svg width="26" height="8" aria-hidden><path d="M3 4H23" stroke="var(--muted-foreground)" strokeWidth="3" strokeDasharray="5 4" opacity="0.6" /></svg>calls one service made to another</span>
      <span className="inline-flex items-center gap-1.5">{dot("var(--primary)")}ok</span>
      <span className="inline-flex items-center gap-1.5">{dot("var(--bad)")}error</span>
      <span className="inline-flex items-center gap-1.5">{dot("var(--refused)")}denied</span>
      <span className="inline-flex items-center gap-1.5">{dot("var(--muted-foreground)")}not built</span>
      <span>a line is thicker the more calls it carried</span>
    </div>
  );
}

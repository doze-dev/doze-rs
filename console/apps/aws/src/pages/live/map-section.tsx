import { RadioIcon } from "lucide-react";
import { useMemo } from "react";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { number } from "@doze/ui/lib/format";
import type { CallRecord } from "../../api/types/CallRecord";
import type { Wiring } from "../../api/types/Wiring";
import { ACTIVE_MS, buildMap, domainColumns, type Layout, routeOf } from "./graph";
import { NodePanel } from "./node-panel";
import { ResourceMap } from "./resource-map";
import { callerOf, type LiveSearch } from "./search";

const dashed = "w-6 border-t-2 border-dashed border-muted-foreground/60";

/** The key under the resource map. */
function Legend() {
  return (
    <div className="mt-3 flex flex-wrap items-center gap-x-5 gap-y-1.5 text-[11.5px] text-muted-foreground" data-testid="map-legend">
      <span>caller = access key per app</span>
      <span className="inline-flex items-center gap-2"><span className="w-6 border-t-[3px] border-muted-foreground/50" />a call, thicker the more of them</span>
      <span className="inline-flex items-center gap-2"><span className={dashed} />wired: a rule&apos;s target, a dead-letter queue, a subscription, a notification</span>
      <span className="inline-flex items-center gap-1.5"><span className="inline-block size-2 rounded-full bg-primary" />a call, as it arrives</span>
      <span className="inline-flex items-center gap-1.5"><span className="inline-block size-2 rounded-full bg-warn" />would be denied</span>
      <span className="inline-flex items-center gap-1.5"><span className="inline-block size-2 rounded-full bg-refused" />refused</span>
    </div>
  );
}

/** The resource map with its controls: Focus (active in the last five minutes, or every flow), the layout, the lists of what is
 * not drawn, and the panel of the node picked. `calls`: the tape without this console's own, newest first. */
export function MapSection({ calls, wiring, now, search, set, callerName, openCall }: { calls: readonly CallRecord[]; wiring: Wiring | undefined; now: number; search: LiveSearch; set: (p: Partial<LiveSearch>) => void; callerName: (key: string) => string; openCall: CallRecord | undefined }) {
  const focus = search.focus === "all" ? "all" : "active";
  const layout: Layout = search.layout === "domain" ? "domain" : "flow";
  const model = useMemo(() => buildMap(calls, wiring, { now, focus, layout, callerName }), [calls, wiring, now, focus, layout, callerName]);
  const node = search.node ? model.nodes.find((n) => n.id === search.node) : undefined;
  const route = useMemo(() => (openCall && !openCall.via ? routeOf(model, openCall, callerOf(openCall)) : null), [model, openCall]);
  const nothing = model.nodes.length === 0;
  return (
    <section className="mb-5 rounded-xl border bg-card px-5 pt-4 pb-4" aria-label="Map of what your apps call and how it is wired" data-testid="live-map-card">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-[13px] font-semibold text-bright">
          {number(model.nodes.filter((n) => n.kind === "app").length)} apps · {number(model.nodes.filter((n) => n.kind !== "app").length)} things called
        </h2>
        <div className="flex flex-wrap items-center gap-3">
          <ToggleGroup type="single" variant="outline" size="sm" value={focus} onValueChange={(v) => v && set({ focus: v === "all" ? "all" : undefined })} aria-label="Focus" className="bg-card" data-testid="map-focus">
            <ToggleGroupItem value="active" className="px-3" data-testid="focus-active">Active in last {ACTIVE_MS / 60_000} min</ToggleGroupItem>
            <ToggleGroupItem value="all" className="px-3" data-testid="focus-all">All flows</ToggleGroupItem>
          </ToggleGroup>
          <ToggleGroup type="single" variant="outline" size="sm" value={layout} onValueChange={(v) => v && set({ layout: v === "domain" ? "domain" : undefined })} aria-label="Layout" className="bg-card" data-testid="map-layout">
            <ToggleGroupItem value="flow" className="px-3" data-testid="layout-flow">By flow</ToggleGroupItem>
            <ToggleGroupItem value="domain" className="px-3" data-testid="layout-domain">By domain</ToggleGroupItem>
          </ToggleGroup>
          <ToggleGroup type="single" variant="outline" size="sm" value="resource" onValueChange={(v) => v === "service" && set({ by: "service" })} aria-label="Draw" className="bg-card" data-testid="map-by">
            <ToggleGroupItem value="resource" className="px-3" data-testid="by-resource">Resources</ToggleGroupItem>
            <ToggleGroupItem value="service" className="px-3" data-testid="by-service">Services</ToggleGroupItem>
          </ToggleGroup>
        </div>
      </div>
      {nothing ? (
        <EmptyState icon={RadioIcon} title={model.quiet.length > 0 ? "Nothing active in the last five minutes" : "Nothing to draw yet"} className="border-0 bg-transparent">
          {model.quiet.length > 0 ? `${number(model.quiet.length)} quieter ${model.quiet.length === 1 ? "flow is" : "flows are"} kept; choose All flows to draw them.` : "Each app and each resource it calls is drawn here, with what they are wired to, as calls arrive."}
        </EmptyState>
      ) : (
        <div className={node ? "grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_21rem]" : undefined}>
          <div className="min-w-0">
            <ResourceMap heads={layout === "domain" ? domainColumns(model.nodes) : undefined} model={model} calls={calls} selected={search.node ?? null} route={route} onPick={(id) => set({ node: id ?? undefined })} />
          </div>
          {node && <NodePanel node={node} model={model} calls={calls} now={now} onPick={(id) => set({ node: id })} onClose={() => set({ node: undefined })} />}
        </div>
      )}
      {model.quiet.length > 0 && !nothing && (
        <details className="mt-3 rounded-lg border bg-muted/30 px-3 py-2 text-[13px]" data-testid="quiet-flows">
          <summary className="cursor-pointer text-muted-foreground">
            <span className="font-medium text-foreground">{number(model.quiet.length)} quieter {model.quiet.length === 1 ? "flow" : "flows"}</span> {model.quiet.slice(0, 4).map((n) => n.name).join(", ")}{model.quiet.length > 4 ? `, +${model.quiet.length - 4}` : ""}
          </summary>
          <p className="mt-2 text-muted-foreground">Called more than five minutes ago. <button type="button" className="text-link hover:underline" onClick={() => set({ focus: "all" })}>Draw all flows</button></p>
        </details>
      )}
      {model.notWired.length > 0 && (
        <details className="mt-3 rounded-lg border border-dashed px-3 py-2 text-[13px]" data-testid="not-wired">
          <summary className="cursor-pointer text-muted-foreground">
            <span className="font-medium text-foreground">Not wired · {number(model.notWired.length)} {model.notWired.length === 1 ? "resource" : "resources"}</span> nothing has called them and nothing is wired to them
          </summary>
          <ul className="mt-2 grid gap-1.5 sm:grid-cols-2 lg:grid-cols-3">
            {model.notWired.map((n) => (
              <li key={n.id} className="min-w-0 rounded-md border bg-card px-3 py-1.5" data-testid="not-wired-item">
                <span className="block truncate font-mono text-[12.5px] text-bright">{n.name}</span>
                <span className="block truncate text-[11.5px] text-muted-foreground">{n.sentence ?? n.service}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
      <Legend />
      {model.more && <p className="mt-2 text-[12px] text-muted-foreground">The map holds the first 60 resources; the rest are in Resources.</p>}
    </section>
  );
}

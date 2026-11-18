// The head of the Data page: everything on disk in one figure, what kind of thing holds it, and the volume it sits on. Every
// word that is a judgement is the server's (data_words.rs); this draws.
import { HardDriveIcon } from "lucide-react";
import type { DataKind } from "@/api/types/DataKind";
import type { DataView } from "@/api/types/DataView";
import { Card } from "@/components/ui/card";
import { bytes } from "@/lib/kafka";
import { cn } from "@/lib/utils";
import { PathValue } from "../r4/bits";

/** Each kind's colour, the same in the bar and its legend. */
export const KIND_COLOUR: Record<DataKind, string> = {
  topics: "bg-cool",
  metadata: "bg-primary",
  strays: "bg-warn",
  other: "bg-muted-foreground/45",
};

/** `12.4%`, `<0.1%`: a share in thousandths. */
export function share(permille: number): string {
  if (permille === 0) return "<0.1%";
  return `${Math.floor(permille / 10)}.${permille % 10}%`;
}

function permille(part: number, whole: number): number {
  return whole > 0 ? Math.min(1000, Math.floor((part * 1000) / whole)) : 0;
}

/** A bar of parts side by side, each its share of `whole`; a part that holds anything is at least a sliver wide. */
function Bar({ parts, whole, className, label }: { parts: { key: string; bytes: number; colour: string }[]; whole: number; className?: string; label: string }) {
  return (
    <div role="img" aria-label={label} className={cn("flex h-3 w-full gap-[2px] overflow-hidden rounded-full bg-track", className)}>
      {parts
        .filter((p) => p.bytes > 0)
        .map((p) => (
          <span key={p.key} className={cn("h-full min-w-[4px] first:rounded-l-full last:rounded-r-full motion-safe:transition-[width] motion-safe:duration-500", p.colour)} style={{ width: `${whole > 0 ? (p.bytes * 100) / whole : 0}%` }} data-part={p.key} />
        ))}
    </div>
  );
}

/** Everything on disk: the figure, the kinds, the volume. */
export function UsageHero({ v }: { v: DataView }) {
  const vol = v.volume;
  const used = vol ? vol.total - vol.free : 0;
  const others = Math.max(0, used - v.total);
  return (
    <Card className="gap-0 overflow-hidden p-0" data-testid="usage">
      <div className="grid xl:grid-cols-[minmax(0,7fr)_minmax(340px,4fr)]">
        <section aria-labelledby="on-disk" className="flex min-w-0 flex-col gap-5 p-7">
          <div className="flex flex-wrap items-end justify-between gap-x-6 gap-y-2">
            <div>
              <h2 id="on-disk" className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">
                On disk
              </h2>
              <p className="mt-1 text-[44px] leading-[48px] font-light tracking-[-0.03em] text-bright tabular-nums" data-testid="total">
                {v.cut ? "≥ " : ""}
                {bytes(v.total)}
              </p>
            </div>
            <p className="pb-1.5 text-[13px] text-muted-foreground">
              {v.topics.length === 1 ? "1 topic" : `${v.topics.length} topics`} · {v.groups === 1 ? "1 group" : `${v.groups} groups`} with commits{v.subjects !== null ? ` · ${v.subjects === 1 ? "1 schema subject" : `${v.subjects} schema subjects`}` : ""}
            </p>
          </div>
          <Bar parts={v.kinds.map((k) => ({ key: k.kind, bytes: k.bytes, colour: KIND_COLOUR[k.kind] }))} whole={v.total} label="What holds the data directory, by kind" />
          <ul className="grid gap-x-8 gap-y-3 sm:grid-cols-2" data-testid="kinds">
            {v.kinds.map((k) => (
              <li key={k.kind} className="flex min-w-0 gap-3" data-kind={k.kind}>
                <span aria-hidden className={cn("mt-1.5 size-2.5 shrink-0 rounded-[3px]", KIND_COLOUR[k.kind])} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline justify-between gap-3">
                    <span className="text-[13.5px] font-medium text-bright">{k.label}</span>
                    <span className="font-mono text-[13px] whitespace-nowrap text-foreground tabular-nums">
                      {bytes(k.bytes)} <span className="text-muted-foreground">· {share(permille(k.bytes, v.total))}</span>
                    </span>
                  </div>
                  <p className="mt-0.5 text-[12.5px] leading-[18px] text-muted-foreground">{k.note}</p>
                </div>
              </li>
            ))}
          </ul>
          <PathValue value={v.data_dir} label="Copy the data directory" className="mt-1" />
        </section>
        <section aria-labelledby="the-disk" className="flex min-w-0 flex-col gap-4 border-t bg-well/50 p-7 xl:border-t-0 xl:border-l" data-testid="volume">
          <h2 id="the-disk" className="flex items-center gap-2 text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">
            <HardDriveIcon aria-hidden className="size-3.5" /> The disk it is on
          </h2>
          {vol ? (
            <>
              <p className="text-[28px] leading-[34px] font-light tracking-[-0.02em] text-bright tabular-nums" data-testid="free">
                {bytes(vol.free)} <span className="text-[15px] font-normal tracking-normal text-muted-foreground">free of {bytes(vol.total)}</span>
              </p>
              <Bar
                parts={[
                  { key: "ours", bytes: v.total, colour: "bg-cool" },
                  { key: "others", bytes: others, colour: "bg-muted-foreground/35" },
                ]}
                whole={vol.total}
                label="The volume: doze-kafka's data, everything else, and the room left"
                className="h-2.5"
              />
              <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-[13px]">
                <dt className="flex items-center gap-2 text-muted-foreground">
                  <span aria-hidden className="size-2 rounded-[3px] bg-cool" /> doze-kafka
                </dt>
                <dd className="text-right font-mono text-foreground tabular-nums">
                  {bytes(v.total)} · {share(permille(v.total, vol.total))}
                </dd>
                <dt className="flex items-center gap-2 text-muted-foreground">
                  <span aria-hidden className="size-2 rounded-[3px] bg-muted-foreground/35" /> Everything else
                </dt>
                <dd className="text-right font-mono text-foreground tabular-nums">{bytes(others)}</dd>
                <dt className="flex items-center gap-2 text-muted-foreground">
                  <span aria-hidden className="size-2 rounded-[3px] bg-track ring-1 ring-border" /> Free
                </dt>
                <dd className="text-right font-mono text-foreground tabular-nums">{bytes(vol.free)}</dd>
              </dl>
              <p className="mt-2 text-[12.5px] leading-[18px] text-muted-foreground">When the disk fills, writes to it fail. The heads-up comes below 5% free (or 2 GiB), and once doze-kafka is a tenth of the disk.</p>
            </>
          ) : (
            <p className="text-[13.5px] text-muted-foreground">This platform does not say how big the volume is or how much room is left on it.</p>
          )}
        </section>
      </div>
    </Card>
  );
}

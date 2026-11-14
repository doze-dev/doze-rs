// The diff of two versions of a subject: the schemas laid out one field a line, side by side or unified, the lines added and
// removed, and what the registry says of the newer following the older at BACKWARD, FORWARD and FULL (nothing is stored).
import { getRouteApi } from "@tanstack/react-router";
import { CircleCheckIcon, CircleXIcon } from "lucide-react";
import { ApiFailure } from "@/api/client";
import { useDiff } from "@/api/schemas";
import type { DiffLine } from "@/api/types/DiffLine";
import { PageHeader } from "@/components/app/page-header";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { PageError } from "../page-error";

const route = getRouteApi("/schemas/subjects/$subject/diff");

const TONE: Record<string, string> = { add: "bg-good/10 text-bright", del: "bg-bad/10 text-bright", same: "text-foreground" };
const MARK: Record<string, string> = { add: "+", del: "−", same: " " };

function Unified({ lines }: { lines: DiffLine[] }) {
  return (
    <div className="overflow-auto rounded-xl border bg-well" data-testid="diff-unified">
      <table className="w-full border-collapse font-mono text-[12.5px] leading-5">
        <tbody>
          {lines.map((l, i) => (
            <tr key={i} className={TONE[l.op]} data-op={l.op}>
              <td className="w-12 px-2 text-right text-muted-foreground select-none">{l.old ?? ""}</td>
              <td className="w-12 px-2 text-right text-muted-foreground select-none">{l.new ?? ""}</td>
              <td className="w-6 text-center select-none">{MARK[l.op]}</td>
              <td className="pr-4 whitespace-pre">{l.text}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Side by side: a removed line beside the added line that replaces it. */
function pairs(lines: DiffLine[]): [DiffLine | null, DiffLine | null][] {
  const out: [DiffLine | null, DiffLine | null][] = [];
  for (let i = 0; i < lines.length; ) {
    const l = lines[i]!;
    if (l.op === "same") {
      out.push([l, l]);
      i++;
      continue;
    }
    const dels: DiffLine[] = [];
    const adds: DiffLine[] = [];
    while (i < lines.length && lines[i]!.op === "del") dels.push(lines[i++]!);
    while (i < lines.length && lines[i]!.op === "add") adds.push(lines[i++]!);
    for (let k = 0; k < Math.max(dels.length, adds.length); k++) out.push([dels[k] ?? null, adds[k] ?? null]);
  }
  return out;
}

function Split({ lines }: { lines: DiffLine[] }) {
  const cell = (l: DiffLine | null, side: "old" | "new") => (
    <>
      <td className={cn("w-10 border-r px-2 text-right text-muted-foreground select-none", l && l.op !== "same" && TONE[l.op])}>{l ? l[side] : ""}</td>
      <td className={cn("w-1/2 px-3 whitespace-pre", l ? TONE[l.op] : "bg-muted/40")}>{l?.text ?? ""}</td>
    </>
  );
  return (
    <div className="overflow-auto rounded-xl border bg-well" data-testid="diff-split">
      <table className="w-full table-fixed border-collapse font-mono text-[12.5px] leading-5">
        <tbody>
          {pairs(lines).map(([a, b], i) => (
            <tr key={i}>
              {cell(a, "old")}
              {cell(b, "new")}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function SchemaDiffPage() {
  const { subject } = route.useParams();
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const res = useDiff(subject, search.from, search.to);
  const d = res.data;
  const header = <PageHeader title={`Compare versions`} crumbs={[{ label: "Schemas", to: "/schemas" }, { label: subject, to: `/schemas/subjects/${encodeURIComponent(subject)}` }]} sub="The two schemas laid out one field a line. Nothing is checked into the registry: the verdicts below are its check, run without storing anything." />;
  if (res.isError && !d) {
    return (
      <>
        {header}
        {res.error instanceof ApiFailure ? <p className="text-[13.5px]">{res.error.message}</p> : <PageError error={res.error} onRetry={() => void res.refetch()} />}
      </>
    );
  }
  if (!d) {
    return (
      <>
        {header}
        <Skeleton className="h-96 rounded-xl" />
      </>
    );
  }
  const pick = (which: "from" | "to") => (
    <div className="flex items-center gap-2">
      <Label htmlFor={`diff-${which}`} className="text-[13px] font-normal text-muted-foreground">
        {which === "from" ? "From" : "To"}
      </Label>
      <Select value={String(d[which])} onValueChange={(v) => void navigate({ search: (s) => ({ ...s, [which]: Number(v) }), replace: true, resetScroll: false })}>
        <SelectTrigger id={`diff-${which}`} className="w-28 bg-card font-mono" data-testid={`diff-${which}`}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {d.versions.map((v) => (
            <SelectItem key={v} value={String(v)} className="font-mono">
              v{v}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
  return (
    <>
      {header}
      <div className="mb-4 flex flex-wrap items-center gap-4">
        {pick("from")}
        {pick("to")}
        <ToggleGroup type="single" variant="outline" size="sm" value={search.layout ?? "split"} onValueChange={(v) => v && void navigate({ search: (s) => ({ ...s, layout: v === "unified" ? "unified" : undefined }), replace: true, resetScroll: false })} aria-label="Layout" data-testid="diff-layout">
          <ToggleGroupItem value="split" className="px-3">
            Side by side
          </ToggleGroupItem>
          <ToggleGroupItem value="unified" className="px-3">
            Unified
          </ToggleGroupItem>
        </ToggleGroup>
        <span className="ml-auto font-mono text-[13px] text-muted-foreground tabular-nums" data-testid="diff-counts">
          v{d.from} to v{d.to} · <span className="text-good">+{d.added}</span> <span className="text-bad">−{d.removed}</span>
        </span>
      </div>
      {d.from === d.to ? <p className="mb-4 text-[13.5px] text-muted-foreground">The same version on both sides: choose another to compare.</p> : null}
      {search.layout === "unified" ? <Unified lines={d.lines} /> : <Split lines={d.lines} />}
      <section className="mt-6 rounded-xl border bg-card p-5" data-testid="diff-checks">
        <h2 className="text-[15px] font-semibold text-bright">
          v{d.to} following v{d.from}, at each level
        </h2>
        <p className="mt-1 mb-3 text-[13px] text-muted-foreground">The subject's level is {d.level}.</p>
        <ul className="flex flex-col gap-2">
          {d.checks.map((c) => (
            <li key={c.level} className="flex items-start gap-2.5 text-[13.5px]" data-testid="level-check" data-ok={c.ok}>
              {c.ok ? <CircleCheckIcon className="mt-0.5 size-4 shrink-0 text-good" aria-hidden /> : <CircleXIcon className="mt-0.5 size-4 shrink-0 text-bad" aria-hidden />}
              <span className="min-w-0">
                <span className="font-mono font-medium text-bright">{c.level}</span> {c.ok ? "passes." : <>would be refused: <span className="break-words">{c.reason}</span></>}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </>
  );
}

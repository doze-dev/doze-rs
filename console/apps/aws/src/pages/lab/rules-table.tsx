import { Link } from "@tanstack/react-router";
import { ActivityIcon, Trash2Icon } from "lucide-react";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Switch } from "@doze/ui/components/ui/switch";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@doze/ui/components/ui/table";
import { clockUtc, dateTimeUtc, number } from "@doze/ui/lib/format";
import type { LabRule } from "../../api/types/LabRule";
import { END_WORDS } from "./rule-form";

const KIND: Record<LabRule["kind"], string> = { slow: "Slow", fail: "Error", drop: "Drop" };

function Applies({ r }: { r: LabRule }) {
  return (
    <div className="min-w-0">
      <div className="truncate text-[13px] text-foreground" title={`${r.what} · ${r.target}`}>{r.what}</div>
      <div className="truncate text-[12.5px] text-muted-foreground" title={`${r.target} · ${r.share_words}`}>{r.target} · {r.share_words}</div>
    </div>
  );
}

function Name({ r, onOpen }: { r: LabRule; onOpen: (id: number) => void }) {
  return (
    <div className="min-w-0">
      <button type="button" onClick={() => onOpen(r.id)} className="max-w-full truncate rounded-sm text-left text-[13.5px] font-medium text-bright hover:text-link focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none" title={r.sentence} data-testid="rule-open">
        {r.name}
      </button>
      <div className="flex items-center gap-2 text-[12px] text-muted-foreground">
        <Badge variant="outline" className="px-1.5 py-0 text-[11px]">{KIND[r.kind]}</Badge>
        <span className="font-mono">#{r.id}</span>
      </div>
    </div>
  );
}

/** The rules in force: a switch, the rule, what it fits, its hits and when it last fired, when it ends, a link to its calls on Live, and Remove. */
export function InForceTable({ rules, onOpen, onSwitch, onRemove, busy }: { rules: LabRule[]; onOpen: (id: number) => void; onSwitch: (id: number, on: boolean) => void; onRemove: (r: LabRule) => void; busy: boolean }) {
  return (
    <div className="rounded-[var(--radius-card)] border bg-card" data-testid="rules-in-force">
      <Table aria-label="Rules in force">
        <TableHeader>
          <TableRow>
            <TableHead className="w-16 pl-5">On</TableHead>
            <TableHead>Rule</TableHead>
            <TableHead>Applies to</TableHead>
            <TableHead className="text-right">Hits</TableHead>
            <TableHead>Last hit</TableHead>
            <TableHead>Ends</TableHead>
            <TableHead className="w-24 pr-5"><span className="sr-only">Actions</span></TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rules.map((r) => (
            <TableRow key={r.id} data-testid="rule-row" data-rule={r.id} data-enabled={r.enabled}>
              <TableCell className="pl-5"><Switch checked={r.enabled} disabled={busy} onCheckedChange={(on) => onSwitch(r.id, on)} aria-label={`${r.name}: on`} data-testid="rule-switch" /></TableCell>
              <TableCell className="max-w-[16rem]"><Name r={r} onOpen={onOpen} /></TableCell>
              <TableCell className="max-w-[16rem]"><Applies r={r} /></TableCell>
              <TableCell className="text-right font-mono text-[13px]" data-testid="rule-hits">{number(r.hits)}</TableCell>
              <TableCell className="font-mono text-[12.5px]">{r.last_hit_ms ? clockUtc(r.last_hit_ms) : "None yet"}</TableCell>
              <TableCell className="font-mono text-[12.5px]" title={`${dateTimeUtc(r.ends_ms)}${r.max_hits ? `, or after ${number(r.max_hits)} hits` : ""}`}>{clockUtc(r.ends_ms)}</TableCell>
              <TableCell className="pr-5">
                <div className="flex justify-end gap-1">
                  <Button asChild variant="ghost" size="icon-sm"><Link to="/live" search={{ lab: r.id, console: "shown" }} aria-label={`See ${r.name} on Live`} title="See its calls on Live" data-testid="rule-watch"><ActivityIcon /></Link></Button>
                  <Button type="button" variant="ghost" size="icon-sm" onClick={() => onRemove(r)} disabled={busy} aria-label={`Remove ${r.name}`} title="Remove it now" data-testid="rule-remove"><Trash2Icon /></Button>
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

/** The rules that are over, newest first, each with how it ended. */
export function EndedTable({ rules, onOpen }: { rules: LabRule[]; onOpen: (id: number) => void }) {
  return (
    <div className="rounded-[var(--radius-card)] border bg-card" data-testid="rules-ended">
      <Table aria-label="Rules that ended">
        <TableHeader>
          <TableRow>
            <TableHead className="pl-5">Rule</TableHead>
            <TableHead>Applies to</TableHead>
            <TableHead className="text-right">Hits</TableHead>
            <TableHead>How it ended</TableHead>
            <TableHead className="pr-5">Ended</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rules.map((r) => (
            <TableRow key={r.id} data-testid="ended-row" data-rule={r.id}>
              <TableCell className="max-w-[16rem] pl-5"><Name r={r} onOpen={onOpen} /></TableCell>
              <TableCell className="max-w-[16rem]"><Applies r={r} /></TableCell>
              <TableCell className="text-right font-mono text-[13px]">{number(r.hits)}</TableCell>
              <TableCell className="text-[13px]" data-testid="ended-how">{r.ended ? END_WORDS[r.ended] : "Over"}</TableCell>
              <TableCell className="pr-5 font-mono text-[12.5px]" title={dateTimeUtc(r.ends_ms)}>{clockUtc(r.ends_ms)}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

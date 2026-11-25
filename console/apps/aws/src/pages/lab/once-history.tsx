import { Trash2Icon } from "lucide-react";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@doze/ui/components/ui/table";
import { clockUtc, dateTimeUtc } from "@doze/ui/lib/format";
import type { LabOnce } from "../../api/types/LabOnce";
import type { LabOnceKind } from "../../api/types/LabOnceKind";
import { AddressLink } from "./once-dialog";
import { onceLink } from "./once-link";

/** What was forced lately, newest first: when, what, in the server's sentence, whether it ran, and a link to the result. */
export function OnceHistory({ history, kinds, onClear, clearing }: { history: LabOnce[]; kinds: LabOnceKind[]; onClear: () => void; clearing: boolean }) {
  const title = (id: string) => kinds.find((k) => k.id === id)?.title ?? id;
  return (
    <div data-testid="once-history">
      <div className="rounded-[var(--radius-card)] border bg-card">
        <Table aria-label="Forced events">
          <TableHeader>
            <TableRow>
              <TableHead className="pl-5">When</TableHead>
              <TableHead>Event</TableHead>
              <TableHead>What came of it</TableHead>
              <TableHead className="pr-5"><span className="sr-only">Result</span></TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {history.map((h) => {
              const link = onceLink(h.kind, h.subject);
              return (
                <TableRow key={h.id} data-testid="once-row" data-kind={h.kind} data-ok={h.ok}>
                  <TableCell className="pl-5 font-mono text-[12.5px]" title={`${dateTimeUtc(h.at_ms)} · ${h.region}`}>{clockUtc(h.at_ms)}</TableCell>
                  <TableCell className="text-[13px]">{title(h.kind)} {!h.ok && <Badge variant="outline" className="ml-1 px-1.5 py-0 text-[11px]">nothing done</Badge>}</TableCell>
                  <TableCell className="max-w-[28rem] text-[13px] whitespace-normal text-foreground" data-testid="once-sentence">{h.sentence}</TableCell>
                  <TableCell className="pr-5 text-right text-[13px]">{link && <AddressLink address={link} testId="once-see">See</AddressLink>}</TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      </div>
      <div className="mt-3">
        <Button type="button" variant="outline" size="sm" className="bg-card" onClick={onClear} disabled={clearing} data-testid="once-clear"><Trash2Icon /> Clear the list</Button>
      </div>
    </div>
  );
}

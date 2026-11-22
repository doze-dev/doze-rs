// An alarm's History tab (cloudwatch__alarm-history): DescribeAlarmHistory by type and window.
import { useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { FilterChips } from "@doze/ui/components/app/filter-chips";
import { UtcDateTimePicker } from "@doze/ui/components/app/utc-datetime-picker";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { HistoryIcon } from "lucide-react";
import { useHistory } from "./api";
import { type HistoryItem, toMs } from "./model";

const KINDS = [["", "All"], ["StateUpdate", "StateUpdate"], ["ConfigurationUpdate", "ConfigurationUpdate"], ["Action", "Action"]] as const;

const COLUMNS: DataColumn<HistoryItem>[] = [
  { id: "at", header: "Timestamp", size: 210, fit: 190, accessorFn: (r) => toMs(r.Timestamp) ?? 0, meta: { mono: true }, cell: (c) => dateTimeUtc(toMs(c.row.original.Timestamp) ?? 0) },
  { id: "type", header: "HistoryItemType", size: 210, fit: 190, accessorFn: (r) => r.HistoryItemType, cell: (c) => <span className="rounded-full border bg-muted/60 px-2.5 py-0.5 font-mono text-[12px]">{c.row.original.HistoryItemType}</span> },
  { id: "summary", header: "HistorySummary", min: 260, enableSorting: false, accessorFn: (r) => r.HistorySummary, cell: (c) => <span className="text-[14px] break-words">{summary(c.row.original.HistorySummary)}</span> },
];

/** An ARN inside a summary is set in mono, as the board has it. */
function summary(text: string): React.ReactNode {
  return text.split(/(arn:aws:[^\s]+)/).map((p, i) => (p.startsWith("arn:aws:") ? <span key={i} className="font-mono text-[13px] break-all">{p}</span> : p));
}

export function AlarmHistory({ name }: { name: string }) {
  const [type, setType] = useState("");
  const [start, setStart] = useState<number | null>(null);
  const [end, setEnd] = useState<number | null>(null);
  const h = useHistory(name, { type, start, end });
  return (
    <div className="grid gap-5" data-testid="history-tab">
      <div className="flex flex-wrap items-center gap-3">
        <FilterChips label="Filter the history by type" value={type} onChange={setType} chips={KINDS.map(([value, label]) => ({ value, label }))} />
        <div className="ml-auto flex flex-wrap items-center gap-3">
          <UtcDateTimePicker value={start} onChange={setStart} aria-label="StartDate, UTC" placeholder="StartDate" className="w-[15rem]" />
          <UtcDateTimePicker value={end} onChange={setEnd} aria-label="EndDate, UTC" placeholder="EndDate" className="w-[15rem]" />
          {(start !== null || end !== null) && <Button type="button" variant="ghost" onClick={() => { setStart(null); setEnd(null); }} data-testid="history-window-clear">Clear window</Button>}
        </div>
      </div>
      {h.isError && !h.data ? (
        <ErrorBlock what="The history could not be read" error={h.error} onRetry={() => void h.refetch()} retrying={h.isFetching} testId="history-error" />
      ) : h.isPending ? (
        <Skeleton className="h-64 rounded-[14px]" data-testid="history-loading" />
      ) : h.items.length === 0 ? (
        <EmptyTab icon={HistoryIcon} title="No history in this window" testId="history-empty">{type || start || end ? "DescribeAlarmHistory returned nothing for this type and window." : "Nothing has happened to this alarm yet."}</EmptyTab>
      ) : (
        <DataTable aria-label="Alarm history" data={h.items} columns={COLUMNS} rowId={(r) => `${r.Timestamp}-${r.HistoryItemType}-${r.HistorySummary}`} maxHeight={640} />
      )}
      {h.more && <p className="text-[13px] text-muted-foreground" data-testid="history-more">Showing the newest 100 entries; narrow the window to see older ones.</p>}
      <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="history-explainer">
        <b className="font-semibold text-bright">Three kinds of entry:</b> configuration updates, state updates and actions. The filter above is the <span className="font-mono">HistoryItemType</span> of DescribeAlarmHistory, and the window is its <span className="font-mono">StartDate</span> and <span className="font-mono">EndDate</span>.
      </p>
    </div>
  );
}

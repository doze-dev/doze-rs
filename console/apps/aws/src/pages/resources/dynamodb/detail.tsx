// A table's page (dynamodb__table-detail and the pages of its tabs): the header with the way into its items, a sentence and what the table
// is wired to on the Overview, and five tabs, Overview, Indexes, Monitor, Exports and streams, Additional settings. What a tab's change
// leaves is said at the top from the address (`?dialog=index-created&name=`).
import { useState } from "react";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { useQueryClient } from "@tanstack/react-query";
import { dateTimeUtc, number } from "@doze/ui/lib/format";
import { useResourceDetail } from "../../../api/resources";
import type { Wired } from "../../../api/types/Wired";
import { DetailError, ResourcePage, ResourcePageSkeleton } from "../../../kit";
import { deepKey } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { headerWords, type TableInfo } from "./model";
import { Overview } from "./overview";
import { tableTarget, useListNav, useTable, useTableNav } from "./parts";
import { ExportsTab } from "./tab-exports";
import { IndexesTab } from "./tab-indexes";
import { MonitorTab } from "./tab-monitor";
import { SettingsTab } from "./tab-settings";
import { TableDelete } from "./table-delete";

const TABS = ["overview", "indexes", "monitor", "exports", "settings"] as const;
type Tab = (typeof TABS)[number];

/** What the page was left with, from the address. */
function banner(dialog: string | undefined, named: string | undefined, t: TableInfo, close: () => void): React.ReactNode {
  const say = (title: string, returned?: string) => <ResultBanner title={title} returned={returned} onDismiss={close} testId="result-banner" />;
  const index = named ? t.global.find((i) => i.name === named) : undefined;
  switch (dialog) {
    case "index-created":
      return say(`${named} was created from the table's ${number(t.items)} ${t.items === 1 ? "item" : "items"};`, `it is ${index ? index.status.charAt(0) + index.status.slice(1).toLowerCase() : "Active"} and can be queried now. On AWS it would be Creating, backfilling, for a while.`);
    case "index-deleted":
      return say(`${named} was deleted.`, `The items in ${t.name} are not touched.`);
    case "capacity-saved":
      return say("Read/write capacity updated.", "UpdateTable changed the billing mode and units of the table; they are recorded and reported by DescribeTable.");
    case "ttl-saved":
      return say(`Time to Live is on for ${t.name}.`, `DynamoDB deletes an item once the number in ${named} is in the past.`);
    default:
      return undefined;
  }
}

export function TablePage({ name, search }: OwnProps) {
  const table = name!;
  const detail = useResourceDetail("dynamodb", "tables", table);
  const read = useTable(table);
  const go = useTableNav(table);
  const toList = useListNav();
  const qc = useQueryClient();
  const [deleting, setDeleting] = useState(false);
  const t = read.table;
  const d = detail.data;
  const tab: Tab = TABS.find((x) => x === search.tab) ?? "overview";
  if ((detail.isError && !d) || (read.isError && !t)) return <DetailError service="dynamodb" kind="tables" name={table} error={detail.error ?? read.error} onRetry={() => (void detail.refetch(), void read.refetch())} retrying={detail.isFetching} />;
  if (!d || !t) return <ResourcePageSkeleton />;
  const over = tab === "overview";
  const wired: Wired[] = [...t.global.map((i) => ({ label: i.name, to: { service: "dynamodb", kind: "tables", name: i.name }, note: `The global secondary index ${i.name}` }))];
  const shown = { ...d, name: table, sentence: over ? d.sentence : null, wired: over ? wired : [], deep: [] };
  const refresh = () => {
    void detail.refetch();
    void qc.invalidateQueries({ queryKey: deepKey(tableTarget(table)) });
    void qc.invalidateQueries({ queryKey: ["resources", "tags"] });
  };
  const here = (action: string) => go({ do: action });
  const close = () => go(search.tab ? { tab: search.tab } : {});
  return (
    <>
      <ResourcePage
        detail={shown}
        details={false}
        meta={<>Created {t.createdMs ? dateTimeUtc(t.createdMs) : "—"}. {headerWords(t)}</>}
        banner={banner(search.dialog, search.name, t, close)}
        tab={tab}
        onTab={(id) => go(id === "overview" ? {} : { tab: id })}
        onVerb={(v) => v.id === "explore" && here("explore")}
        primary={over ? "explore" : undefined}
        actions={[
          { label: "Delete table", onSelect: () => setDeleting(true) },
          { label: "PartiQL editor", onSelect: () => toList({ do: "partiql" }) },
        ]}
        trail={[...(over ? [] : [{ label: ({ indexes: "Indexes", monitor: "Monitor", exports: "Exports and streams", settings: "Additional settings" } as Record<string, string>)[tab] ?? tab, to: "" }])]}
        sentenceBold={[/\d[\d,]*(?= items?\.)/]}
        wiredSelf={0}
        wiredText={(w) => `${w.to.name} index`}
        wiredHref={() => `/resources/dynamodb/tables/${encodeURIComponent(table)}?tab=indexes`}
        refresh={<RefreshControl updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} onRefresh={refresh} interval={false} />}
        tabs={[
          { id: "overview", label: "Overview", render: () => <Overview t={t} d={d} /> },
          { id: "indexes", label: "Indexes", render: () => <IndexesTab name={table} global={t.global} local={t.local} onCreate={() => here("index-create")} onDeleted={(i) => go({ tab: "indexes", dialog: "index-deleted", name: i })} /> },
          { id: "monitor", label: "Monitor", render: () => <MonitorTab name={table} indexes={t.global.map((i) => i.name)} /> },
          { id: "exports", label: "Exports and streams", render: () => <ExportsTab t={t} /> },
          { id: "settings", label: "Additional settings", render: () => <SettingsTab t={t} go={here} /> },
        ]}
      />
      {deleting && <TableDelete name={table} open onOpenChange={(o) => !o && setDeleting(false)} onDone={(items) => toList({ dialog: "deleted", name: table, ...(items !== null ? { sel: `n${items}` } : {}) })} />}
    </>
  );
}

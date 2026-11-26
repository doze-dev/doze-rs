// The list of buckets: General purpose buckets. Its rows are chosen to Empty or Delete one (each a page of its own), Create bucket
// is a page too, and what the page was left with (a bucket made, a bucket deleted) is said at the top.
import { PlusIcon, SearchIcon, Trash2Icon, DatabaseIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useResourceList } from "../../../api/resources";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { crumbs, useListNav } from "./api";
import { FlashBanner, useFlash } from "./parts";
import { longDate, regionWords } from "./words";
import { FromYourApp } from "../../../kit/from-your-app";

export function BucketsList() {
  const list = useResourceList("s3", "buckets");
  const go = useListNav();
  const [flash, dismiss] = useFlash();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const first = list.data?.pages[0];
  const rows = useMemo(() => list.data?.pages.flatMap((p) => p.rows) ?? [], [list.data]);
  const q = query.trim().toLowerCase();
  const shown = q ? rows.filter((r) => r.name.toLowerCase().includes(q)) : rows;
  const columns: DataColumn<ResourceRow>[] = [
    { id: "name", header: "Name", accessorFn: (r) => r.name, size: 280, min: 160, meta: { mono: true }, cell: (c) => <ResourceTo to={{ service: "s3", kind: "buckets", name: c.row.original.name }} className="font-mono text-[13px] font-medium" testId="row-link">{c.row.original.name}</ResourceTo> },
    { id: "region", header: "AWS Region", size: 300, min: 200, accessorFn: (r) => r.cells[0] ?? "", cell: (c) => regionWords(c.row.original.cells[0] ?? "") },
    { id: "created", header: "Creation date", size: 330, min: 240, accessorFn: (r) => r.cells[1] ?? "", cell: (c) => longDate(c.row.original.cells[1]) },
  ];
  const create = (
    <Button onClick={() => go({ do: "create" })} data-testid="create-open">
      <PlusIcon /> Create bucket
    </Button>
  );
  // Empty and Delete act on one bucket: the one chosen.
  const one = selected.size === 1 ? [...selected][0] : undefined;
  return (
    <div data-testid="buckets-list">
      <PageHeader title="General purpose buckets" sub="A bucket is a container for objects." crumbs={crumbs(null).slice(0, 3)} actions={create} refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={() => void list.refetch()} interval={false} />} />
      {flash && <div className="mb-6"><FlashBanner flash={flash} dismiss={dismiss} /></div>}
      {list.isError && !first ? (
        <ErrorBlock what="The buckets could not be listed" error={list.error} hint={HINT} onRetry={() => void list.refetch()} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading">
          <Skeleton className="h-9 w-80" />
          <Skeleton className="h-64 rounded-xl" />
        </div>
      ) : rows.length === 0 ? (
        <div data-testid="list-empty">
          <EmptyState icon={DatabaseIcon} title="No buckets" action={<Button variant="outline" className="bg-card" onClick={() => go({ do: "create" })} data-testid="empty-create">Create bucket</Button>}>
            You have no buckets in this region. Create a bucket to store objects.
          </EmptyState>
          <FromYourApp service="s3" />
        </div>
      ) : (
        <>
          {first.sentence && <p className="mb-5 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence"><Emphasis text={first.sentence} names={rows.map((r) => r.name)} bold={[/^\d[\d,]*(?= buckets?)/, /(?<=notifies (?:Lambda function|SQS queue|SNS topic) )[\w.-]+/g]} /></p>}
          <SelectableTable
            aria-label="General purpose buckets"
            data={shown}
            columns={columns}
            rowId={(r) => r.name}
            selected={selected}
            single
            onSelectedChange={setSelected}
            noun="bucket"
            maxHeight={620}
            empty={`No bucket's name contains “${query}”.`}
            toolbar={
              <>
                <div className="relative w-full max-w-[22rem]">
                  <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                  <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search buckets by name" aria-label="Search buckets by name" className="bg-card pl-9" data-testid="list-search" />
                </div>
                <Button variant="outline" className="bg-card" disabled={!one} onClick={() => one && go({ do: "empty", name: one })} data-testid="bucket-empty">
                  Empty
                </Button>
                <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={!one} onClick={() => one && go({ do: "delete", name: one })} data-testid="bucket-delete">
                  <Trash2Icon /> Delete
                </Button>
              </>
            }
          />
          <Pagination shown={shown.length} total={q ? rows.length : undefined} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun="buckets" className="mt-3" />
        </>
      )}
    </div>
  );
}

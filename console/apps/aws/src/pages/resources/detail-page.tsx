import { EyeIcon, EyeOffIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useResourceDetail, useReveal } from "../../api/resources";
import type { FieldValue } from "../../api/types/FieldValue";
import type { ResourceDetail } from "../../api/types/ResourceDetail";
import type { ResourceRow } from "../../api/types/ResourceRow";
import type { SubList } from "../../api/types/SubList";
import { DetailError } from "../../kit/detail-error";
import { RecentCalls } from "../../kit/recent-calls";
import { DeleteDialog } from "./delete-dialog";
import { columnLeast, columnSizes, isDocument, oneOf, sortKey, titleFor } from "./model";
import { CreateDialog } from "./create-dialog";
import { ResourceTo, Value } from "./value";
import { deepKey } from "./deep/api";
import { DeepPage, DeepTabs, DetailLead, GonePage, hasGonePage } from "./deep/pages";

/** The hidden value of a field: Reveal makes the one call that reads it (Live shows it); Hide forgets it. */
function Hidden({ service, kind, name, label }: { service: string; kind: string; name: string; label: string }) {
  const reveal = useReveal(service, kind, name);
  const [open, setOpen] = useState(false);
  const value = open ? reveal.data?.value : undefined;
  if (value !== undefined) {
    return (
      <div className="grid gap-2" data-testid="revealed">
        <pre className="max-h-64 overflow-auto rounded-[10px] border bg-muted px-4 py-3 font-mono text-[13px] leading-5 whitespace-pre-wrap break-all text-foreground" data-testid="revealed-value">
          {value}
        </pre>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="outline" size="sm" className="bg-card" onClick={() => setOpen(false)} data-testid="hide-value">
            <EyeOffIcon /> Hide
          </Button>
          <CopyButton text={value} what="Value copied" className="bg-card" />
          <span className="text-[12.5px] text-muted-foreground">Read with {reveal.data?.operation}, as the console.</span>
        </div>
      </div>
    );
  }
  const failed = reveal.error ? errorWords(reveal.error) : null;
  return (
    <div className="grid gap-2">
      <div className="flex flex-wrap items-center gap-3">
        <span className="font-mono tracking-[0.2em] text-muted-foreground select-none" aria-label="Hidden">
          ••••••••
        </span>
        <Button variant="outline" size="sm" className="bg-card" disabled={reveal.isPending} onClick={() => reveal.mutate(undefined, { onSuccess: () => setOpen(true) })} data-testid="reveal-value">
          <EyeIcon /> {reveal.isPending ? "Reading…" : label}
        </Button>
      </div>
      {failed && (
        <Alert tone="bad" title={`The value could not be read${failed.code ? `: ${failed.code}` : ""}`} testId="reveal-error">
          {failed.message}
        </Alert>
      )}
    </div>
  );
}

function Fields({ d }: { d: ResourceDetail }) {
  const blocks = d.fields.filter((f) => f.format === "json" && isDocument(f.value));
  const plain = d.fields.filter((f) => !(f.format === "json" && isDocument(f.value)));
  const half = Math.ceil(plain.length / 2);
  const row = (f: FieldValue) => (
    <div key={f.label} className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{f.label}</dt>
      <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={`field-${f.label}`}>
        {f.hidden ? <Hidden service={d.service} kind={d.kind} name={d.name} label={d.reveal ?? "Reveal"} /> : <Value value={f.value} format={f.format} link={f.link} />}
      </dd>
    </div>
  );
  const list = "grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] content-start [&>div:last-child>*]:border-b-0";
  return (
    <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-labelledby="details-title" data-testid="details">
      <h2 id="details-title" className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">
        Details
      </h2>
      <div className="mt-4 grid lg:grid-cols-2 lg:gap-x-12">
        <dl className={list}>{plain.slice(0, half).map(row)}</dl>
        <dl className={list}>{plain.slice(half).map(row)}</dl>
      </div>
      {blocks.map((f) => (
        <div key={f.label} className="mt-6" data-testid={`field-${f.label}`}>
          <h3 className="mb-2 text-[13.5px] text-muted-foreground">{f.label}</h3>
          <Value value={f.value} format="json" block />
        </div>
      ))}
    </section>
  );
}

function Sub({ sub, service }: { sub: SubList; service: string }) {
  const [adding, setAdding] = useState(false);
  const sizes = columnSizes(sub);
  const least = columnLeast(sub);
  const columns: DataColumn<ResourceRow>[] = sub.columns.map((col, i) => ({
    id: `c${i}`,
    header: col.title,
    size: sizes[i],
    min: least[i],
    accessorFn: (r) => sortKey(r.cells[i] ?? null, col.format),
    meta: { numeric: col.format === "number" || col.format === "bytes", mono: col.format === "mono" },
    cell: (c) => {
      const r = c.row.original;
      const v = <Value value={r.cells[i] ?? null} format={col.format} />;
      return i === 0 && r.link ? <ResourceTo to={r.link}>{v}</ResourceTo> : v;
    },
  }));
  const add = sub.add;
  return (
    <section className="mt-10" aria-label={sub.title} data-testid={`sub-${sub.title}`}>
      <div className="flex flex-wrap items-end justify-between gap-3">
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{sub.title}</h2>
        {add && (
          <Button variant="outline" size="sm" className="bg-card" onClick={() => setAdding(true)} data-testid="sub-add">
            <PlusIcon /> Add {add.one}
          </Button>
        )}
      </div>
      {add && <CreateDialog open={adding} onOpenChange={setAdding} service={service} kind={add.kind} one={add.one} operation={add.create.operations.join(", ")} inputs={add.create.inputs.map((i) => (i.key === add.key ? { ...i, value: add.value } : i))} />}
      <p className="mt-1 mb-4 text-[13.5px] text-muted-foreground">
        Read with <span className="font-mono">{sub.operation}</span>
        {sub.more ? "; the first page only, there are more." : "."}
      </p>
      {sub.error ? (
        <Alert tone="warn" title={`${sub.operation} could not be read`}>
          {sub.error}
        </Alert>
      ) : sub.rows.length === 0 ? (
        <p className="rounded-[12px] border border-dashed px-5 py-4 text-[13.5px] text-muted-foreground" data-testid="sub-none">
          None.
        </p>
      ) : (
        <DataTable aria-label={sub.title} data={sub.rows} columns={columns} rowId={(r) => `${r.name}|${r.cells.join("|")}`} maxHeight={420} empty={`None.`} />
      )}
    </section>
  );
}

export function Calls({ d }: { d: ResourceDetail }) {
  return <RecentCalls calls={d.calls} noun={d.one} className="mt-12" />;
}

/** One resource: what the descriptor's detail operations answered, the lists that belong to it, its recent calls in Live,
 * Delete (a confirm dialog), and Reveal for a hidden value. */
export function ResourceDetailPage({ service, kind, name, tab }: { service: string; kind: string; name: string; tab?: string }) {
  const detail = useResourceDetail(service, kind, name);
  const [deleting, setDeleting] = useState(false);
  const { refetch } = detail;
  const qc = useQueryClient();
  // A page of its own reads on its own: Refresh reads it again too.
  const refresh = useCallback(() => {
    void refetch();
    void qc.invalidateQueries({ queryKey: deepKey({ service, kind, name }) });
  }, [refetch, qc, service, kind, name]);
  const d = detail.data;
  const crumbs = [{ label: "Resources", to: "/resources" }, ...(d ? [{ label: d.service_title, to: `/resources/${service}` }, { label: d.kind_title, to: `/resources/${service}/${kind}` }] : [])];
  return (
    <>
      <PageHeader
        title={titleFor(name, d?.fields)}
        mono
        crumbs={crumbs}
        sub={d ? <>Read with {d.operations.map((o, i) => <span key={`${o}${i}`}>{i > 0 && ", "}<span className="font-mono text-foreground">{o}</span></span>)} as the caller <span className="font-mono text-foreground">console</span>.</> : undefined}
        refresh={<RefreshControl updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} onRefresh={refresh} interval={false} />}
        actions={
          d?.delete && (
            <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setDeleting(true)} data-testid="delete-open">
              <Trash2Icon /> {d.delete.verb}
            </Button>
          )
        }
      />
      {detail.isError && !d && hasGonePage(service, kind, errorWords(detail.error).message) ? (
        <GonePage service={service} kind={kind} name={name} />
      ) : detail.isError && !d ? (
        <DetailError service={service} kind={kind} name={name} what={`This ${oneOf(kind)} could not be read`} error={detail.error} onRetry={refresh} retrying={detail.isFetching} header={false} />
      ) : !d ? (
        <Skeleton className="h-80 rounded-[14px]" data-testid="detail-loading" />
      ) : (
        <div data-testid="detail">
          <DeepTabs d={d} name={name} tab={tab} />
          {tab ? (
            <DeepPage d={d} id={tab} />
          ) : (
            <>
              <DetailLead d={d} />
              <Fields d={d} />
              {d.subs.map((s) => (
                <Sub key={s.title} sub={s} service={service} />
              ))}
            </>
          )}
          <Calls d={d} />
          {d.delete && <DeleteDialog open={deleting} onOpenChange={setDeleting} service={service} kind={kind} one={d.one} name={d.name} label={name} spec={d.delete} />}
        </div>
      )}
    </>
  );
}

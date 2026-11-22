// A dashboard's page (cloudwatch__dashboard-view, dashboard-source, dashboard-add-widget, widget-alarm-status, widget-text): the body
// drawn on its grid, with Add widget, View/edit source and Save dashboard. Changes are made to a draft and written by Save dashboard,
// which is PutDashboard with the whole body. `?do=widget` is the page of a metric widget.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { CodeIcon, PlusIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useAbout } from "../../../api/queries";
import { useResourceDetail } from "../../../api/resources";
import { DetailError, PlainConfirm, RecentCalls, useServiceCrumbs } from "../../../kit";
import { useAlarms, useDashboard, useDashboards, useDeleteDashboards, usePutDashboard } from "./api";
import { AddWidgetDialog, AlarmWidgetDialog, SourceDialog, TextWidgetDialog, type WidgetChoice } from "./dashboard-dialogs";
import { setDraft, useDraft } from "./dashboard-draft";
import { type Body, parseBody, stringify, withoutWidget } from "./dashboard-model";
import { toMs } from "./model";
import { RangeBar, useRange } from "./range";
import { Widgets } from "./widgets";

type Search = { dialog?: string; done?: string };

export function DashboardPage({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const read = useDashboard(name, detail.isSuccess);
  const entries = useDashboards(detail.isSuccess);
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (read.isError && !read.data) return <DetailError service={service} kind={kind} name={name} error={read.error} onRetry={() => void read.refetch()} retrying={read.isFetching} />;
  if (!detail.data || !read.data) return <div className="grid gap-4" data-testid="dashboard-loading"><Skeleton className="h-12 w-80" /><Skeleton className="h-72 rounded-[14px]" /></div>;
  const saved = read.data.DashboardBody;
  const modified = toMs(entries.data?.find((e) => e.DashboardName === name)?.LastModified);
  return <View name={name} saved={saved} modified={modified} calls={detail.data.calls} fetching={detail.isFetching || read.isFetching} updatedAt={Math.max(detail.dataUpdatedAt, read.dataUpdatedAt)} refresh={() => { void detail.refetch(); void read.refetch(); void entries.refetch(); }} />;
}

function View({ name, saved, modified, calls, fetching, updatedAt, refresh }: { name: string; saved: string; modified: number | null; calls: import("../../../api/types/CallRecord").CallRecord[]; fetching: boolean; updatedAt: number; refresh: () => void }) {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as Search;
  const about = useAbout().data;
  const crumbs = useServiceCrumbs("cloudwatch");
  const put = usePutDashboard();
  const del = useDeleteDashboards();
  const alarms = useAlarms();
  const draft = useDraft(name);
  const text = draft ?? saved;
  const body = useMemo(() => parseBody(text), [text]);
  const [range, patchRange, span] = useRange("3h");
  const [asking, setAsking] = useState(false);
  const [saveFailed, setSaveFailed] = useState<unknown>(null);
  const dirty = draft !== undefined && draft !== saved;
  const dialog = (d?: string) => void navigate({ to: ".", search: ((prev: Record<string, unknown>) => { const n = { ...prev }; if (d) n.dialog = d; else delete n.dialog; return n; }) as never, replace: true });
  const change = (next: Body) => { setDraft(name, stringify(next)); setSaveFailed(null); dialog(undefined); };
  const next = (kind: WidgetChoice) => {
    if (kind === "text") dialog("text");
    else if (kind === "alarm") dialog("alarms");
    else void navigate({ to: "/resources/$service/$kind/$name", params: { service: "cloudwatch", kind: "dashboards", name }, search: { do: "widget", mode: kind } });
  };
  const save = () => put.mutate({ name, body: text }, { onSuccess: (a) => { setDraft(name, null); setSaveFailed(a.DashboardValidationMessages?.length ? a.DashboardValidationMessages : null); void navigate({ to: ".", search: { done: "saved" }, replace: true }); }, onError: setSaveFailed });
  const failed = saveFailed && !Array.isArray(saveFailed) ? errorWords(saveFailed) : null;
  const region = about?.region ?? "us-east-1";
  const account = about?.account ?? "000000000000";
  const count = body?.widgets.length ?? 0;
  const done = search.done === "created" ? { title: `Dashboard ${name} created.`, returned: "PutDashboard stored an empty dashboard. Add a widget, then Save dashboard." }
    : search.done === "saved" ? { title: `Dashboard ${name} saved.`, returned: "PutDashboard replaced the body of the dashboard with the one on this page." }
    : search.done === "widget-added" ? { title: "Widget added to the dashboard on this page.", returned: "Choose Save dashboard to store it: PutDashboard writes the whole body, with the widget." }
    : search.done === "widget" ? { title: "Widget added.", returned: "PutDashboard wrote the existing widgets back with the new one." } : null;
  return (
    <>
      <PageHeader
        title={name}
        mono
        crumbs={[...crumbs, { label: "Dashboards", to: "/resources/cloudwatch/dashboards" }, { label: name, to: `/resources/cloudwatch/dashboards/${name}` }]}
        sub={<>{dirty ? <>You have changes that are not saved. </> : null}Last saved {modified ? dateTimeUtc(modified) : "never"}.</>}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        actions={
          <>
            <Button type="button" variant="outline" className="bg-card" onClick={() => dialog("source")} data-testid="source-open"><CodeIcon /> Actions › View/edit source</Button>
            <Button type="button" variant="outline" className="bg-card" onClick={() => dialog("add-widget")} data-testid="add-widget"><PlusIcon /> Add widget</Button>
            <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setAsking(true)} data-testid="delete-open">Delete</Button>
            <Button type="button" disabled={put.isPending || !dirty || !body} onClick={save} data-testid="save-dashboard">{put.isPending ? "Saving…" : "Save dashboard"}</Button>
          </>
        }
      />
      {done && <ResultBanner className="mb-6" title={done.title} returned={done.returned} onDismiss={() => void navigate({ to: ".", search: {}, replace: true })} testId="done-banner" />}
      {failed && <Alert tone="bad" title={`The dashboard was not saved${failed.code ? `: ${failed.code}` : ""}`} testId="save-refused" className="mb-6">{failed.message}</Alert>}
      {Array.isArray(saveFailed) && <Alert tone="warn" title="CloudWatch said of the body" testId="save-messages" className="mb-6"><ul className="list-disc pl-4">{(saveFailed as { DataPath?: string; Message?: string }[]).map((m, i) => <li key={i}>{m.DataPath} {m.Message}</li>)}</ul></Alert>}
      <div className="mb-5"><RangeBar state={range} onChange={patchRange} /></div>
      {!body ? (
        <Alert tone="warn" title="The body of this dashboard is not a JSON object" testId="body-invalid">doze-aws keeps the body as written, so there is nothing to draw. Open View/edit source to replace it.</Alert>
      ) : count === 0 ? (
        <div className="rounded-[14px] border border-dashed px-6 py-14 text-center" data-testid="dashboard-empty"><p className="text-[16px] font-medium text-bright">This dashboard has no widgets</p><p className="mt-1 text-[13.5px] text-muted-foreground">Choose Add widget to put a graph, a number, alarm states or a note on it.</p></div>
      ) : (
        <Widgets widgets={body.widgets} range={span} alarms={alarms.data} onRemove={(i) => change(withoutWidget(body, i))} />
      )}
      <div className="mt-8 grid gap-3">
        <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="widget-count"><b className="font-semibold text-foreground">This dashboard has {count} {count === 1 ? "widget" : "widgets"};</b> on AWS a dashboard body holds 0 to 500 widgets laid out on a 24-column grid, each 1 to 24 columns wide (doze-aws does not check).</p>
        <p className="max-w-[46rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="dashboard-local"><b className="font-semibold text-foreground">Stored, not applied locally.</b> doze-aws keeps the dashboard body exactly as written and never interprets it; this console draws it.</p>
      </div>
      <RecentCalls calls={calls} noun="dashboard" className="mt-10" />
      {search.dialog === "source" && <SourceDialog text={text} onUpdate={change} onClose={() => dialog(undefined)} />}
      {search.dialog === "add-widget" && <AddWidgetDialog onNext={next} onClose={() => dialog(undefined)} />}
      {search.dialog === "text" && body && <TextWidgetDialog body={body} onCreate={change} onClose={() => dialog(undefined)} />}
      {search.dialog === "alarms" && body && <AlarmWidgetDialog body={body} region={region} account={account} onCreate={change} onClose={() => dialog(undefined)} />}
      <PlainConfirm
        open={asking}
        onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }}
        title="Delete dashboards?"
        description={<Emphasis text={`Delete ${name}. The dashboard and its widgets are removed.`} names={[name]} />}
        verb="Delete dashboards"
        call="DeleteDashboards"
        pending={del.isPending}
        error={del.error}
        testId="delete-dialog"
        onConfirm={() => del.mutate([name], { onSuccess: () => { setAsking(false); setDraft(name, null); void navigate({ to: "/resources/$service/$second", params: { service: "cloudwatch", second: "dashboards" }, search: { gone: name } }); } })}
      />
    </>
  );
}


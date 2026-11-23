// A function's page (lambda__function-code and the boards of its tabs): the header with Add trigger, Add destination and Delete,
// the sentence (what runs it, what triggers it, when it last ran), the Triggered by strip, and the tabs Code, Test, Monitor,
// Configuration, Aliases and Versions. Its forms are pages of their own, chosen by `?view=`; Delete and Publish are dialogs.
import { useQueryClient } from "@tanstack/react-query";
import { lazy, Suspense, useMemo, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { errorWords } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { Alert } from "@doze/ui/components/app/alert";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { TypedConfirm } from "@doze/ui/components/app/typed-confirm";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useDelete, useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, RecentCalls, ResourcePage, ResourcePageSkeleton, useBlockers, type Verb } from "../../../kit";
import { timeMs } from "../model";
import { ConfigTab } from "./config-tab";
import { CodeTab } from "./code-tab";
import { deleteWords, leftBehind, rememberLeft } from "./delete-model";
import { type Go, useFn, useFunctionSearch, useGo, useTriggers, useWhere } from "./hooks";
import { MonitorTab } from "./monitor-tab";
import { AliasesTab, VersionsTab } from "./versions-tab";
import { chipWords, madeSentence, sentenceOf, wiredOf } from "./model";
import { TestTab } from "./test-tab";

const Forms = lazy(() => import("./forms-page").then((m) => ({ default: m.FunctionForm })));

const when = (v: string): string => {
  const ms = timeMs(v);
  return ms === null ? v : dateTimeUtc(ms);
};

/** What was just done, said as the board's banner: its title and what the call returned. */
const DONE: Record<string, (name: string) => { title: string; returned: string }> = {
  created: (n) => ({ title: `${n} was created.`, returned: "Its code is the Hello world example; change it here and choose Deploy." }),
  deployed: (n) => ({ title: `The code of ${n} was deployed.`, returned: "UpdateFunctionCode replaced the package of $LATEST; published versions keep the code they had." }),
  updated: (n) => ({ title: `The configuration of ${n} was saved.`, returned: "UpdateFunctionConfiguration changed what you changed; it applies to $LATEST." }),
  env: (n) => ({ title: `The environment variables of ${n} were saved.`, returned: "UpdateFunctionConfiguration replaced the whole variables map." }),
  "trigger-added": () => ({ title: "Trigger added.", returned: "CreateEventSourceMapping made a mapping; Lambda starts polling the queue." }),
  "trigger-saved": () => ({ title: "Trigger saved.", returned: "UpdateEventSourceMapping changed the mapping." }),
  "permission-added": () => ({ title: "Permission added.", returned: "AddPermission put one Allow statement in the resource-based policy." }),
  "destination-saved": () => ({ title: "Destination saved.", returned: "The asynchronous invocation settings of the function were updated." }),
  "url-created": () => ({ title: "A function URL was created for the $LATEST unpublished version.", returned: "The function URL appears in the Function URL section and under the function's name." }),
  "alias-created": () => ({ title: "Alias created.", returned: "CreateAlias pointed the name at the version." }),
  published: () => ({ title: "Version published.", returned: "PublishVersion made an immutable copy of $LATEST." }),
  "layer-added": () => ({ title: "Layer added.", returned: "UpdateFunctionConfiguration set the function's layers." }),
};

export function FunctionDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "deep", service, kind, name] });
  };
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={refresh} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <FunctionView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

const TABS = [
  ["code", "Code"],
  ["test", "Test"],
  ["monitor", "Monitor"],
  ["configuration", "Configuration"],
  ["aliases", "Aliases"],
  ["versions", "Versions"],
] as const;

function FunctionView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useFunctionSearch();
  const go = useGo(d.name);
  const navigate = useNavigate();
  const where = useWhere();
  const f = useFn(d);
  const { triggers } = useTriggers(d);
  const [dialog, setDialog] = useState<"delete" | null>(null);
  const del = useDelete("lambda", "functions");
  const blockers = useBlockers({ service: d.service, kind: d.kind, name: d.name }, dialog === "delete");
  const lastRan = useMemo(() => {
    const ran = d.calls.find((c) => c.action === "Invoke" || c.action === "InvokeWithResponseStream");
    return ran ? new Date(ran.at_ms).toISOString().slice(11, 19) : null;
  }, [d.calls]);
  if (search.view) return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><Forms d={d} f={f} view={search.view} sel={search.sel} go={go} where={where} triggers={triggers} /></Suspense>;
  const tab = search.tab && TABS.some(([id]) => id === search.tab) ? search.tab : "code";
  const done = search.done ? DONE[search.done]?.(d.name) : undefined;
  const sentence = search.done === "created" ? madeSentence(d.name, f.roleName, triggers.length) : sentenceOf(f, triggers, lastRan);
  const wired = wiredOf(f, triggers);
  const shown: ResourceDetail = { ...d, sentence, wired };
  const clear = () => go({ tab: search.tab, p: search.p });
  const onVerb = (v: Verb) => {
    if (v.id === "trigger") go({ tab: "configuration", p: "triggers", view: "trigger-add" });
    else if (v.id === "destination") go({ tab: "configuration", p: "triggers", view: "destination-add" });
    else if (v.id === "delete") setDialog("delete");
  };
  const meta = <>Last modified {when(f.lastModified)}.{f.url ? <> Function URL: <span className="font-mono" data-testid="header-url">{f.url}</span></> : null}</>;
  const calls = <RecentCalls calls={d.calls} noun="function" className="mt-10" />;
  const tabs = TABS.map(([id, label]) => ({
    id,
    label,
    render: () =>
      id === "code" ? <CodeTab d={d} f={f} go={go} />
      : id === "test" ? <><TestTab d={d} f={f} triggers={triggers} where={where} />{calls}</>
      : id === "monitor" ? <MonitorTab d={d} />
      : id === "configuration" ? <><ConfigTab d={d} f={f} section={search.p ?? "general"} sel={search.sel} go={go} triggers={triggers} />{calls}</>
      : id === "aliases" ? <><AliasesTab d={d} go={go} />{calls}</>
      : <><VersionsTab d={d} f={f} go={go} />{calls}</>,
  }));
  const confirmDelete = () =>
    del.mutate(d.name, {
      onSuccess: () => {
        setDialog(null);
        rememberLeft(d.name, leftBehind(d, triggers));
        void navigate({ to: "/resources/$service/$second", params: { service: "lambda", second: "functions" }, search: { gone: d.name } });
      },
    });
  const failed = del.error ? errorWords(del.error) : null;
  const mapped = blockers.data?.blockers.find((b) => b.title === "Event source mappings")?.items.map((i) => i.name) ?? [];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={meta}
        banner={done ? <ResultBanner title={done.title} returned={done.returned} onDismiss={clear} testId="done-banner" /> : undefined}
        tabs={tabs}
        tab={tab}
        onTab={(id) => go({ tab: id })}
        onVerb={onVerb}
        wiredSelf={wired.filter((w) => w.label === "Triggered by").length}
        wiredText={(w) => chipWords(w, triggers)}
        wiredLead="Triggered by"
        bannerAbove
        sentenceBold={[/\b\d{2}:\d{2}:\d{2}\b/]}
        details={false}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="function-page"
      />
      <TypedConfirm
        open={dialog === "delete"}
        onOpenChange={(o) => {
          if (!o) {
            setDialog(null);
            del.reset();
          }
        }}
        title="Delete function"
        words={blockers.data?.words ?? "confirm"}
        prompt={<>To confirm, type <span className="font-mono font-semibold text-bright">{blockers.data?.words ?? "confirm"}</span></>}
        verb="Delete"
        pending={del.isPending}
        onConfirm={confirmDelete}
        testId="delete-dialog"
        description={<Emphasis text={deleteWords(d.name, blockers.data)} names={[d.name]} />}
        refusal={failed && <Alert tone="bad" title={`DeleteFunction was refused${failed.code ? `: ${failed.code}` : ""}`} testId="delete-error">{failed.message}</Alert>}
      >
        {mapped.length > 0 ? (
          <Callout tone="warn" testId="delete-mappings"><Emphasis text={mappingWords(mapped)} names={mapped} bold={[/\bnot\b/]} /></Callout>
        ) : null}
      </TypedConfirm>
    </>
  );
}

const mappingWords = (names: string[]): string => (names.length === 1 ? `The event source mapping that reads ${names[0]} is not deleted with the function. Delete it separately.` : `The event source mappings that read ${names.join(", ")} are not deleted with the function. Delete them separately.`);
export type { Go };

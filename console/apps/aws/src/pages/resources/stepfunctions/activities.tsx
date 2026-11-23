// Step Functions' activities (stepfunctions__activities-list, activities-none, activity-create): the tasks workers outside Step
// Functions poll for, the sentence of who polls and which machine uses each, the worker protocol, the last worker calls, and Create activity.
import { useMutation, useQueries, useQueryClient } from "@tanstack/react-query";
import { ListIcon, PlusIcon } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Pagination } from "@doze/ui/components/app/pagination";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Switch } from "@doze/ui/components/ui/switch";
import { client } from "../../../api/client";
import { RESOURCE_PATHS, useResourceList } from "../../../api/resources";
import type { CallRecord } from "../../../api/types/CallRecord";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import type { ResourceRow } from "../../../api/types/ResourceRow";
import { Field, FormPage, FormSection, KeyPicker, LOCAL_WORDS, RecentCalls, callKindOp, useFeeders, useServiceCrumbs } from "../../../kit";
import { HINT } from "../index-page";
import { Value } from "../value";
import type { OwnProps } from "../deep/registry";
import { parseDefinition } from "./graph";
import { fieldOf } from "./details-card";
import { useGoTo, useWorkerCalls } from "./hooks";
import { existingWords, nameProblem, allStates, taskResource } from "./model";
import { encryptionOf, encryptionProblem } from "./studio-model";

const ACTIVITY = { service: "stepfunctions", kind: "activities" } as const;
const WORKER_CALLS = ["GetActivityTask", "SendTaskSuccess", "SendTaskFailure", "SendTaskHeartbeat"];

const COLUMNS: DataColumn<ResourceRow>[] = [
  { id: "name", header: "Name", size: 240, min: 150, accessorFn: (r) => r.name.toLowerCase(), meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright" data-testid="activity-name">{c.row.original.name}</span> },
  { id: "arn", header: "ARN", min: 280, accessorFn: (r) => r.id, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px]">{c.row.original.id}</span> },
  { id: "created", header: "Creation date", size: 210, fit: 112, accessorFn: (r) => r.cells[0] ?? "", cell: (c) => <Value value={c.row.original.cells[0] ?? null} format="time" /> },
];

/** Which machines name each activity in a Task: every machine is read for its definition. */
function useUsers(machines: string[], enabled: boolean) {
  const reads = useQueries({ queries: machines.slice(0, 25).map((name) => ({ queryKey: ["resources", "one", "stepfunctions", "machines", name], queryFn: ({ signal }: { signal: AbortSignal }) => client.api<ResourceDetail>(RESOURCE_PATHS.one("stepfunctions", "machines", name), {}, signal), retry: false, enabled })) });
  return useMemo(() => {
    const used = new Map<string, string[]>();
    for (const r of reads) {
      const d = r.data;
      if (!d) continue;
      const def = parseDefinition(fieldOf(d, "Definition") ?? "");
      for (const [state, s] of allStates(def ?? undefined)) {
        const t = taskResource(s.Resource);
        if (t?.activity) used.set(t.activity, [...(used.get(t.activity) ?? []), `${d.name}\u0000${state}`]);
      }
    }
    return { used, pending: reads.some((r) => r.isPending) };
  }, [reads]);
}

export function ActivitiesList({ service, kind, search }: OwnProps) {
  const go = useGoTo();
  const list = useResourceList(service, kind);
  const machines = useResourceList("stepfunctions", "machines");
  const calls = useWorkerCalls();
  const { refetch } = list;
  const first = list.data?.pages[0];
  const all = useMemo(() => list.data?.pages.flatMap((p) => p.rows) ?? [], [list.data]);
  const mine = useMemo(() => (calls.data?.calls ?? []).filter((c) => c.service === "stepfunctions" && c.action !== null && WORKER_CALLS.includes(c.action)).reverse(), [calls.data]);
  const users = useUsers((machines.data?.pages.flatMap((p) => p.rows) ?? []).filter((r) => r.cells[0] !== "EXPRESS").map((r) => r.name), all.length > 0);
  const pollers = useFeeders(mine as CallRecord[], ["GetActivityTask"]);
  const refresh = useCallback(() => {
    void refetch();
    void calls.refetch();
  }, [refetch, calls]);
  const polled = all.find((a) => mine.some((c) => c.action === "GetActivityTask" && (c.resource ?? "").endsWith(`:activity:${a.name}`)));
  const sentence = useMemo(() => {
    if (all.length === 0) return null;
    const parts: string[] = [];
    const names: string[] = [];
    if (polled && pollers[0]) {
      const use = users.used.get(polled.name)?.[0]?.split("\u0000");
      parts.push(`${pollers[0]} polls ${polled.name}${use ? `, which ${use[0]} uses in its ${use[1]} step` : ""}.`);
      names.push(polled.name);
    }
    const idle = all.filter((a) => !users.used.has(a.name) && !users.pending).map((a) => a.name);
    if (idle.length > 0) {
      parts.push(`${idle.length === 1 ? idle[0] : `${idle.slice(0, -1).join(", ")} and ${idle[idle.length - 1]}`} ${idle.length === 1 ? "is" : "are"} not used by any machine yet.`);
      names.push(...idle);
    }
    return parts.length > 0 ? { text: parts.join(" "), names } : null;
  }, [all, polled, pollers, users]);
  const gone = search.name;
  return (
    <>
      {gone && <ResultBanner className="mb-6" title={search.mode === "existed" ? `Activity ${gone} already exists.` : `Activity ${gone} created.`} returned={search.mode === "existed" ? "CreateActivity returned the existing activity's ARN and changed nothing, so no second activity was made and its tags are as they were." : "CreateActivity returned its ARN. Use it in a Task state; workers poll for it by that ARN."} onDismiss={() => go.activities()} testId="created-banner" />}
      <PageHeader
        title="Activities"
        crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: `/resources/${service}` }, { label: "State machines", to: "/resources/stepfunctions/machines" }] : [])]}
        sub="Tasks done by workers outside Step Functions. A worker polls for work, does it, and reports back."
        refresh={<RefreshControl updatedAt={list.dataUpdatedAt} fetching={list.isFetching} onRefresh={refresh} interval={false} />}
        actions={<Button onClick={() => go.activities({ do: "create" })} data-testid="create-open"><PlusIcon /> Create activity</Button>}
      />
      {list.isError && !first ? (
        <ErrorBlock what="The activities could not be listed" error={list.error} hint={HINT} onRetry={refresh} retrying={list.isFetching} testId="list-error" />
      ) : !first ? (
        <div className="grid gap-3" data-testid="list-loading"><Skeleton className="h-9 w-80" /><Skeleton className="h-64 rounded-xl" /></div>
      ) : all.length === 0 ? (
        <div data-testid="list-empty">
          <EmptyState icon={ListIcon} title="No activities" action={<Button variant="outline" className="bg-card" onClick={() => go.activities({ do: "create" })} data-testid="create-empty"><PlusIcon /> Create activity</Button>}>
            An activity lets a worker you run yourself take part in a state machine.
          </EmptyState>
        </div>
      ) : (
        <>
          {sentence && <p className="mb-5 max-w-[62rem] text-[15px] leading-6 text-muted-foreground" data-testid="list-sentence"><Emphasis text={sentence.text} names={sentence.names} /></p>}
          <DataTable aria-label="Activities" data={all} columns={COLUMNS} rowId={(r) => r.name} maxHeight={560} />
          <Pagination className="mt-3" shown={all.length} total={all.length} more={list.hasNextPage} onMore={() => void list.fetchNextPage()} loading={list.isFetchingNextPage} noun={all.length === 1 ? "activity" : "activities"} />
          <p className="mt-5 max-w-[46rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground" data-testid="activities-note">
            A worker asks for a task with <span className="font-mono text-foreground">GetActivityTask</span>, which holds the request for up to 60 seconds, then answers with <span className="font-mono text-foreground">SendTaskSuccess</span> or <span className="font-mono text-foreground">SendTaskFailure</span>, and keeps a long task alive with <span className="font-mono text-foreground">SendTaskHeartbeat</span>. Activities are not versioned: for a change that breaks compatibility, create a new activity.
          </p>
          <div className="mt-10" data-testid="worker-calls">
            <RecentCalls calls={mine} noun="activity" scope="activities" title="Recent calls" limit={20} />
          </div>
        </>
      )}
    </>
  );
}

/** Create activity (stepfunctions__activity-create): a name, with CreateActivity drawn as it is typed. */
export function ActivityCreate() {
  const go = useGoTo();
  const qc = useQueryClient();
  const base = useServiceCrumbs("stepfunctions");
  const list = useResourceList("stepfunctions", "activities");
  const [name, setName] = useState("");
  const [tried, setTried] = useState(false);
  const taken = list.data?.pages.flatMap((p) => p.rows.map((r) => r.name)) ?? [];
  const [kms, setKms] = useState(false);
  const [kmsKey, setKmsKey] = useState<string | null>(null);
  const [kmsPeriod, setKmsPeriod] = useState("300");
  const enc = { kms, kmsKey, kmsPeriod };
  const problem = nameProblem(name, []) ?? encryptionProblem(enc);
  const body = { name, ...encryptionOf(enc) };
  const make = useMutation({
    mutationFn: (input: object) => callKindOp<{ activityArn: string }>(ACTIVITY, "CreateActivity", input),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["resources", "list", "stepfunctions", "activities"] }),
  });
  const failed = make.error ? errorWords(make.error) : null;
  return (
    <FormPage
      title="Create activity"
      sub="Give the activity a name, then use its ARN in a Task state. Workers poll for it by that ARN."
      crumbs={[...base, { label: "State machines", to: "/resources/stepfunctions/machines" }, { label: "Activities", to: "/resources/stepfunctions/activities" }]}
      request={{ service: "stepfunctions", operation: "CreateActivity", input: body }}
      callNote={<>The response carries the activity ARN, <span className="font-mono">arn:aws:states:…:activity:{name || "name"}</span>.</>}
      refusal={failed ? { code: failed.code ?? "Error", message: failed.message } : null}
      banner={failed ? <Alert tone="bad" title={`CreateActivity was refused${failed.code ? `: ${failed.code}` : ""}`} testId="activity-refused">{failed.message}</Alert> : undefined}
      submitLabel="Create activity"
      pending={make.isPending}
      disabled={tried && Boolean(problem)}
      disabledWhy={problem ?? "Fix the name first."}
      onCancel={() => go.activities()}
      onSubmit={() => { setTried(true); if (!problem) make.mutate(body, { onSuccess: () => go.activities(taken.includes(name) ? { name, mode: "existed" } : { name }) }); }}
      testId="activity-create-page"
    >
      <FormSection title="Details">
        <div className="grid gap-6">
          <Field label="Name" error={tried && nameProblem(name, []) ? nameProblem(name, []) ?? undefined : undefined} help={<>1 to 80 characters, with the same rules as a state machine name: no whitespace, wildcards, brackets or special characters. Unique in this account and Region. {taken.length > 0 && <span className="text-muted-foreground">{existingWords(taken.slice(0, 4)).replace(" in this Region.", "")}; creating a name that exists returns the existing activity and leaves its tags alone.</span>}</>}>
            {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="activity-name-input" />}
          </Field>
          <div className="grid gap-3" data-testid="activity-encryption">
            <span className="text-sm leading-none font-medium">Encryption</span>
            <div className="flex items-center gap-3 text-[13.5px]">
              <Switch checked={kms} onCheckedChange={setKms} aria-label="Customer managed KMS key encryption" data-testid="activity-kms" />
              <span>Customer managed KMS key <span className="font-mono text-[12.5px] text-muted-foreground">encryptionConfiguration</span></span>
            </div>
            <p className="border-l-2 pl-3 text-[13px] text-muted-foreground">doze-aws keeps the setting and returns it, and does not encrypt an activity's task input with it. <span className="text-muted-foreground/80">{LOCAL_WORDS.stored}.</span></p>
            {kms && (
              <div className="grid gap-5 sm:grid-cols-2">
                <Field label="KMS key">{(id) => <KeyPicker id={id} value={kmsKey} onChange={setKmsKey} by="id" />}</Field>
                <Field label="Data key reuse period" hint="Seconds, 60 to 900.">{(id) => <Input id={id} inputMode="numeric" value={kmsPeriod} onChange={(e) => setKmsPeriod(e.target.value)} className="bg-card" data-testid="activity-kms-period" />}</Field>
              </div>
            )}
          </div>
        </div>
      </FormSection>
    </FormPage>
  );
}

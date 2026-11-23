// A rule's page (eventbridge__rule-detail, rule-targets-none, rule-delete from the page): what feeds it, what it sends to, its pattern and its
// targets with their input and retry policy, and the calls that touched it and what it delivered. Add target is `?view=target`.
import { useNavigate, useSearch } from "@tanstack/react-router";
import { PlusIcon, TrashIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Badge } from "@doze/ui/components/ui/badge";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Button } from "@doze/ui/components/ui/button";
import { Link } from "@tanstack/react-router";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { DetailError, type FlowGroup, FlowStrip, PlainConfirm, RecentCalls, ResourcePage, ResourcePageSkeleton, RuleHelp, type Verb, callKindOp, useFeeders, useTags } from "../../../kit";
import { RULES, useTargets } from "./api";
import { useFanout } from "./fanout";
import { Breakable, DetailsGrid, Heading, useWhere } from "./parts";
import { resultNote, ruleCalls, useRecord } from "./record";
import { RuleDelete } from "./rule-delete";
import { TargetAdd } from "./target-add";
import type { TargetItem } from "./types";
import { arnName, inputWords, prettyPattern, retryWords, targetParts } from "./words";

export function RuleDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const search = useSearch({ strict: false }) as { view?: string };
  const navigate = useNavigate();
  const bus = name.includes("/") ? name.split("/")[0]! : "default";
  const rule = name.includes("/") ? name.split("/").slice(1).join("/") : name;
  const targets = useTargets(bus, rule);
  if (detail.isError && !detail.data) return <DetailError service={service} kind={kind} name={name} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  if (search.view === "target") return <TargetAdd bus={bus} rule={rule} taken={(targets.data?.Targets ?? []).map((t) => t.Id)} back={() => void navigate({ to: "/resources/$service/$kind/$name", params: { service, kind, name }, search: {} })} />;
  return <RuleView d={detail.data} bus={bus} rule={rule} targets={targets.data?.Targets} reload={() => { void detail.refetch(); void targets.refetch(); }} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} />;
}

const field = (d: ResourceDetail, label: string) => d.fields.find((f) => f.label === label)?.value ?? null;

function RuleView({ d, bus, rule, targets, reload, updatedAt, fetching }: { d: ResourceDetail; bus: string; rule: string; targets: TargetItem[] | undefined; reload: () => void; updatedAt: number; fetching: boolean }) {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { done?: string };
  const { region, account } = useWhere();
  void region; void account;
  const [deleting, setDeleting] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [removing, setRemoving] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const tags = useTags({ service: d.service, kind: d.kind, name: d.name });
  const record = useRecord();
  const busCalls = useFeeders(record.data?.calls.filter((c) => c.resource === bus && c.action === "PutEvents") ?? [], ["PutEvents"]);
  const fanout = useFanout(targets);
  const calls = useMemo(() => ruleCalls(d.calls, record.data?.calls ?? [], rule), [d.calls, record.data, rule]);
  const note = useMemo(() => (c: Parameters<typeof resultNote>[0]) => (c.action === "PutEvents" ? "matched this rule" : resultNote(c, record.data?.calls ?? [], (targets ?? []).find((t) => c.resource !== null && t.Arn.split(":").pop() === c.resource.split(/[/:]/).pop()))), [record.data, targets]);
  const state = field(d, "State");
  const schedule = field(d, "Schedule");
  const pattern = field(d, "Event pattern");
  const go = (search: Record<string, string>) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: search as never });
  const toggle = async () => {
    setError(null);
    try {
      await callKindOp(RULES, state === "ENABLED" ? "DisableRule" : "EnableRule", { Name: rule, EventBusName: bus });
      reload();
      go({ done: state === "ENABLED" ? "disabled" : "enabled" });
    } catch (e) {
      setError((e as Error).message);
    }
  };
  const onVerb = (v: Verb) => {
    if (v.id === "delete") setDeleting(true);
    else if (v.id === "disable" || v.id === "enable") void toggle();
  };
  const refresh = () => { reload(); void record.refetch(); void tags.refetch(); };
  const groups: FlowGroup[] = [
    ...(busCalls.length > 0 ? [{ label: "Fed by", chips: busCalls.map((l) => ({ label: l })) }] : []),
    { chips: [{ label: `${bus} bus`, to: { service: "eventbridge", kind: "buses", name: bus } }] },
    { chips: [{ label: rule, self: true }] },
    ...((targets ?? []).length > 0 ? [{ chips: (targets ?? []).map((t) => { const p = targetParts(t.Arn); return { label: `${p.name} ${p.kind.split(" ").pop()}`, to: link(p.kind, p.name) }; }) }] : []),
    ...(fanout.length > 0 ? [{ chips: fanout.map((label) => ({ label })) }] : []),
  ];
  const columns: DataColumn<TargetItem>[] = [
    { id: "id", header: "Target ID", size: 210, min: 150, accessorFn: (t) => t.Id, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium">{c.row.original.Id}</span> },
    { id: "arn", header: "Target ARN", min: 220, accessorFn: (t) => t.Arn, cell: (c) => <span className="block font-mono text-[12.5px] leading-4 [overflow-wrap:anywhere] whitespace-normal" data-testid="target-arn"><Breakable text={c.row.original.Arn} /></span> },
    { id: "input", header: "Input", size: 250, accessorFn: (t) => inputWords(t), cell: (c) => <span className="block text-[13px] leading-4 whitespace-normal">{inputWords(c.row.original)}</span> },
    { id: "retry", header: "Retry policy", size: 210, accessorFn: (t) => retryWords(t), cell: (c) => <span className="text-[13px]">{retryWords(c.row.original)}</span> },
    { id: "dlq", header: "Dead-letter queue", size: 170, accessorFn: (t) => t.DeadLetterConfig?.Arn ?? "", cell: (c) => (c.row.original.DeadLetterConfig?.Arn ? <span className="font-mono text-[12.5px]">{arnName(c.row.original.DeadLetterConfig.Arn)}</span> : <span className="text-muted-foreground">–</span>) },
  ];
  const removeTargets = async () => {
    setRemoving(true);
    setError(null);
    try {
      await callKindOp(RULES, "RemoveTargets", { Rule: rule, EventBusName: bus, Ids: [...picked] });
      setPicked(new Set());
      reload();
      setConfirming(false);
      go({ done: "removed" });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRemoving(false);
    }
  };
  const banner = search.done === "created" ? <ResultBanner title={`Rule ${rule} created.`} returned={<>PutRule created it {state === "ENABLED" ? "enabled" : "disabled"} and PutTargets added its {(targets ?? []).length === 1 ? "target" : `${(targets ?? []).length} targets`}.</>} onDismiss={() => go({})} testId="created-banner" />
    : search.done === "target" ? <ResultBanner title="Target added." returned="PutTargets added it to the rule; the next matching event is delivered to it." onDismiss={() => go({})} testId="target-banner" />
    : search.done === "removed" ? <ResultBanner title="Targets removed." returned="RemoveTargets took them off the rule. The resources they sent to are untouched." onDismiss={() => go({})} testId="removed-banner" />
    : search.done === "enabled" || search.done === "disabled" ? <ResultBanner title={`Rule ${rule} ${search.done}.`} returned={search.done === "enabled" ? "EnableRule: it matches events again." : "DisableRule: it matches no events until it is enabled."} onDismiss={() => go({})} testId="state-banner" /> : undefined;
  const none = targets !== undefined && targets.length === 0;
  return (
    <>
      <ResourcePage
        detail={{ ...d, name: rule, wired: [] }}
        meta={<>A rule on the <Link to="/resources/$service/$kind/$name" params={{ service: "eventbridge", kind: "buses", name: bus }} className="text-link hover:underline">{bus}</Link> event bus.</>}
        banner={banner}
        actions={[{ label: "Add target", onSelect: () => go({ view: "target" }) }]}
        onVerb={onVerb}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        overview={
          <>
            <FlowStrip groups={groups} className="mb-6" />
            <div className="mb-10">
              <Heading title="Rule details" />
              <DetailsGrid columns={2} testId="rule-details" rows={[["Name", <span key="n" className="font-mono">{rule}</span>], ["State", state === "ENABLED" ? "Enabled" : "Disabled"], ["Rule type", schedule ? "Scheduled rule (legacy)" : "Rule with an event pattern"], ["Event bus", <Link key="b" to="/resources/$service/$kind/$name" params={{ service: "eventbridge", kind: "buses", name: bus }} className="font-mono text-link hover:underline">{bus}</Link>], ["Description", field(d, "Description") ?? <span className="text-muted-foreground">–</span>], ["ARN", <span key="a" className="font-mono">{field(d, "ARN") ?? ""}</span>], ...(schedule ? [["Schedule", <span key="s" className="font-mono">{schedule}</span>] as const] : []), ["Tags", tags.data?.tags.length ? <span key="t" className="flex flex-wrap gap-1.5">{tags.data.tags.map((t) => <Badge key={t.key} variant="outline" className="font-mono text-[11.5px] font-normal">{t.value ? `${t.key}=${t.value}` : t.key}</Badge>)}</span> : <span className="text-muted-foreground">None</span>]]} />
            </div>
            {pattern && (
              <section className="mb-10" aria-labelledby="pattern-title" data-testid="rule-pattern">
                <div className="mb-3 flex items-center justify-between"><Heading title="Event pattern" id="pattern-title">An event goes to the targets when it matches all of this.</Heading><CopyButton text={pattern} label="Copy" what="Pattern copied" className="bg-card" /></div>
                <pre className="overflow-auto rounded-[14px] border bg-card px-5 py-4 font-mono text-[13px] leading-5 text-bright" data-testid="pattern-box">{prettyPattern(pattern)}</pre>
                <RuleHelp className="mt-3 max-w-3xl">Every field's value is a list, and one entry of the list matching is enough. Fields you leave out are not checked. Numeric conditions compare values as 64-bit floating-point numbers.</RuleHelp>
              </section>
            )}
            <section className="mb-12" aria-labelledby="targets-title" data-testid="rule-targets">
              <div className="flex items-start justify-between gap-3"><Heading title="Targets" id="targets-title">Where matching events are sent. A rule can have up to 5 targets, and they run in parallel.</Heading>{!none && <Button variant="outline" className="bg-card" onClick={() => go({ view: "target" })} data-testid="add-target"><PlusIcon /> Add target</Button>}</div>
              {none ? (
                <EmptyTab title="No targets" action={<Button onClick={() => go({ view: "target" })} data-testid="add-target"><PlusIcon /> Add target</Button>}><b className="font-mono font-semibold text-bright">{rule}</b> matches events but has nowhere to send them. A rule needs at least one target to do anything.</EmptyTab>
              ) : (
                <SelectableTable<TargetItem> aria-label="Targets" data={targets ?? []} columns={columns} rowId={(t) => t.Id} selected={picked} onSelectedChange={setPicked} noun="target" rowHeight={60} actionsAlways actionsEnd hint=" "
                  actions={(sel) => <Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" disabled={sel.length === 0 || removing} onClick={() => { setError(null); setConfirming(true); }} data-testid="remove-targets"><TrashIcon /> Remove</Button>} />
              )}
              {error && !confirming && <p className="mt-3 text-[13px] text-bad" role="alert">{error}</p>}
              <PlainConfirm open={confirming} onOpenChange={(o) => { if (!removing) setConfirming(o); }} title={`Remove ${picked.size === 1 ? "this target" : `${picked.size} targets`}?`} verb="Remove" call="RemoveTargets" pending={removing} error={error ? new Error(error) : undefined} onConfirm={() => void removeTargets()} testId="remove-targets-dialog"
                description={<>RemoveTargets takes <b className="font-mono font-semibold text-bright">{[...picked].join(", ")}</b> off <b className="font-mono font-semibold text-bright">{rule}</b>, so matching events stop going there. The {picked.size === 1 ? "resource it sends" : "resources they send"} to {picked.size === 1 ? "is" : "are"} not touched.{(targets ?? []).length === picked.size ? " This is the last target: the rule keeps matching events but has nowhere to send them." : ""}</>} />
              {!none && <RuleHelp className="mt-4 max-w-3xl">On AWS the retry policy defaults to <b>24 hours</b> and <b>185</b> attempts. <b>Applied here:</b> a failed delivery is retried with a back-off and, when it runs out of tries or time, goes to the target's dead-letter queue if it has one, else is dropped.</RuleHelp>}
            </section>
            <RecentCalls calls={calls} noun="rule" scope={`${rule} and what it delivered`} note={note} />
          </>
        }
        testId="rule-page"
      />
      <RuleDelete open={deleting} onOpenChange={setDeleting} bus={bus} rules={[{ name: rule, targets: targets ?? [] }]} onDone={() => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "rules" }, search: { name: bus, gone: rule } })} />
    </>
  );
}

/** Where a target chip goes, when it is a resource the console has a page for. */
function link(kind: string, name: string): { service: string; kind: string; name: string } | undefined {
  const to: Record<string, [string, string]> = { "SQS queue": ["sqs", "queues"], "SNS topic": ["sns", "topics"], "Lambda function": ["lambda", "functions"], "Log group": ["logs", "groups"], "Kinesis stream": ["kinesis", "streams"], "State machine": ["stepfunctions", "machines"], "API destination": ["eventbridge", "api-destinations"], "Event bus": ["eventbridge", "buses"] };
  const at = to[kind];
  return at ? { service: at[0], kind: at[1], name } : undefined;
}

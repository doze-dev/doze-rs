// A subscription's page (sns__subscription-detail): its id and status in the header, what it delivers and when, the strip from topic through its
// filter to the endpoint, the Details card, the filter policy, the redrive policy and the calls that touched it. Edit is a page of its own (`?view=edit`).
import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { ArrowRightIcon } from "lucide-react";
import { lazy, Suspense, useMemo, useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { RefreshControl } from "@doze/ui/components/app/refresh-control";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useDelete, useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { resourceHref, ResourcePage, ResourcePageSkeleton, type Verb } from "../../../kit";
import { HINT } from "../index-page";
import { ResourceTo } from "../value";
import { inlineJson, policyWords } from "./filter-match";
import { Block, Rich, TopicCalls } from "./parts";
import { protocolLabel } from "./protocols";
import { b, type Part } from "./sentences";
import { redriveTarget, subscriptionOf, type Subscription } from "./sub-form-model";
import { endpointName, isPending, shortId, tail } from "./sub-model";
import { Status } from "./subs-table";
import { ConfirmSubscription } from "./confirm-dialog";
import { DeleteSubscription } from "./sub-dialogs";

const Edit = lazy(() => import("./sub-edit").then((m) => ({ default: m.SubEdit })));

export function SubDetail({ service, kind, name }: { service: string; kind: string; name: string }) {
  const detail = useResourceDetail(service, kind, name);
  const { refetch } = detail;
  const qc = useQueryClient();
  const refresh = () => {
    void refetch();
    void qc.invalidateQueries({ queryKey: ["resources", "one", "sns", "topics"] });
  };
  if (detail.isError && !detail.data) return <ErrorBlock what="This subscription could not be read" error={detail.error} hint={HINT} onRetry={refresh} retrying={detail.isFetching} testId="detail-error" />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <SubView d={detail.data} updatedAt={detail.dataUpdatedAt} fetching={detail.isFetching} refresh={refresh} />;
}

/** `Messages published to orders-events.fifo reach the queue orders-events.fifo only when their event attribute is order_placed or order_cancelled.` */
export function subSentence(s: Subscription): Part[] {
  const topic = tail(s.topicArn);
  const to = endpointName(s.protocol, s.endpoint);
  const what = s.protocol === "sqs" ? ["the queue ", b(to)] : s.protocol === "lambda" ? ["the function ", b(to)] : s.protocol === "http" || s.protocol === "https" ? ["the webhook ", b(to)] : [b(to)];
  if (s.pending) return ["This subscription of ", b(topic), " to ", ...what, " is waiting to be confirmed, so nothing is delivered to it yet."];
  const out: Part[] = ["Messages published to ", b(topic), " reach ", ...what];
  if (s.filter) {
    const simple = plainEntries(s.filter);
    if (simple && simple.length === 1 && s.scope === "MessageAttributes") {
      const [key, values] = simple[0]!;
      const vals = values as (string | number)[];
      out.push(" only when their ", b(key), " attribute is ");
      vals.forEach((v, i) => out.push(...(i === 0 ? [b(String(v))] : [i === vals.length - 1 ? " or " : ", ", b(String(v))])));
    } else out.push(s.scope === "MessageBody" ? " only when their body matches its filter policy" : " only when their attributes match its filter policy");
  } else out.push(", every one of them");
  out.push(`. Raw message delivery is ${s.raw ? "on, so the endpoint gets the bare message body" : "off"}.`);
  return out;
}

function SubView({ d, updatedAt, fetching, refresh }: { d: ResourceDetail; updatedAt: number; fetching: boolean; refresh: () => void }) {
  const search = useSearch({ strict: false }) as { view?: string; done?: string };
  const navigate = useNavigate();
  const s = useMemo(() => subscriptionOf(d), [d]);
  const del = useDelete("sns", "subscriptions");
  const [asking, setAsking] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [mine, setMine] = useState(false);
  const go = (view?: string, done?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: d.service, kind: d.kind, name: d.name }, search: { ...(view ? { view } : {}), ...(done ? { done } : {}) } });
  const pending = isPending(d.name) || s.pending;
  if (search.view === "edit" && s.arn) return <Suspense fallback={<Skeleton className="h-96 rounded-[14px]" />}><Edit d={d} s={s} back={() => go()} done={(done) => go(undefined, done)} /></Suspense>;
  const id = s.arn ? tail(s.arn) : null;
  const title = shortId(id);
  const topic = tail(s.topicArn);
  const target = redriveTarget(s.redrive);
  const banner =
    search.done === "created" ? <ResultBanner title="Subscription created." returned={pending ? "Subscribe returned pending confirmation: nothing is delivered until the endpoint confirms." : "Subscribe returned its ARN, and the subscription's Details page opens. It is confirmed at once."} onDismiss={() => go()} testId="created-banner" />
    : search.done === "updated" ? <ResultBanner title="Subscription updated." returned="SetSubscriptionAttributes saved each setting that changed, one call each." onDismiss={() => go()} testId="updated-banner" />
    : undefined;
  const onVerb = (v: Verb) => {
    if (v.id === "edit") go("edit");
    else if (v.id === "confirm") setConfirming(true);
    else if (v.id === "delete") setAsking(true);
  };
  const shown: ResourceDetail = { ...d, name: title, sentence: null, wired: [], badges: [{ text: pending ? "Pending confirmation" : "Confirmed", tone: pending ? "warn" : "good" }, { text: protocolLabel(s.protocol), tone: "neutral" }] };
  const mail = s.protocol === "email" || s.protocol === "email-json";
  const rows: [string, React.ReactNode][] = [
    ["Subscription ARN", !s.arn ? <span className="text-muted-foreground">None yet: SNS gives a subscription its ARN when it is confirmed</span> : <span className="flex items-center gap-2 font-mono" data-testid="field-arn">{s.arn}<CopyButton text={s.arn ?? ""} what="ARN copied" iconOnly variant="ghost" label="Copy ARN" /></span>],
    ["Endpoint", <span key="e" className="font-mono">{s.protocol === "sqs" ? <ResourceTo to={{ service: "sqs", kind: "queues", name: endpointName("sqs", s.endpoint) }}>{s.endpoint}</ResourceTo> : s.protocol === "lambda" ? <ResourceTo to={{ service: "lambda", kind: "functions", name: endpointName("lambda", s.endpoint) }}>{s.endpoint}</ResourceTo> : s.endpoint}{mail && pending ? <span className="ml-3 font-sans text-[12.5px] text-muted-foreground">Stored: doze-aws sends no email</span> : null}</span>],
    ["Topic ARN", <span key="t" className="font-mono"><Link to={resourceHref({ service: "sns", kind: "topics", name: topic }) as never} className="text-link hover:underline">{s.topicArn}</Link></span>],
    ["Protocol", protocolLabel(s.protocol)],
    ["Status", <Status key="s" s={{ pending }} />],
    ["Owner", <span key="o" className="font-mono">{s.owner ?? "–"}</span>],
    ["Raw message delivery", s.raw ? "Enabled" : "Disabled"],
  ];
  return (
    <>
      <ResourcePage
        detail={shown}
        meta={<span className="font-mono">{id ?? s.endpoint}</span>}
        banner={banner}
        overview={
          <>
            <Rich parts={subSentence(s)} className="mb-6" testId="detail-sentence" />
            <nav aria-label="Wired to" className="mb-8 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-[14px] border bg-card px-5 py-4" data-testid="wired-strip">
              <Link to={resourceHref({ service: "sns", kind: "topics", name: topic }) as never} className="inline-flex items-center rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright hover:bg-accent" data-testid="wired-link">{topic}</Link>
              <ArrowRightIcon aria-hidden className="size-3.5 text-input" />
              {s.filter && <><span className="text-[13.5px] text-muted-foreground" data-testid="wired-filter">filter: {policyWords(s.filter)}</span><ArrowRightIcon aria-hidden className="size-3.5 text-input" /></>}
              <span className="inline-flex items-center rounded-lg border bg-well px-3 py-1.5 font-mono text-[13px] text-bright">{endpointName(s.protocol, s.endpoint)} ({s.protocol === "sqs" ? "SQS" : s.protocol === "lambda" ? "Lambda" : protocolLabel(s.protocol)})</span>
            </nav>
            <section className="mb-10 rounded-[14px] border bg-card px-6 py-2 lg:px-7" aria-label="Details" data-testid="details">
              <dl className="grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
                {rows.map(([l, v]) => (<div key={l} className="contents"><dt className="border-b py-3.5 pr-4 text-[13.5px] text-muted-foreground">{l}</dt><dd className="min-w-0 border-b py-3.5 text-[13.5px] break-words text-bright" data-testid={`field-${l}`}>{v}</dd></div>))}
              </dl>
            </section>
            <div className="grid gap-10">
              <Block title="Subscription filter policy" testId="filter-section" lede={s.filter ? <>Scope: <b className="font-semibold text-foreground">{s.scope === "MessageBody" ? "payload-based filtering" : "attribute-based filtering"}</b> (FilterPolicyScope = {s.scope}). Only messages whose {s.scope === "MessageBody" ? "body" : "attributes"} satisfy the policy are delivered.</> : "No filter policy: every message published to the topic is delivered."}>
                {s.filter && (<><CodeBlock text={inlineJson(s.filter)} max="max-h-72" testId="filter-json" /><Callout tone="note" lead="Only String, String.Array and Number attributes are matched." testId="note">Binary attributes are ignored, and string matching is case-sensitive. To match the message body instead, set the filter policy scope to MessageBody (payload-based filtering).</Callout></>)}
              </Block>
              <Block title="Redrive policy (dead-letter queue)" testId="redrive-section" lede={target ? <>Messages SNS cannot deliver are sent to <ResourceTo to={{ service: "sqs", kind: "queues", name: tail(target) }} className="font-mono">{tail(target)}</ResourceTo> (<span className="font-mono">deadLetterTargetArn</span> {target}). Applied by doze-aws.</> : "Not set. Messages that cannot be delivered are not held in a dead-letter queue."}>{null}</Block>
            </div>
          </>
        }
        verbStates={pending ? { ...(s.arn ? {} : { edit: { disabled: "A subscription waiting for its endpoint has no ARN to change yet." } }), delete: { disabled: "You can't delete a subscription that's pending confirmation. It is confirmed from its topic, or SNS deletes it after 48 hours on AWS." } } : undefined}
        onVerb={onVerb}
        refresh={<RefreshControl updatedAt={updatedAt} fetching={fetching} onRefresh={refresh} interval={false} />}
        testId="sub-page"
      />
      <div className="mt-12"><TopicCalls d={d} mine={mine} onMine={setMine} what="this subscription" /></div>
      <DeleteSubscription open={asking} onOpenChange={(o) => { setAsking(o); if (!o) del.reset(); }} protocol={s.protocol} endpoint={s.endpoint} topic={topic} pending={del.isPending} error={del.error}
        onConfirm={() => del.mutate(d.name, { onSuccess: () => { setAsking(false); void navigate({ to: "/resources/$service/$second", params: { service: "sns", second: "subscriptions" }, search: { gone: title } }); } })} />
      <ConfirmSubscription sub={confirming ? { name: d.name, pending: true, topicArn: s.topicArn, topic, protocol: s.protocol, endpoint: s.endpoint, id: null } : null} topic={topic} onClose={() => setConfirming(false)} onDone={() => { setConfirming(false); void navigate({ to: "/resources/$service/$kind/$name", params: { service: "sns", kind: "topics", name: topic }, search: { done: "confirmed", tab: "subscriptions" } }); }} />
    </>
  );
}

/** The keys of a filter policy that holds only lists of plain values; `null` when it holds more (or is not JSON). */
function plainEntries(policy: string): [string, unknown][] | null {
  try {
    const entries = Object.entries(JSON.parse(policy) as Record<string, unknown>);
    return entries.every(([, v]) => Array.isArray(v) && v.every((x) => typeof x === "string" || typeof x === "number")) ? entries : null;
  } catch {
    return null;
  }
}

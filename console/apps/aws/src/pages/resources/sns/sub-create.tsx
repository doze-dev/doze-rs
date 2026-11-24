// Create subscription (sns__subscription-create, -http, -lambda): a page, with the call it will make beside it, the table of what each protocol
// changes under it, and the service's own refusal said in the call. From a topic's page the topic is prefilled (`?from=`).
import { useNavigate, useSearch } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { Alert } from "@doze/ui/components/app/alert";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { ApiFailure } from "@doze/ui/lib/api";
import { cn } from "@doze/ui/lib/utils";
import { useResourceList } from "../../../api/resources";
import { FormPage, useKindOp } from "../../../kit";
import { Rich } from "./parts";
import { countWord, disabledCount, PROTOCOLS, protocolOf, type Protocol } from "./protocols";
import { b, type Part } from "./sentences";
import { emptySubForm, problems, subscribeInput, type SubForm } from "./sub-form-model";
import { Details, FilterPolicy, Redrive } from "./sub-form-sections";
import { tail } from "./sub-model";

const SUBS = { service: "sns", kind: "subscriptions" };

function sentenceOf(f: SubForm): Part[] {
  const topic = tail(f.topic) || "the topic";
  const fifo = f.topic.endsWith(".fifo");
  const end = f.endpoint ? tail(f.endpoint) : "";
  if (f.protocol === "sqs") return ["This subscribes ", ...(end ? [fifo ? "the FIFO queue " : "the queue ", b(end)] : [fifo ? "a FIFO queue" : "a queue"]), fifo ? " to the FIFO topic " : " to ", b(topic), ". The queue is in your account, so the subscription is ", b("confirmed at once"), "."];
  if (f.protocol === "lambda" && !end) return ["Choose the function to subscribe to ", b(topic), "."];
  if (f.protocol === "lambda") return ["This subscribes the function ", b(end), " to ", b(topic), ". The function is in your account, so the subscription is ", b("confirmed at once"), " and every publish invokes it."];
  if (f.protocol === "http" || f.protocol === "https") return ["This subscribes the webhook ", b(f.endpoint || "…"), " to ", b(topic), ". SNS will send a confirmation message to it, and ", b("nothing is delivered"), " until the endpoint confirms."];
  return ["This subscribes ", b(f.endpoint || "…"), " to ", b(topic), `. ${protocolOf(f.protocol)?.about ?? ""}`];
}

const TABLE: { names: string; endpoint: string; confirm: string; raw: string; form?: Protocol }[] = [
  { names: "HTTP, HTTPS", endpoint: "A URL beginning with http:// or https://. A local URL works", confirm: "Pending until the endpoint confirms, by an HTTP GET to the SubscribeURL", raw: "Yes", form: "http" },
  { names: "Amazon SQS", endpoint: "The ARN of a queue. The choice for a FIFO topic", confirm: "Same account: at once", raw: "Yes" },
  { names: "AWS Lambda", endpoint: "The ARN of a function", confirm: "Same account: at once", raw: "No", form: "lambda" },
];

export function SubCreate() {
  const navigate = useNavigate();
  const search = useSearch({ strict: false }) as { from?: string };
  const [f, setF] = useState<SubForm>(() => emptySubForm(search.from ?? ""));
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<SubForm>) => setF((x) => ({ ...x, ...patch }));
  const make = useKindOp<{ SubscriptionArn?: string }>(SUBS, "Subscribe");
  const topics = useResourceList("sns", "topics");
  const first = topics.data?.pages[0];
  const p = problems(f, true);
  const input = useMemo(() => subscribeInput(f), [f]);
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : make.error ? { code: "Error", message: (make.error as Error).message } : null;
  const submit = () => {
    setTried(true);
    if (Object.keys(p).length > 0) return;
    make.mutate(input, { onSuccess: (r) => {
      const arn = r.SubscriptionArn ?? "";
      void navigate(arn.startsWith("arn:") ? { to: "/resources/$service/$kind/$name", params: { service: "sns", kind: "subscriptions", name: arn }, search: { done: "created" } } : { to: "/resources/$service/$kind/$name", params: { service: "sns", kind: "topics", name: tail(f.topic) }, search: { done: "subscribed", tab: "subscriptions" } });
    } });
  };
  const back = () => void navigate(search.from ? { to: "/resources/$service/$kind/$name", params: { service: "sns", kind: "topics", name: tail(search.from) }, search: {} } : { to: "/resources/$service/$second", params: { service: "sns", second: "subscriptions" }, search: {} });
  const info = protocolOf(f.protocol);
  return (
    <>
    <FormPage
      callFormat="outline"
      title="Create subscription"
      sub="Subscribe an endpoint to a topic so it receives what is published there."
      crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: "/resources/sns" }] : []), ...(search.from ? [{ label: tail(search.from), to: `/resources/sns/topics/${encodeURIComponent(tail(search.from))}` }] : [{ label: "Subscriptions", to: "/resources/sns/subscriptions" }])]}
      request={f.topic && f.endpoint ? { service: "sns", operation: "Subscribe", input } : null}
      refusal={refused}
      callNote={info?.confirms === "endpoint" ? "For HTTP/S, the answer is the text pending confirmation until the endpoint confirms; with ReturnSubscriptionArn true it is the pending subscription's ARN instead." : f.protocol === "lambda" ? "A Lambda subscription in the same account is confirmed at once, so the answer is the full subscription ARN." : undefined}
      banner={refused ? <Alert tone="bad" title="The subscription was not made." testId="create-refused">Subscribe returned {refused.code}: {refused.message}.</Alert> : <Rich parts={sentenceOf(f)} testId="create-sentence" />}
      submitLabel="Create subscription"
      pending={make.isPending}
      disabled={tried && Object.keys(p).length > 0}
      disabledWhy={Object.values(p)[0]}
      onCancel={back}
      onSubmit={submit}
      testId="sub-create-form"
    >
      <Details f={f} set={set} p={p} show={tried} />
      <FilterPolicy key={`filter-${f.protocol}`} f={f} set={set} p={p} show={tried} collapsed={f.protocol !== "sqs"} />
      <Redrive key={`redrive-${f.protocol}`} f={f} set={set} p={p} show={tried} collapsed={f.protocol !== "sqs"} />
    </FormPage>
    {f.protocol === "sqs" && <ProtocolTable open={(protocol) => { set({ protocol, endpoint: "", raw: false }); window.scrollTo({ top: 0 }); }} />}
    </>
  );
}

/** What each protocol changes: what Endpoint is, who confirms, whether raw delivery is offered; the ones doze-aws does not deliver to say why. */
export function ProtocolTable({ open }: { open: (p: Protocol) => void }) {
  const off: { names: string; about: string }[] = [
    { names: "Email, Email-JSON", about: PROTOCOLS.find((x) => x.id === "email")!.about },
    ...PROTOCOLS.filter((x) => !x.active && !["email", "email-json"].includes(x.id)).map((x) => ({ names: x.label, about: x.about })),
  ];
  const th = "px-4 py-3 text-left text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase";
  const td = "px-4 py-3.5 align-top text-[13.5px] leading-5";
  return (
    <section className="mt-12 grid gap-3" aria-label="What each protocol changes" data-testid="protocol-table">
      <h2 className="text-[17px] font-semibold tracking-[-0.01em] text-bright">What each protocol changes</h2>
      <p className="max-w-[56rem] text-[13.5px] text-muted-foreground">The Protocol you pick decides what Endpoint means and whether the endpoint owner must confirm. {countWord(disabledCount())} of the nine AWS protocols are disabled here, with the reason.</p>
      <div className="overflow-x-auto rounded-[var(--radius-card)] border bg-card">
        <table className="w-full min-w-[56rem] border-collapse">
          <thead className="bg-muted/50"><tr><th className={th}>Protocol</th><th className={th}>Endpoint is</th><th className={th}>Confirmation</th><th className={th}>Raw delivery</th><th className={th}>Form</th></tr></thead>
          <tbody className="divide-y">
            {TABLE.map((r) => (
              <tr key={r.names}>
                <td className={cn(td, "font-medium text-bright")}>{r.names}</td><td className={td}>{r.endpoint}</td><td className={td}>{r.confirm}</td><td className={td}>{r.raw}</td>
                <td className={td}>{r.form ? <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => open(r.form!)} data-testid={`open-${r.form}`}>Open the {r.form === "http" ? "HTTP" : "Lambda"} form</Button> : <span className="text-muted-foreground">This form</span>}</td>
              </tr>
            ))}
            {off.map((r) => (
              <tr key={r.names}>
                <td className={cn(td, "font-medium text-bright")}><span className="grid gap-1">{r.names}<Badge variant="secondary" className="w-fit font-normal text-muted-foreground">Disabled</Badge></span></td>
                <td className={cn(td, "text-muted-foreground")} colSpan={4}>{r.about}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <Callout tone="note" lead="HTTP(S) endpoints, email addresses, and resources in other AWS accounts require confirmation." testId="note">An Amazon SQS queue or Lambda function in your own account needs none. Raw delivery is offered for Amazon SQS and HTTP/S.</Callout>
    </section>
  );
}

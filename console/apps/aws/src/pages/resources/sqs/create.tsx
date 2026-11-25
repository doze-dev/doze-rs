// Create queue (sqs__queue-create, queue-create-fifo, queue-create-refused; queue-created is where it lands): a page, not a dialog,
// with the call it will make beside it, and the service's own refusal said on the field and in the call.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { RowsIcon } from "lucide-react";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { ApiFailure } from "@doze/ui/lib/api";
import { Link } from "@tanstack/react-router";
import { useAbout } from "../../../api/queries";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import { FormPage, useKindOp } from "../../../kit";
import { seconds } from "../model";
import { Configuration, type Hints, TypeAndName } from "./form-fields";
import { createInput, emptyForm, type QueueForm, policyBroken, problems } from "./form-model";
import { AccessPolicy, DeadLetter, Encryption, FoldedStart, RedriveAllow, TagsSection } from "./form-sections";
import { queueOf } from "./queue-model";
import { mismatched, nameTaken, whatItHas } from "./refusal";

const QUEUES = { service: "sqs", kind: "queues" };

/** What the queue named in the form already has, said under the fields it is a neighbour of. */
function hintsFor(existing: ReturnType<typeof queueOf> | null, name: string, neighbour: { name: string; retention: number } | null): Hints {
  if (!existing) return {};
  return {
    name: `${name} already exists in this Region.`,
    visibility: `${name} uses ${seconds(String(existing.visibility))}.`,
    // Its dead-letter queue is a neighbour of the retention: a dead-letter queue should keep messages longer than the queue that feeds it.
    retention: `${name} keeps ${seconds(String(existing.retention))}.${neighbour ? ` ${neighbour.name} keeps ${seconds(String(neighbour.retention))}.` : ""}`,
  };
}

export function SqsCreate() {
  const navigate = useNavigate();
  const about = useAbout().data;
  const [f, setF] = useState<QueueForm>(emptyForm);
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<QueueForm>) => setF((x) => ({ ...x, ...patch }));
  const region = about?.region ?? "us-east-1";
  const account = about?.account ?? "000000000000";
  const arn = `arn:aws:sqs:${region}:${account}:${f.name || "<queue-name>"}`;
  const make = useKindOp<{ QueueUrl: string }>(QUEUES, "CreateQueue");
  const list = useResourceList("sqs", "queues");
  const known = useMemo(() => new Set((list.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name)), [list.data]);
  const exists = known.has(f.name);
  const neighbour = useResourceDetail("sqs", "queues", f.name, exists);
  const existing = exists && neighbour.data ? queueOf(neighbour.data) : null;
  const deadName = existing?.dlq ?? null;
  const dead = useResourceDetail("sqs", "queues", deadName ?? "", deadName !== null);
  const deadHolds = deadName && dead.data ? { name: deadName, retention: queueOf(dead.data).retention } : null;
  const p = problems(f, { create: true });
  const input = useMemo(() => createInput(f, arn), [f, arn]);
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : make.error ? { code: "Error", message: String((make.error as Error).message) } : null;
  const taken = refused ? nameTaken(refused.code) : false;
  const attribute = taken && refused ? mismatched(refused.message) : null;
  const had = attribute && neighbour.data ? whatItHas(attribute, neighbour.data) : null;
  const first = list.data?.pages[0];
  const submit = () => {
    setTried(true);
    if (Object.keys(p).length > 0) return;
    make.mutate(input, { onSuccess: (r) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "sqs", kind: "queues", name: r.QueueUrl.split("/").pop() ?? f.name }, search: { done: "created" } }) });
  };
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "sqs", second: "queues" }, search: {} });
  return (
    <FormPage
      title="Create queue"
      sub="The console provides default values for all settings except the queue name."
      crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: "/resources/sqs" }] : []), { label: "Queues", to: "/resources/sqs/queues" }]}
      request={f.name ? { service: "sqs", operation: "CreateQueue", input } : null}
      refusal={refused}
      callNote={taken && had ? `The existing queue has ${had.name} ${had.value}. Sending the exact same attributes as the existing queue would have returned its URL instead of an error.` : f.fifo ? "Only the settings that differ from the defaults are sent." : "Only the settings that differ from the defaults are sent, so SQS applies its own defaults to the rest."}
      banner={
        refused ? (
          <Alert tone="bad" title="Queue not created." testId="create-refused">
            {refused.code}: {refused.message}.{taken ? " A request is refused only when an attribute it carries differs from the existing queue's." : ""}
          </Alert>
        ) : undefined
      }
      aside={
        taken && neighbour.data ? (
          <section className="rounded-[var(--radius-card)] border bg-card p-5" data-testid="what-it-has">
            <h2 className="text-[12px] font-medium tracking-[0.08em] text-muted-foreground uppercase">What {f.name} already has</h2>
            <p className="mt-2 text-[13.5px] text-muted-foreground">{had ? "The attribute the request does not match." : "The existing queue's settings."}</p>
            {had && <p className="mt-2 font-mono text-[13.5px] text-bright">{had.name} {had.value}</p>}
            <Button asChild variant="outline" size="sm" className="mt-4 bg-card"><Link to="/resources/$service/$kind/$name" params={{ service: "sqs", kind: "queues", name: f.name }} data-testid="open-existing"><RowsIcon /> Open {f.name}</Link></Button>
          </section>
        ) : undefined
      }
      submitLabel="Create queue"
      pending={make.isPending}
      disabled={policyBroken(f) || (tried && Object.keys(p).length > 0)}
      disabledWhy={policyBroken(f) ? "Fix the JSON policy to continue." : Object.values(p)[0]}
      onCancel={back}
      onSubmit={submit}
      testId="queue-create"
    >
      <TypeAndName f={f} set={set} p={p} show={tried} create hints={taken ? { ...hintsFor(existing, f.name, deadHolds), name: undefined } : hintsFor(existing, f.name, deadHolds)} refusal={taken ? { why: "A queue with this name already exists, with different attributes.", suggestion: "Choose another name, or send the same attributes the queue has." } : null} />
      <Configuration f={f} set={set} p={p} show={tried} create hints={hintsFor(existing, f.name, deadHolds)} />
      <FoldedStart.Provider value={taken} key={taken ? "refused" : "open"}>
        <AccessPolicy f={f} set={set} p={p} show={tried} queueArn={arn} basic />
        <RedriveAllow f={f} set={set} p={p} show={tried} name={f.name} />
        <Encryption f={f} set={set} p={p} show={tried} edit={false} />
        <DeadLetter f={f} set={set} p={p} show={tried} self={f.name} region={region} account={account} />
        <TagsSection f={f} set={set} show={tried} />
      </FoldedStart.Provider>
    </FormPage>
  );
}

// Create topic (sns__topic-create, topic-create-fifo, topic-create-refused; topic-created is where it lands): a page, with the call it
// will make beside it, and the service's own refusal said on the field and in the call.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { FormPage, useKindOp } from "../../../kit";
import { Rich } from "./parts";
import { b } from "./sentences";
import { createInput, emptyTopicForm, problems, type TopicForm } from "./topic-form-model";
import { RetryPolicy, Logging, TagsSection, Tracing } from "./topic-form-delivery";
import { AccessPolicy, Details, Encryption, FifoOptions, MaxSize } from "./topic-form-fields";

const TOPICS = { service: "sns", kind: "topics" };

export function TopicCreate() {
  const navigate = useNavigate();
  const about = useAbout().data;
  const region = about?.region ?? "us-east-1";
  const account = about?.account ?? "000000000000";
  const [f, setF] = useState<TopicForm>(emptyTopicForm);
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<TopicForm>) => setF((x) => ({ ...x, ...patch }));
  const make = useKindOp<{ TopicArn: string }>(TOPICS, "CreateTopic");
  const topics = useResourceList("sns", "topics");
  const queues = useResourceList("sqs", "queues");
  const first = topics.data?.pages[0];
  const hints = useMemo(() => ({ topics: (topics.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name), fifoQueues: (queues.data?.pages.flatMap((p) => p.rows) ?? []).filter((r) => r.cells[0] === "FIFO").map((r) => r.name), region }), [topics.data, queues.data, region]);
  const p = problems(f, true);
  const input = useMemo(() => createInput(f, region, account), [f, region, account]);
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : make.error ? { code: "Error", message: (make.error as Error).message } : null;
  const badFifoName = f.fifo && !f.name.endsWith(".fifo");
  const nameRefusal = refused && badFifoName ? { why: "For a FIFO topic, the name must end with the .fifo suffix.", suggestion: `Try ${f.name}.fifo.` } : refused && /Topic Name/i.test(refused.message) ? { why: refused.message } : null;
  const submit = () => {
    setTried(true);
    if (Object.keys(p).length > 0) return;
    make.mutate(input, { onSuccess: (r) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "sns", kind: "topics", name: r.TopicArn.split(":").pop() ?? f.name }, search: { done: "created" } }) });
  };
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "sns", second: "topics" }, search: {} });
  const named = f.name.trim() || "<topic-name>";
  return (
    <FormPage
      callFormat="outline"
      key={f.fifo ? "fifo" : "standard"}
      title="Create topic"
      sub={refused ? "Nothing was created. Fix the name and try again." : f.fifo ? "FIFO topics keep messages in order within a message group and drop duplicates." : "Choose the type and the name. Everything else is optional and can be edited later."}
      crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.service_title, to: "/resources/sns" }] : []), { label: "Topics", to: "/resources/sns/topics" }]}
      request={f.name.trim() ? { service: "sns", operation: "CreateTopic", input } : null}
      refusal={refused}
      callNote={refused && badFifoName ? "The rule comes from the API: a FIFO topic's name must end with .fifo." : <>Settings left at their defaults are not sent. To tag a topic on creation you need the sns:CreateTopic and sns:TagResource permissions.{f.fifo ? " If the name is refused, CreateTopic answers InvalidParameter." : ""}</>}
      banner={
        <div className="grid gap-4">
          {refused ? (
            <Alert tone="bad" title="No topic was created." testId="create-refused">
              CreateTopic returned {refused.code}: {refused.message}.{badFifoName ? " For a FIFO topic, the name must end with the .fifo suffix." : ""}
            </Alert>
          ) : (
            <Rich testId="create-sentence" parts={f.fifo ? ["This creates a ", b("FIFO"), " topic named ", b(named), ". Publishers must send a ", b("message group ID"), ...(f.dedup ? [" on every message."] : [" and, because deduplication is by ID, a ", b("deduplication ID"), " on every message."])] : ["This creates a ", b("standard"), " topic named ", b(named), ` in ${region}, with no subscriptions yet.`]} />
          )}
        </div>
      }
      submitLabel="Create topic"
      pending={make.isPending}
      disabled={tried && Object.keys(p).length > 0}
      disabledWhy={Object.values(p)[0]}
      onCancel={refused ? () => make.reset() : back}
      cancelLabel={refused ? "Back to the form" : "Cancel"}
      onSubmit={submit}
      testId="topic-create"
    >
      <Details f={f} set={set} p={p} show={tried} create hints={hints} refusal={nameRefusal} />
      {f.fifo && <FifoOptions f={f} set={set} />}
      <MaxSize f={f} set={set} p={p} show={tried} collapsed={f.fifo} />
      <Encryption f={f} set={set} p={p} show={tried} collapsed={f.fifo} />
      <AccessPolicy f={f} set={set} p={p} show={tried} name={f.name} region={region} account={account} collapsed={f.fifo} />
      {!f.fifo && <RetryPolicy f={f} set={set} p={p} show={tried} collapsed={false} />}
      <Logging f={f} set={set} p={p} show={tried} collapsed={f.fifo} account={account} />
      {!f.fifo && <Tracing f={f} set={set} collapsed={false} />}
      <TagsSection f={f} set={set} show={tried} collapsed={f.fifo} />
    </FormPage>
  );
}

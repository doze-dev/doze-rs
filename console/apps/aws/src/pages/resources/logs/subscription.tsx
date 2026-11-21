// Subscription filter (Lambda) (logs__subscription-filter-create): the matching events of a group are sent to a function as they arrive
// (PutSubscriptionFilter).
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { Field, FormPage, FormSection, FunctionPicker } from "../../../kit";
import { useGroupWrite } from "./api";
import { filterNameProblem, functionArn, isOwnGroup, patternProblem, subscriptionInput } from "./model";

export function SubscriptionCreate({ group }: { group: string }) {
  const navigate = useNavigate();
  const about = useAbout().data;
  const [fn, setFn] = useState<string | null>(null);
  const [pattern, setPattern] = useState("");
  const [name, setName] = useState("");
  const [tried, setTried] = useState(false);
  const put = useGroupWrite("PutSubscriptionFilter");
  const arn = fn ? functionArn(about?.region ?? "us-east-1", about?.account ?? "000000000000", fn) : "<function ARN>";
  const input = subscriptionInput(group, name, pattern, arn);
  const own = fn !== null && isOwnGroup(group, fn);
  const p = { fn: fn === null ? "Choose a function." : own ? "A function can't subscribe to its own /aws/lambda/ group: every line it wrote would invoke it again." : null, name: filterNameProblem(name), pattern: patternProblem(pattern) };
  const bad = Object.values(p).find(Boolean) ?? null;
  const refused = put.error instanceof ApiFailure ? { code: put.error.code, message: put.error.message } : null;
  const back = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "logs", kind: "groups", name: group }, search: {} });
  return (
    <FormPage
      title="Subscription filter (Lambda)"
      sub={<>Send matching events from <span className="font-mono text-foreground">{group}</span> to a Lambda function as they arrive. <span className="text-muted-foreground">Choose the function, the pattern and a name.</span></>}
      crumbs={[{ label: "Resources", to: "/resources" }, { label: "CloudWatch Logs", to: "/resources/logs" }, { label: "Log groups", to: "/resources/logs/groups" }, { label: group, to: `/resources/logs/groups/${encodeURIComponent(group)}` }]}
      request={fn ? { service: "logs", operation: "PutSubscriptionFilter", input } : null}
      callNote="No roleArn: a Lambda destination doesn't need one."
      refusal={refused}
      banner={refused ? <Alert tone="bad" title="Subscription filter not created." testId="create-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Create subscription filter"
      pending={put.isPending}
      disabled={tried && bad !== null}
      disabledWhy={bad ?? undefined}
      onCancel={back}
      onSubmit={() => {
        setTried(true);
        if (bad) return;
        put.mutate({ group, input }, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "logs", kind: "groups", name: group }, search: { tab: "subscription-filters", done: "subscription-created", name } }) });
      }}
      testId="subscription-create"
    >
      <FormSection title="Destination">
        <Field label="Lambda function" error={tried || own ? (p.fn ?? undefined) : undefined} help={<>Same-account functions only. On AWS, CloudWatch Logs must be allowed to invoke the function: principal <span className="font-mono">logs.amazonaws.com</span>, action <span className="font-mono">lambda:InvokeFunction</span>. A function can't subscribe to its own <span className="font-mono">/aws/lambda/</span> group: every line it wrote would invoke it again.</>}>
          {(id) => <FunctionPicker id={id} by="name" value={fn} onChange={setFn} />}
        </Field>
      </FormSection>
      <FormSection title="Subscription filter">
        <Field label="Subscription filter pattern" error={tried ? (p.pattern ?? undefined) : undefined} help="Only matching events are sent. An empty pattern matches every event. Regular-expression and space-delimited patterns work in doze-aws within AWS's limits.">
          {(id) => <Input id={id} value={pattern} onChange={(e) => setPattern(e.target.value)} placeholder='{ $.level = "error" }' autoComplete="off" spellCheck={false} className="font-mono" data-testid="sub-pattern" />}
        </Field>
        <Field label="Filter name" error={tried ? (p.name ?? undefined) : undefined} help={<>filterName: up to <b>512</b> characters, no <span className="font-mono">:</span> or <span className="font-mono">*</span>. Saving under an existing name updates that filter. A group holds up to <b>2</b> subscription filters.</>}>
          {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} placeholder="errors-to-email" autoComplete="off" spellCheck={false} className="font-mono" data-testid="sub-name" />}
        </Field>
      </FormSection>
      <p className="max-w-[44rem] text-[13px] leading-5 text-muted-foreground" data-testid="sub-teaching">
        Lambda receives each batch gzip-compressed and base64-encoded in <span className="font-mono">awslogs.data</span>. Delivery to a destination is one attempt in doze-aws; a failure is logged with the group, the filter and the destination.
      </p>
    </FormPage>
  );
}

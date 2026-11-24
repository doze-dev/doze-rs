// Edit subscription (sns__subscription-edit): raw message delivery, the filter policy and the redrive policy, saved one SetSubscriptionAttributes call for each
// setting that changed. The topic, the protocol and the endpoint cannot change.
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { FormPage, type PreviewRequest, RuleHelp } from "../../../kit";
import { useOp } from "../deep/api";
import { Rich } from "./parts";
import { protocolLabel } from "./protocols";
import { changes, changeWords, formOfSubscription, problems, type Subscription, type SubForm } from "./sub-form-model";
import { Details, FilterPolicy, Redrive } from "./sub-form-sections";
import { count } from "./words";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";

export function SubEdit({ d, s, back, done }: { d: ResourceDetail; s: Subscription; back: () => void; done: (done: string) => void }) {
  const target = { service: d.service, kind: d.kind, name: d.name };
  const [f, setF] = useState<SubForm>(() => formOfSubscription(s));
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<SubForm>) => setF((x) => ({ ...x, ...patch }));
  const save = useOp(target, "SetSubscriptionAttributes", { keep: true });
  const qc = useQueryClient();
  const p = problems(f, false);
  const changed = useMemo(() => changes(f, s), [f, s]);
  const nothing = changed.length === 0;
  const bad = Object.keys(p).length > 0;
  const refused = save.error instanceof ApiFailure ? { code: save.error.code, message: save.error.message } : save.error ? { code: "Error", message: (save.error as Error).message } : null;
  const requests: PreviewRequest[] = changed.map((c) => ({ service: "sns", operation: "SetSubscriptionAttributes", input: { SubscriptionArn: s.arn, AttributeName: c.name, AttributeValue: c.value } }));
  const submit = async () => {
    setTried(true);
    if (bad || nothing) return;
    try {
      for (const c of changed) await save.mutateAsync({ AttributeName: c.name, AttributeValue: c.value });
      await qc.invalidateQueries({ queryKey: ["resources", "one", d.service, d.kind, d.name] });
      void qc.invalidateQueries({ queryKey: ["resources", "list", d.service, d.kind] });
      done("updated");
    } catch {
      /* the refusal is shown */
    }
  };
  return (
    <FormPage
      callFormat="outline"
      title={`Edit ${s.arn?.split(":").pop() ?? d.name}`}
      sub="The subscription's Details section, filter policy and redrive policy."
      crumbs={[{ label: "Resources", to: "/resources" }, { label: d.service_title, to: `/resources/${d.service}` }, { label: "Subscriptions", to: "/resources/sns/subscriptions" }, { label: (s.arn?.split(":").pop() ?? d.name).slice(0, 8), to: `/resources/sns/subscriptions/${encodeURIComponent(d.name)}` }]}
      mono
      request={requests[0] ?? null}
      requests={requests}
      refusal={refused}
      callNote="SetSubscriptionAttributes sets one attribute per call, so each changed setting is its own call."
      banner={refused ? <Alert tone="bad" title="The changes were not saved." testId="edit-refused">{refused.code}: {refused.message}</Alert> : <Rich testId="edit-sentence" parts={nothing ? ["Nothing has changed yet."] : [`${changeWords(changed)} The rest is unchanged. Saving sends ${count(changed.length, "call")}.`]} />}
      submitLabel="Save changes"
      pending={save.isPending}
      disabled={nothing || (tried && bad)}
      disabledWhy={nothing ? "Nothing has changed." : Object.values(p)[0]}
      onCancel={back}
      onSubmit={() => void submit()}
      testId="sub-edit-form"
    >
      <Details f={f} set={set} p={p} show={tried} locked={
        <>
          <p className="text-[13.5px] text-muted-foreground">These cannot change. To use another endpoint, create a new subscription.</p>
          <dl className="grid grid-cols-[10rem_minmax(0,1fr)] gap-y-2 text-[13.5px]">
            <dt className="text-muted-foreground">Subscription ARN</dt><dd className="font-mono break-all text-bright" data-testid="edit-arn">{s.arn}</dd>
            <dt className="text-muted-foreground">Topic ARN</dt><dd className="font-mono break-all text-bright">{s.topicArn}</dd>
            <dt className="text-muted-foreground">Endpoint</dt><dd className="font-mono break-all text-bright">{s.endpoint}</dd>
            <dt className="text-muted-foreground">Protocol</dt><dd className="text-bright">{protocolLabel(s.protocol)}</dd>
          </dl>
        </>
      } after={s.protocol === "sqs" ? <RuleHelp><b>With raw delivery on, an SQS subscription carries at most 10 message attributes.</b> A message with more is discarded as a client-side error. Turn raw delivery off to send more than 10.</RuleHelp> : undefined} />
      <FilterPolicy f={f} set={set} p={p} show={tried} collapsed={false} />
      <Redrive f={f} set={set} p={p} show={tried} collapsed={false} />
    </FormPage>
  );
}

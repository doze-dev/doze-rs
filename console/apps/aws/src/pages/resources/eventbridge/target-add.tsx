// Add target (eventbridge__rule-targets-none's way on): one target put on a rule that exists, with the call it makes.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { FormPage, useKindOp, useServiceCrumbs } from "../../../kit";
import { RULES, ruleAddress } from "./api";
import { useWhere } from "./parts";
import { type TargetDraft, newTarget, targetId, targetOf, targetProblem } from "./rule-model";
import { TargetForm } from "./target-form";
import type { TargetItem } from "./types";

export function TargetAdd({ bus, rule, taken, back }: { bus: string; rule: string; taken: readonly string[]; back: () => void }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const { region, account } = useWhere();
  const [t, setT] = useState<TargetDraft>(newTarget());
  const [tried, setTried] = useState(false);
  const put = useKindOp(RULES, "PutTargets", { keep: true });
  const problem = targetProblem(t);
  const target: TargetItem = targetOf(t, targetId(t, taken));
  const input = { Rule: rule, EventBusName: bus, Targets: [target] };
  const failed = put.error instanceof ApiFailure ? { code: put.error.code, message: put.error.message } : null;
  return (
    <FormPage
      title="Add target"
      sub={<>Send the events <b className="font-mono font-semibold text-foreground">{rule}</b> matches to one more target. A rule can have up to 5.</>}
      crumbs={[...crumbs, { label: "Rules", to: "/resources/eventbridge/rules" }, { label: rule, to: `/resources/eventbridge/rules/${encodeURIComponent(ruleAddress(bus, rule))}` }]}
      request={{ service: "events", operation: "PutTargets", input }}
      refusal={failed}
      callNote="PutTargets adds the target to the rule's targets; one with the same Id is replaced."
      banner={failed ? <Alert tone="bad" title="The target wasn't added." testId="target-refused">PutTargets answered <span className="font-mono font-semibold text-bright">{failed.code}</span>: “{failed.message}”</Alert> : undefined}
      submitLabel="Add target"
      pending={put.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={back}
      onSubmit={() => {
        setTried(true);
        if (!problem) put.mutate(input as never, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "eventbridge", kind: "rules", name: ruleAddress(bus, rule) }, search: { done: "target" } }) });
      }}
      testId="target-add"
    >
      <TargetForm t={t} index={0} set={(p) => setT((x) => ({ ...x, ...p }))} region={region} account={account} tried={tried} />
    </FormPage>
  );
}

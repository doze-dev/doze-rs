// Delete rule (eventbridge__rule-delete): RemoveTargets, then DeleteRule, each shown as the call it is; the targets' own resources are not touched.
import { useState } from "react";
import { CallsPanel, PlainConfirm, callKindOp } from "../../../kit";
import { RULES } from "./api";
import type { TargetItem } from "./types";
import { list } from "./words";

export type Doomed = { name: string; targets: readonly TargetItem[] };

/** What deleting `rules` on `bus` calls, in order: a rule's targets are removed first (EventBridge refuses a rule that has them). */
export function deleteCalls(bus: string, rules: readonly Doomed[]) {
  return rules.flatMap((r) => [
    ...(r.targets.length > 0 ? [{ service: "events", operation: "RemoveTargets", input: { Rule: r.name, EventBusName: bus, Ids: r.targets.map((t) => t.Id) } }] : []),
    { service: "events", operation: "DeleteRule", input: { Name: r.name, EventBusName: bus } },
  ]);
}

export function RuleDelete({ open, onOpenChange, bus, rules, onDone }: { open: boolean; onOpenChange: (o: boolean) => void; bus: string; rules: readonly Doomed[]; onDone: (names: string[]) => void }) {
  const [state, setState] = useState<{ pending: boolean; error: unknown }>({ pending: false, error: null });
  const names = rules.map((r) => r.name);
  const run = async () => {
    setState({ pending: true, error: null });
    try {
      for (const call of deleteCalls(bus, rules)) await callKindOp(RULES, call.operation, call.input);
      setState({ pending: false, error: null });
      onDone(names);
    } catch (error) {
      setState({ pending: false, error });
    }
  };
  const topics = [...new Set(rules.flatMap((r) => r.targets.map((t) => t.Arn.split(":").pop() ?? t.Arn)))];
  return (
    <PlainConfirm
      open={open}
      onOpenChange={(o) => { onOpenChange(o); if (!o) setState({ pending: false, error: null }); }}
      title={names.length === 1 ? "Delete rule?" : `Delete ${names.length} rules?`}
      description={<>Delete <b className="font-mono font-semibold text-bright">{list(names)}</b> from the <b className="font-mono font-semibold text-bright">{bus}</b> bus?</>}
      call="DeleteRule"
      pending={state.pending}
      error={state.error}
      onConfirm={() => void run()}
      testId="rule-delete"
    >
      <CallsPanel requests={deleteCalls(bus, rules)} title="The calls this will make" testId="delete-calls" note={<>The API reference says a rule's targets must be removed before the rule can be deleted, which is why the calls go in this order.{topics.length > 0 ? <> {list(topics)} {topics.length === 1 ? "is" : "are"} not touched.</> : null}</>} />
    </PlainConfirm>
  );
}

// What feeds a queue: the rules of every bus that target it. EventBridge lists them a bus at a time, so the page lists the buses
// and asks each (the queue's bound operations `ListEventBuses` and `ListRuleNamesByTarget`, crates/aws-console/src/resources/sqs.rs).
import { useQuery } from "@tanstack/react-query";
import { callOp, type Target } from "../deep/api";

export type Feeder = { bus: string; rule: string };

/** The address of a rule in the console: `<bus>/<rule>`, or the rule alone on the default bus. */
export const ruleName = (f: Feeder): string => (f.bus === "default" ? f.rule : `${f.bus}/${f.rule}`);

export function useFeeders(t: Target) {
  return useQuery({
    queryKey: ["sqs", "feeders", t.name],
    queryFn: async (): Promise<Feeder[]> => {
      const buses = await callOp<{ EventBuses?: { Name: string }[] }>(t, "ListEventBuses", {});
      const found = await Promise.all((buses.EventBuses ?? []).map(async (b) => {
        const r = await callOp<{ RuleNames?: string[] }>(t, "ListRuleNamesByTarget", { EventBusName: b.Name });
        return (r.RuleNames ?? []).map((rule) => ({ bus: b.Name, rule }));
      }));
      return found.flat();
    },
    retry: false,
    staleTime: 0,
  });
}

/** `the rule payment-settled`, `the rules a and b`: the sentence's name for them. */
export function feederWords(fs: readonly Feeder[]): string {
  const names = fs.map((f) => f.rule);
  if (names.length === 0) return "";
  return names.length === 1 ? `The rule ${names[0]}` : `The rules ${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

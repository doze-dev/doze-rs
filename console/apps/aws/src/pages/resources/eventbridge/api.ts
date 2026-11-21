// The calls the EventBridge pages make: each is an operation the kind allows (`ops` in crates/aws-console/src/resources/eventbridge/),
// made through the console as the caller `console`. The reads are made when a page opens and when the person asks.
import { useQueries, useQuery } from "@tanstack/react-query";
import { type Kind, callKindOp, useKindOp, useKindOpRead } from "../../../kit";
import type { ApiDests, Archives, Buses, Connections, Replays, RuleItem, Rules, TargetItem, Targets } from "./types";

export const RULES: Kind = { service: "eventbridge", kind: "rules" };
export const BUSES: Kind = { service: "eventbridge", kind: "buses" };
export const ARCHIVES: Kind = { service: "eventbridge", kind: "archives" };
export const REPLAYS: Kind = { service: "eventbridge", kind: "replays" };
export const DESTS: Kind = { service: "eventbridge", kind: "api-destinations" };

/** `arn:aws:events:us-east-1:111122223333:event-bus/larder`. */
export const busArn = (region: string, account: string, bus: string) => `arn:aws:events:${region}:${account}:event-bus/${bus}`;
/** A rule's ARN: `rule/<name>` on the default bus, `rule/<bus>/<name>` on another. */
export const ruleArn = (region: string, account: string, bus: string, rule: string) => `arn:aws:events:${region}:${account}:rule/${bus === "default" ? "" : `${bus}/`}${rule}`;
export const archiveArn = (region: string, account: string, name: string) => `arn:aws:events:${region}:${account}:archive/${name}`;

/** A rule's name in the console's addresses: `larder/order-placed`, or the bare name on the default bus. */
export const ruleAddress = (bus: string, rule: string) => (bus === "default" ? rule : `${bus}/${rule}`);

/** The event buses (every account has the default one). */
export const useBuses = () => useKindOpRead<Buses>(BUSES, "ListEventBuses", {});
/** The rules of one bus. */
export const useRulesOf = (bus: string, enabled = true) => useKindOpRead<Rules>(RULES, "ListRules", { EventBusName: bus, Limit: 100 }, enabled);
/** Every archive, or those of one bus. */
export const useArchives = (sourceArn?: string) => useKindOpRead<Archives>(ARCHIVES, "ListArchives", sourceArn ? { EventSourceArn: sourceArn, Limit: 100 } : { Limit: 100 });
export const useReplays = () => useKindOpRead<Replays>(REPLAYS, "ListReplays", { Limit: 100 });
export const useDests = () => useKindOpRead<ApiDests>(DESTS, "ListApiDestinations", { Limit: 100 });
export const useConnections = () => useKindOpRead<Connections>(DESTS, "ListConnections", { Limit: 100 });

/** The targets of each rule asked for (by rule name on `bus`), by name; a rule's missing key is a read that has not answered. */
export function useTargetsOf(bus: string, rules: readonly RuleItem[]): Record<string, TargetItem[]> {
  const reads = useQueries({
    queries: rules.map((r) => ({ queryKey: ["resources", "kindop", "eventbridge", "rules", "ListTargetsByRule", bus, r.Name], queryFn: () => callKindOp<Targets>(RULES, "ListTargetsByRule", { Rule: r.Name, EventBusName: bus }), retry: false })),
  });
  return Object.fromEntries(rules.flatMap((r, i) => (reads[i]?.data ? [[r.Name, reads[i]?.data?.Targets ?? []] as const] : [])));
}

/** The targets of one rule. */
export const useTargets = (bus: string, rule: string, enabled = true) =>
  useQuery({ queryKey: ["resources", "kindop", "eventbridge", "rules", "ListTargetsByRule", bus, rule], queryFn: ({ signal }) => callKindOp<Targets>(RULES, "ListTargetsByRule", { Rule: rule, EventBusName: bus }, signal), enabled, retry: false });

/** A write on rules; what the pages read of rules is stale after. */
export const useRuleOp = <T = unknown, I extends object = object>(operation: string) => useKindOp<T, I>(RULES, operation);

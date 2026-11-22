// The calls the EventBridge pages make: each is an operation the kind allows (`ops` in crates/aws-console/src/resources/eventbridge/),
// made through the console as the caller `console`. The reads are made when a page opens and when the person asks.
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
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
/** The event pattern of each archive (ListArchives leaves it out; DescribeArchive has it), by name. */
export function useArchivePatterns(archives: readonly { ArchiveName: string }[]): Record<string, string | undefined> {
  const reads = useQueries({ queries: archives.map((a) => ({ queryKey: ["resources", "kindop", "eventbridge", "archives", "DescribeArchive", a.ArchiveName], queryFn: () => callKindOp<{ EventPattern?: string }>(ARCHIVES, "DescribeArchive", { ArchiveName: a.ArchiveName }), retry: false })) });
  return Object.fromEntries(archives.map((a, i) => [a.ArchiveName, reads[i]?.data?.EventPattern]));
}
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

/** A write (a create, a start): what the pages read of EventBridge (the lists, the pickers, a resource's page) is stale after it. */
export function useEbOp<T, I extends object = object>(k: Kind, operation: string) {
  const qc = useQueryClient();
  const op = useKindOp<T, I>(k, operation);
  return { ...op, mutate: ((input: I, options?: Parameters<typeof op.mutate>[1]) => op.mutate(input, { ...options, onSuccess: (...args) => { void qc.invalidateQueries({ queryKey: ["resources", "kindop", "eventbridge"] }); void qc.invalidateQueries({ queryKey: ["resources", "one", "eventbridge"] }); options?.onSuccess?.(...args); } })) as typeof op.mutate };
}

/** A write on rules; what the pages read of rules is stale after. */
export const useRuleOp = <T = unknown, I extends object = object>(operation: string) => useKindOp<T, I>(RULES, operation);

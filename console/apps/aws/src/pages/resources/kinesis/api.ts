// The calls the Kinesis pages make on one stream: each is bound by the server to the stream in the address.
import { useMutation, useQueries, useQueryClient } from "@tanstack/react-query";
import { callKindOp, type Kind } from "../../../kit";
import { callOp, deepKey } from "../deep/api";

export const STREAMS: Kind = { service: "kinesis", kind: "streams" };
/** One stream, as the deep pages address it. */
export const on = (name: string) => ({ service: "kinesis", kind: "streams", name });

/** The consumers of a stream (by ARN) as ListStreamConsumers answers. */
export type Consumers = { Consumers?: { ConsumerName: string }[] };

/** A write on the stream `name`; the stream's page and the list are stale after. */
export function useStreamWrite<I extends object = object>(operation: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ stream, input }: { stream: string; input: I }) => callOp<unknown>(on(stream), operation, input),
    onSuccess: (_, { stream }) => {
      void qc.invalidateQueries({ queryKey: deepKey(on(stream)) });
      void qc.invalidateQueries({ queryKey: ["resources", "one", "kinesis", "streams", stream] });
      void qc.invalidateQueries({ queryKey: ["resources", "list", "kinesis", "streams"] });
    },
  });
}

/** The consumers' names of each stream asked for (`arns` by name), read through the kind. */
export function useConsumerNames(arns: Readonly<Record<string, string>>): Record<string, string[]> {
  const names = Object.keys(arns);
  const reads = useQueries({
    queries: names.map((n) => ({ queryKey: ["resources", "kindop", "kinesis", "streams", "ListStreamConsumers", arns[n]], queryFn: () => callKindOp<Consumers>(STREAMS, "ListStreamConsumers", { StreamARN: arns[n] }), retry: false })),
  });
  return Object.fromEntries(names.map((n, i) => [n, (reads[i]?.data?.Consumers ?? []).map((c) => c.ConsumerName)]));
}

export const streamArn = (region: string, account: string, name: string) => `arn:aws:kinesis:${region}:${account}:stream/${name}`;

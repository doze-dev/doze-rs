// The calls the Logs pages make on one log group: each is bound by the server to the group in the address.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { callOp, deepKey, on } from "../deep/api";

/** A write on the group `name` (the server sets the group, whatever the input says). The group's page and the list are stale after. */
export function useGroupWrite<I extends object = object>(operation: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ group, input }: { group: string; input: I }) => callOp<unknown>(on.group(group), operation, input),
    onSuccess: (_, { group }) => {
      void qc.invalidateQueries({ queryKey: deepKey(on.group(group)) });
      void qc.invalidateQueries({ queryKey: ["resources", "one", "logs", "groups", group] });
      void qc.invalidateQueries({ queryKey: ["resources", "list", "logs", "groups"] });
    },
  });
}

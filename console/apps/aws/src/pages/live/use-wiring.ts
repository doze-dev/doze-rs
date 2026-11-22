// What the map is wired from: the resources of the services apps called and the lines between them, read through the bus as this
// console (Live hides those calls by default). Read when Live opens, then every ten seconds while it is on screen: a service no app
// called is never looked in, so nothing asleep is woken.
import { useQuery } from "@tanstack/react-query";
import { client, PATHS } from "../../api/client";
import type { Wiring } from "../../api/types/Wiring";

export function useWiring(enabled: boolean) {
  return useQuery({ queryKey: ["live", "wiring"], queryFn: ({ signal }) => client.api<Wiring>(PATHS.wiring, {}, signal), enabled, refetchInterval: enabled ? 10_000 : false, staleTime: 5_000 });
}

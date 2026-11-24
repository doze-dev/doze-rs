// The snapshot queries: what a page shows until it is refreshed (the RefreshControl's button, or its interval).
import { useQuery } from "@tanstack/react-query";
import { usePollInterval } from "@doze/ui/lib/refresh";
import { client, PATHS } from "./client";
import { useSelectedRegion } from "./region";
import type { About } from "./types/About";
import type { ServicesResponse } from "./types/ServicesResponse";

/** `GET /api/v1/about`: this doze-aws. The frame's chips and Home read it; it is one query. */
export function useAbout() {
  const refetchInterval = usePollInterval();
  // `region` is the region the console is looking at (the default one until another is chosen): every page that words or builds an
  // address with the region follows the choice.
  const chosen = useSelectedRegion();
  return useQuery({ queryKey: ["about"], queryFn: ({ signal }) => client.api<About>(PATHS.about, {}, signal), refetchInterval, select: (a: About) => (chosen ? { ...a, region: chosen } : a) });
}

/** `GET /api/v1/services`: every service of the build and how much of it is built. */
export function useServices() {
  const refetchInterval = usePollInterval();
  return useQuery({ queryKey: ["services"], queryFn: ({ signal }) => client.api<ServicesResponse>(PATHS.services, {}, signal), refetchInterval });
}

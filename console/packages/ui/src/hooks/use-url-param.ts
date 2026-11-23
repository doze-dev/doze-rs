import { useNavigate, useRouterState } from "@tanstack/react-router";
import { useCallback } from "react";
import { paramText, withParam } from "../lib/url-param";

/** One address parameter (`?state=ALARM`, `?tab=tags`) as state: its text (or `undefined`) and a setter that rewrites the
 * address in place (`replace`), keeping every other parameter. */
export function useUrlParam(key: string): [string | undefined, (value: string | null | undefined) => void] {
  // The router is the app's (its search keys are its own): read as plain text params, whatever they are typed there.
  const search = useRouterState({ select: ((s: { location: { search: unknown } }) => s.location.search) as never }) as unknown as Record<string, unknown>;
  const navigate = useNavigate();
  const set = useCallback(
    (value: string | null | undefined) => {
      void navigate({ to: ".", search: ((prev: Record<string, unknown>) => withParam(prev, key, value)) as never, replace: true });
    },
    [navigate, key],
  );
  return [paramText(search[key]), set];
}

// Reading one IAM resource for its page: the loading state, what IAM says when it is not there, a failed read, and the refresh.
import { useQueryClient } from "@tanstack/react-query";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceDetail } from "../../../api/resources";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { ResourcePageSkeleton } from "../../../kit";
import { HINT } from "../index-page";
import { useRefresh } from "./api";
import { KINDS, NotThere } from "./parts";

const GONE = /NoSuchEntity|NOT_FOUND|NotFound/i;

/** The detail of `name`, drawn by `render` once read; the words of a missing one, a failed read with its retry. */
export function Entity({ kind, name, render }: { kind: keyof typeof KINDS; name: string; render: (d: ResourceDetail, tools: { updatedAt: number; fetching: boolean; refresh: () => void }) => React.ReactNode }) {
  const detail = useResourceDetail("iam", kind, name);
  const refreshAll = useRefresh();
  const qc = useQueryClient();
  const { refetch } = detail;
  const refresh = () => {
    void refetch();
    refreshAll();
    void qc.invalidateQueries({ queryKey: ["resources", "tags", "iam", kind, name] });
  };
  if (detail.isError && !detail.data) {
    const e = detail.error;
    if (e instanceof ApiFailure && e.status > 0 && e.status < 500 && GONE.test(e.code)) return <NotThere kind={kind} name={name} />;
    return <ErrorBlock what={`This ${{ users: "user", groups: "user group", roles: "role", policies: "policy" }[kind]} could not be read`} error={e} hint={HINT} onRetry={refresh} retrying={detail.isFetching} testId="detail-error" />;
  }
  if (!detail.data) return <ResourcePageSkeleton />;
  return <>{render(detail.data, { updatedAt: detail.dataUpdatedAt, fetching: detail.isFetching, refresh })}</>;
}

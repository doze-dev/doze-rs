// The pages of `?do=create` (Workflow Studio on a draft) and `?do=edit` (Workflow Studio on a machine), and the Create alias and
// Manage tags pages, as the registry draws them.
import { useResourceDetail, useResourceList } from "../../../api/resources";
import { DetailError, ResourcePageSkeleton } from "../../../kit";
import type { OwnProps } from "../deep/registry";
import { Studio } from "./studio";
import { useGoTo } from "./hooks";
import { useEffect } from "react";

/** Create state machine: Workflow Studio on a draft named by the dialog. A name that did not come with the address goes back to the list. */
export function StudioCreate({ search }: OwnProps) {
  const go = useGoTo();
  const list = useResourceList("stepfunctions", "machines");
  const name = search.name ?? "";
  useEffect(() => {
    if (!name) go.list();
  }, [name, go]);
  if (!name) return null;
  return <Studio key={name} editing={false} name={name} taken={list.data?.pages.flatMap((p) => p.rows.map((r) => r.name)) ?? []} />;
}

/** Edit state machine: Workflow Studio on what the machine is now. */
export function StudioEdit({ name }: OwnProps) {
  const detail = useResourceDetail("stepfunctions", "machines", name ?? "");
  if (detail.isError && !detail.data) return <DetailError service="stepfunctions" kind="machines" name={name ?? ""} error={detail.error} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />;
  if (!detail.data) return <ResourcePageSkeleton />;
  return <Studio key={`${detail.data.name}`} editing detail={detail.data} />;
}

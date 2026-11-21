// A stack's address that CloudFormation no longer answers to: say whether it was deleted (ListStacks keeps a deleted stack's
// record, by its id, for 90 days) or was never made. DescribeStackEvents does not answer to a deleted stack's name, only to its id.
import { Link } from "@tanstack/react-router";
import { Alert } from "@doze/ui/components/app/alert";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { on, useOpRead } from "../api";
import { at, Made, Refused } from "../parts";
import { deletedRecord } from "./sentence";

export function DeletedStack({ name }: { name: string }) {
  const q = useOpRead<{ StackSummaries?: { StackName?: string; StackId?: string; DeletionTime?: number | string }[] }>(on.stack(name), "ListStacks", {});
  const back = (
    <Link to="/resources/$service/$second" params={{ service: "cloudformation", second: "stacks" }} search={{}} className="rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none">
      Back to stacks
    </Link>
  );
  if (q.isPending) return <Skeleton className="h-28 rounded-[14px]" data-testid="detail-loading" />;
  const record = q.data ? deletedRecord(q.data.StackSummaries ?? [], name) : null;
  return (
    <div className="grid gap-3" data-testid="stack-gone">
      {record ? (
        <Alert tone="info" title={`${name} was deleted${record.deleted ? ` on ${at(record.deleted)} UTC` : ""}`} action={back} testId="stack-deleted">
          CloudFormation no longer answers to its name; the record stays readable by its stack ID for 90 days: <span className="font-mono text-[13px] break-all">{record.id}</span>
        </Alert>
      ) : (
        <Alert tone="warn" title={`There is no stack named ${name}`} action={back} testId="stack-none">
          None is listed under that name, deleted or not{q.isError ? " (the deleted stacks could not be read, so this is a guess)" : ""}.
        </Alert>
      )}
      <Refused operation="ListStacks" error={q.error} />
      <Made ops={["ListStacks"]} />
    </div>
  );
}

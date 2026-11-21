// The Delete stack dialog (cloudformation__stack-delete): how many resources the stack owns, which go with it and which its template
// keeps (DeletionPolicy Retain), why the delete is allowed or why it would not be carried out, and what stays readable after.
import { Callout } from "@doze/ui/components/app/callout";
import { PlainConfirm } from "../../../kit";
import { type Retained, deleteRefusal, deletionSplit, nameList, physicalName, type Stack, type StackResource } from "./model";

const serviceOf = (type: string) => type.split("::")[1] ?? type;

export function DeleteStackDialog({ open, onOpenChange, stack, resources, retained, importers, pending, error, onConfirm }: { open: boolean; onOpenChange: (o: boolean) => void; stack: Stack; resources: StackResource[] | undefined; retained: Retained[]; importers: Record<string, readonly string[]> | undefined; pending: boolean; error: unknown; onConfirm: () => void }) {
  const { deleted, kept } = deletionSplit(resources ?? [], retained);
  const nested = Boolean(stack.ParentId);
  const refusal = nested ? `${stack.StackName} is a nested stack: delete its root stack instead.` : deleteRefusal(stack, importers ?? {});
  const total = deleted.length + kept.length;
  const exportsMade = (stack.Outputs ?? []).filter((o) => o.ExportName).length;
  return (
    <PlainConfirm
      open={open}
      onOpenChange={onOpenChange}
      title="Delete stack?"
      description={<>Deleting <b className="font-mono text-[13.5px] text-bright">{stack.StackName}</b> deletes the stack and the resources it owns, except the ones whose template says to keep them. Once deletion has begun it can't be stopped.</>}
      call="DeleteStack"
      pending={pending}
      disabled={refusal !== null}
      error={error}
      onConfirm={onConfirm}
      testId="delete-stack"
    >
      <div className="grid gap-3 text-[13.5px] leading-[21px] text-foreground" data-testid="delete-details">
        {resources === undefined ? <p className="text-muted-foreground">Reading what the stack owns…</p> : (
          <p data-testid="delete-owns">The stack owns <b className="text-bright">{total} {total === 1 ? "resource" : "resources"}</b>.{" "}
            {deleted.length > 0 ? <><b className="font-mono text-[13px] text-bright">{nameList(deleted.map((r) => r.LogicalResourceId))}</b> {deleted.length === 1 ? "is" : "are"} deleted with it.</> : "None is deleted with it."}</p>
        )}
        {kept.map((r) => (
          <Callout key={r.LogicalResourceId} tone="info" lead={`${r.LogicalResourceId} is kept.`} testId="delete-kept">Its template says DeletionPolicy: Retain, so <b className="font-mono text-[13px] text-bright">{physicalName(r)}</b> stays in {serviceOf(r.ResourceType)}{r.ResourceType === "AWS::S3::Bucket" ? " with its objects" : ""}.</Callout>
        ))}
        {refusal ? (
          <Callout tone="bad" lead="Delete would not be carried out." testId="delete-refusal">{refusal}</Callout>
        ) : (
          <p data-testid="delete-allowed">Termination protection is off, which is why this is allowed, and {exportsMade === 0 ? "the stack exports nothing for another stack to import" : `no other stack imports an export of ${stack.StackName}`}. Deleting runs in the background: the stack is DELETE_IN_PROGRESS first. The stack record stays readable by its stack ID as DELETE_COMPLETE; by name it disappears.</p>
        )}
      </div>
    </PlainConfirm>
  );
}

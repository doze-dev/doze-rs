// A secret marked for deletion: what that means (DeleteSecret's own words), and the way back, RestoreSecret. Shown on the secret's
// details in place of the Reveal and Schedule deletion that the server no longer offers for it.
import { Undo2Icon } from "lucide-react";
import { toast } from "sonner";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { useOp } from "../deep/api";
import { at, Made, Refused } from "../deep/parts";

/** The label the server gives a secret's mark (descriptor `resources/secrets.rs`); its value is the day DeleteSecret ran. */
const MARK = "Marked for deletion";

/** When the secret was marked for deletion, as the server wrote it; `null` for a secret that is not. */
export function markedAt(d: Pick<ResourceDetail, "fields">): string | null {
  return d.fields.find((f) => f.label === MARK)?.value ?? null;
}

export function SecretMarked({ d }: { d: ResourceDetail }) {
  const marked = markedAt(d);
  const restore = useOp(d, "RestoreSecret");
  if (!marked) return null;
  return (
    <div className="mb-6" data-testid="secret-marked">
      <Alert
        tone="warn"
        title={`${d.name} was marked for deletion on ${at(marked)}`}
        action={
          <Button size="sm" disabled={restore.isPending} onClick={() => restore.mutate({}, { onSuccess: () => toast.success("Deletion cancelled", { description: `${d.name} can be read again.` }) })} data-testid="restore-secret">
            <Undo2Icon /> {restore.isPending ? "Cancelling…" : "Cancel deletion"}
          </Button>
        }
      >
        Its value cannot be read, and Secrets Manager deletes it for good when the recovery window DeleteSecret named runs out (7 to 30 days; the console asks for 7). Its name stays taken until then. Cancelling removes the mark.
      </Alert>
      <Refused operation="RestoreSecret" error={restore.error} testId="restore-error" />
      <Made ops={["RestoreSecret"]} className="mt-2" />
    </div>
  );
}

// Create dashboard (cloudwatch__dashboard-create): a name, and an empty dashboard that opens on its page.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Field } from "../../../kit";
import { usePutDashboard } from "./api";
import { emptyBody } from "./dashboard-model";

const NAME = /^[A-Za-z0-9_-]{1,255}$/;

/** What is wrong with a dashboard name, or null. */
export function nameProblem(name: string, taken: readonly string[]): string | null {
  if (name === "") return "Enter a dashboard name.";
  if (!NAME.test(name)) return "A name is up to 255 characters: A-Z, a-z, 0-9, hyphen and underscore.";
  if (taken.includes(name)) return `A dashboard named ${name} already exists. Open it from the list, or choose another name.`;
  return null;
}

export function CreateDashboard({ names, onClose }: { names: readonly string[]; onClose: () => void }) {
  const navigate = useNavigate();
  const put = usePutDashboard();
  const [name, setName] = useState("");
  const [tried, setTried] = useState(false);
  const problem = nameProblem(name, names);
  const failed = put.error ? errorWords(put.error) : null;
  const submit = () => {
    setTried(true);
    if (problem) return;
    put.mutate({ name, body: emptyBody() }, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "cloudwatch", kind: "dashboards", name }, search: { done: "created" } }) });
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[560px]" data-testid="create-dashboard-dialog">
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">Create new dashboard</DialogTitle>
          <DialogDescription className="sr-only">Name the dashboard; it is made empty and opens on its page.</DialogDescription>
        </DialogHeader>
        <form className="grid gap-5" noValidate onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <Field label="Dashboard name" error={tried ? problem ?? undefined : undefined} help="Up to 255 characters: A-Z, a-z, 0-9, hyphen and underscore. Saving a dashboard under a name that already exists replaces that dashboard's contents.">
            {(id) => <Input id={id} value={name} onChange={(e) => setName(e.target.value)} autoFocus className="font-mono" autoComplete="off" spellCheck={false} data-testid="dashboard-name" />}
          </Field>
          {failed && <Alert tone="bad" title={`The dashboard was not made${failed.code ? `: ${failed.code}` : ""}`} testId="create-refused">{failed.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={put.isPending} data-testid="create-dashboard-submit">{put.isPending ? "Creating…" : "Create dashboard"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

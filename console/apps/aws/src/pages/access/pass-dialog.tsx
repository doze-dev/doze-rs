import { useState } from "react";
import { toast } from "sonner";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { useAttach } from "../../api/spaces";
import type { Refusal } from "../../api/types/Refusal";
import { CodeBlock } from "../card";
import { anyExplicit, draftPolicy, holderOf, policyName } from "./draft";

/** Make this pass: the drafted policy to copy, and, when the refused principal is a user or a role IAM holds, putting it on
 * them as an inline policy (PutUserPolicy or PutRolePolicy, as the console; Live shows it) after a confirmation. */
export function PassDialog({ open, onOpenChange, rows, what }: { open: boolean; onOpenChange: (o: boolean) => void; rows: Refusal[]; what: string }) {
  const doc = draftPolicy(rows);
  const holder = rows.map((r) => holderOf(r.principal)).find((h) => h !== null) ?? null;
  const actions = [...new Set(rows.map((r) => r.action))].sort();
  const [name, setName] = useState(() => policyName(actions));
  const attach = useAttach();
  const change = (o: boolean) => {
    if (!o) attach.reset();
    onOpenChange(o);
  };
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!doc || !holder || attach.isPending || name.trim() === "") return;
    attach.mutate(
      { kind: holder.kind, name: holder.name, policy_name: name.trim(), document: doc },
      {
        onSuccess: () => {
          change(false);
          toast.success(`${name.trim()} put on ${holder.kind} ${holder.name}`);
        },
      },
    );
  };
  const failed = attach.error ? errorWords(attach.error) : null;
  const op = holder?.kind === "role" ? "PutRolePolicy" : "PutUserPolicy";
  return (
    <Dialog open={open} onOpenChange={change}>
      <DialogContent className="sm:max-w-[720px]" data-testid="pass-dialog">
        <form onSubmit={submit} className="grid gap-5">
          <DialogHeader>
            <DialogTitle className="text-[21px] font-semibold">Make this pass</DialogTitle>
            <DialogDescription className="text-[14px] leading-[22px] text-foreground">
              The least policy that allows what IAM refused for <b className="font-mono font-semibold text-bright">{what}</b>: each action IAM checked, on the resource it checked it on.
            </DialogDescription>
          </DialogHeader>
          {doc ? <CodeBlock code={doc} what="Policy copied" testId="drafted-policy" /> : <p className="text-[13.5px] text-muted-foreground">Nothing to draft: IAM refused these with an explicit deny, which no allow can lift.</p>}
          {anyExplicit(rows) && doc && (
            <Alert tone="warn" title="An explicit deny is left out">
              One of these was refused by a statement that denies it. An allow cannot lift that; the deny itself has to change.
            </Alert>
          )}
          {doc && holder ? (
            <div className="grid gap-1.5">
              <Label htmlFor="policy-name">
                Put it on {holder.kind} <span className="font-mono">{holder.name}</span> as an inline policy named
              </Label>
              <Input id="policy-name" value={name} onChange={(e) => setName(e.target.value)} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="policy-name" />
              <p className="text-[12.5px] text-muted-foreground">
                Called as <span className="font-mono">{op}</span> by this console; Live shows it. An inline policy of the same name is replaced.
              </p>
            </div>
          ) : (
            doc && <p className="text-[13px] text-muted-foreground">IAM does not name a user or role for this caller, so copy the policy to where it belongs (a role in your template, a user in your setup).</p>
          )}
          {failed && (
            <Alert tone="bad" title={failed.code ? `${op} was refused: ${failed.code}` : `${op} failed`} testId="attach-error">
              {failed.message}
            </Alert>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => change(false)}>
              Close
            </Button>
            {doc && holder && (
              <Button type="submit" disabled={attach.isPending || name.trim() === ""} data-testid="attach-submit">
                {attach.isPending ? "Working…" : `Put on ${holder.kind}`}
              </Button>
            )}
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

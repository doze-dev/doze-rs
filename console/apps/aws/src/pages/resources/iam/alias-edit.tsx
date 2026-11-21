// The account alias (iam__alias-edit): one alias per account, 3 to 63 lowercase letters, digits and hyphens, edited in a dialog over the dashboard.
// IAM refuses a second alias, so saving a new one deletes the old one first, then makes the new one.
import { useState } from "react";
import { Input } from "@doze/ui/components/ui/input";
import { useAbout } from "../../../api/queries";
import { Field, SectionEdit, callKindOp } from "../../../kit";
import { USERS, useRead, useRefresh } from "./api";
import { aliasProblem } from "./model";

export function AliasDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const refresh = useRefresh();
  const account = useAbout().data?.account ?? "";
  const aliases = useRead<{ AccountAliases: string[] }>(USERS, "ListAccountAliases", {});
  const current = aliases.data?.AccountAliases[0] ?? null;
  const [typed, setTyped] = useState<string | null>(null);
  const value = typed ?? current ?? "";
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<unknown>(null);
  const problem = aliasProblem(value);
  const close = (o: boolean) => {
    if (!o) {
      setTyped(null);
      setFailed(null);
    }
    onOpenChange(o);
  };
  const save = async () => {
    if (problem || value === current) return;
    setPending(true);
    setFailed(null);
    try {
      if (current) await callKindOp(USERS, "DeleteAccountAlias", { AccountAlias: current });
      await callKindOp(USERS, "CreateAccountAlias", { AccountAlias: value });
      refresh();
      close(false);
    } catch (e) {
      setFailed(e);
    } finally {
      setPending(false);
    }
  };
  return (
    <SectionEdit
      open={open}
      onOpenChange={close}
      title="Account alias"
      description={<>Give account <span className="font-mono">{account}</span> a name that is easier to remember than its number.</>}
      request={{ service: "iam", operation: "CreateAccountAlias", input: { AccountAlias: value } }}
      onSave={() => void save()}
      pending={pending}
      error={failed}
      disabled={problem !== null || value === current}
      disabledWhy={problem ?? "Change the alias to save it."}
      testId="alias-edit-dialog"
    >
      <Field label="Account alias" error={typed !== null && problem ? problem : undefined} help={<><b>3 to 63 characters</b>: lowercase letters, digits and hyphens, with a letter or digit at each end and no two hyphens in a row. An account has one alias, so saving a new one overwrites {current ? <span className="font-mono">{current}</span> : "it"}: IAM refuses CreateAccountAlias while one exists, so the old alias is deleted first (DeleteAccountAlias). Aliases are not secrets: do not put anything sensitive in one.</>} testId="field-alias">
        {(id) => <Input id={id} value={value} onChange={(e) => { setTyped(e.target.value); setFailed(null); }} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="alias-input" />}
      </Field>
    </SectionEdit>
  );
}

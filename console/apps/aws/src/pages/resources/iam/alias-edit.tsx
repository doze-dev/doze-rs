// The account alias (iam__alias-edit): one alias per account, 3 to 63 lowercase letters, digits and hyphens. Saving a new alias deletes the old
// one first, then makes the new one.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { Field, FormPage, FormSection, type PreviewRequest, callKindOp } from "../../../kit";
import { USERS, useRead, useRefresh } from "./api";
import { aliasProblem } from "./model";
import { crumbs } from "./parts";

export function AliasEdit() {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const account = useAbout().data?.account ?? "";
  const aliases = useRead<{ AccountAliases: string[] }>(USERS, "ListAccountAliases", {});
  const current = aliases.data?.AccountAliases[0] ?? null;
  const [alias, setAlias] = useState<string | null>(null);
  const value = alias ?? current ?? "";
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string } | null>(null);
  const problem = aliasProblem(value);
  const back = () => void navigate({ to: "/resources/$service", params: { service: "iam" }, search: {} as never });
  const requests: PreviewRequest[] = [...(current && current !== value ? [{ service: "iam", operation: "DeleteAccountAlias", input: { AccountAlias: current } }] : []), { service: "iam", operation: "CreateAccountAlias", input: { AccountAlias: value } }];
  const submit = async () => {
    if (problem) return;
    setPending(true);
    setFailed(null);
    let at = "";
    try {
      if (current !== value) {
        for (const r of requests) {
          at = r.operation;
          await callKindOp(USERS, r.operation, r.input as object);
        }
      }
      refresh();
      back();
    } catch (e) {
      setFailed({ operation: at, code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  return (
    <FormPage
      title="Account alias"
      sub={<>Give account <span className="font-mono">{account}</span> a name that is easier to remember than its number.</>}
      crumbs={[...crumbs(), { label: "Account alias", to: "" }]}
      request={null}
      requests={requests}
      callTitle="The calls this will make"
      callNote="An account has one alias, so a new one replaces the old: IAM refuses CreateAccountAlias while one exists."
      refusal={failed ? { code: `${failed.operation}: ${failed.code}`, message: failed.message } : null}
      banner={failed ? <Alert tone="bad" title={`${failed.operation} was refused: ${failed.code}`} testId="alias-refused">{failed.message}</Alert> : undefined}
      submitLabel="Save changes"
      pending={pending}
      disabled={problem !== null || value === current}
      disabledWhy={problem ?? "Change the alias to save it."}
      onSubmit={() => void submit()}
      onCancel={back}
      testId="alias-edit-page"
    >
      <FormSection title="Account alias" testId="section-alias">
        <Field label="Account alias" error={alias !== null && problem ? problem : undefined} help={<><b>3 to 63 characters</b>: lowercase letters, digits and hyphens, with a letter or digit at each end and no two hyphens in a row. An account has one alias, so saving a new one overwrites {current ? <span className="font-mono">{current}</span> : "it"}. Aliases are not secrets: do not put anything sensitive in one.</>} testId="field-alias">
          {(id) => <Input id={id} value={value} onChange={(e) => { setAlias(e.target.value); setFailed(null); }} autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="alias-input" />}
        </Field>
      </FormSection>
    </FormPage>
  );
}

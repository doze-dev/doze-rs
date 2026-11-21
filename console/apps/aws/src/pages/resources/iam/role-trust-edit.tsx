// Edit trust policy (iam__role-trust-edit): the role's trust policy as JSON, checked as it is typed (every statement needs a Principal and none
// may name a Resource), and the UpdateAssumeRolePolicy it will make. The document is replaced whole.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { FormPage, FormSection, callKindOp } from "../../../kit";
import { ROLES, useRefresh } from "./api";
import { crumbs, KINDS, Note } from "./parts";
import { PolicyJson, policyErrors } from "./policy-json";
import { TrustNote } from "./trust-note";

export function RoleTrustEdit({ role, doc }: { role: string; doc: string }) {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const [text, setText] = useState(doc);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ code: string; message: string } | null>(null);
  const errors = policyErrors(text, "trust");
  const back = (done?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "roles", name: role }, search: { tab: "trust", ...(done ? { done } : {}) } });
  const submit = async () => {
    setPending(true);
    setFailed(null);
    try {
      await callKindOp(ROLES, "UpdateAssumeRolePolicy", { RoleName: role, PolicyDocument: text });
      refresh();
      back("trust");
    } catch (e) {
      setFailed({ code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  return (
    <FormPage
      title="Edit trust policy"
      sub={<>Change who can assume <span className="font-mono text-bright">{role}</span>. To add a principal, list it in the Principal element; to remove one, delete it.</>}
      crumbs={[...crumbs(KINDS.roles, role), { label: "Trust relationships", to: `/resources/iam/roles/${encodeURIComponent(role)}?tab=trust` }, { label: "Edit trust policy", to: "" }]}
      request={{ service: "iam", operation: "UpdateAssumeRolePolicy", input: { RoleName: role, PolicyDocument: text } }}
      callNote="UpdateAssumeRolePolicy replaces the policy that grants an IAM entity permission to assume the role, the role's trust policy."
      refusal={failed ? { code: failed.code, message: failed.message } : null}
      banner={failed ? <Alert tone="bad" title={`UpdateAssumeRolePolicy was refused: ${failed.code}`} testId="trust-refused">{failed.message}</Alert> : undefined}
      submitLabel="Update policy"
      pending={pending}
      disabled={errors.length > 0 || text.trim() === ""}
      disabledWhy="Fix the errors in the policy first."
      onSubmit={() => void submit()}
      onCancel={() => back()}
      testId="trust-edit"
    >
      <FormSection title="Trust policy" description="Edit the JSON. doze-aws checks the document when you choose Update policy: every statement needs a Principal and none may name a Resource." testId="section-trust">
        <PolicyJson value={text} onChange={(v) => { setText(v); setFailed(null); }} kind="trust" label="Trust policy" rows={18} />
        <Note>Principals in another account are only half of a cross-account trust: that account&apos;s administrator must also allow its users to call <span className="font-mono">sts:AssumeRole</span> on this role&apos;s ARN. <span className="dim">doze-aws does not check the other account.</span></Note>
        <TrustNote />
      </FormSection>
    </FormPage>
  );
}

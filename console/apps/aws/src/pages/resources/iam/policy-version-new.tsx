// Create policy version (a policy's Actions menu and Policy versions tab): the default version's document to edit, and whether the new version
// becomes the default. CreatePolicyVersion keeps up to five versions.
import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { FormPage, FormSection, callKindOp } from "../../../kit";
import { POLICIES, useRefresh } from "./api";
import { crumbs, KINDS } from "./parts";
import { PolicyJson, policyErrors } from "./policy-json";

export function PolicyVersionNew({ arn, label, doc }: { arn: string; label: string; doc: string }) {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const [text, setText] = useState(doc);
  const [asDefault, setAsDefault] = useState(true);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ code: string; message: string } | null>(null);
  const errors = policyErrors(text, "identity");
  const back = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "policies", name: arn }, search: { tab: "versions" } });
  const submit = async () => {
    setPending(true);
    setFailed(null);
    try {
      await callKindOp(POLICIES, "CreatePolicyVersion", { PolicyArn: arn, PolicyDocument: text, SetAsDefault: asDefault });
      refresh();
      back();
    } catch (e) {
      setFailed({ code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  return (
    <FormPage
      title="Create policy version"
      sub={<>A new version of <span className="font-mono text-bright">{label}</span>, starting from its default version. A customer managed policy keeps up to five.</>}
      crumbs={[...crumbs(KINDS.policies, label), { label: "Create policy version", to: "" }]}
      request={{ service: "iam", operation: "CreatePolicyVersion", input: { PolicyArn: arn, PolicyDocument: text, SetAsDefault: asDefault } }}
      refusal={failed}
      banner={failed ? <Alert tone="bad" title={`CreatePolicyVersion was refused: ${failed.code}`} testId="version-refused">{failed.message}</Alert> : undefined}
      submitLabel="Create version"
      pending={pending}
      disabled={errors.length > 0}
      disabledWhy="Fix the errors in the policy first."
      onSubmit={() => void submit()}
      onCancel={back}
      testId="version-new-page"
    >
      <FormSection title="Policy document" testId="section-document">
        <PolicyJson value={text} onChange={(v) => { setText(v); setFailed(null); }} kind="identity" label="JSON" rows={16} />
        <Label className="items-start gap-3 font-normal leading-5"><Checkbox checked={asDefault} onCheckedChange={(v) => setAsDefault(v === true)} className="mt-0.5" data-testid="version-as-default" /><span className="grid gap-0.5"><span className="font-medium text-bright">Set as the default version</span><span className="text-[13px] text-muted-foreground">The default version is the one that applies to everything the policy is attached to.</span></span></Label>
      </FormSection>
    </FormPage>
  );
}

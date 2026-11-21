// Create access key (iam__access-key-retrieve, the last of three steps): the use case, the description tag, then the one place the secret is
// shown. CreateAccessKey answers the secret once; the page holds it in memory only, so leaving it loses the secret for good.
import { useNavigate } from "@tanstack/react-router";
import { DownloadIcon, EyeIcon, EyeOffIcon } from "lucide-react";
import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, type PreviewRequest, callKindOp } from "../../../kit";
import { USERS, useRefresh } from "./api";
import { count } from "./model";
import { CardChoice, crumbs, KINDS, Note } from "./parts";
import { useUserKeys } from "./user-data";

const STEPS = [{ id: "practices", label: "Access key best practices & alternatives" }, { id: "tag", label: "Set description tag" }, { id: "retrieve", label: "Retrieve access keys" }];
const USES = [
  { value: "cli", title: "Command Line Interface (CLI)", description: "You plan to use this access key to enable the AWS CLI to access your account." },
  { value: "code", title: "Local code", description: "You plan to use this access key to enable application code in a local development environment to access your account." },
  { value: "other", title: "Other", description: "Your use case is not listed here." },
];

type Made = { id: string; secret: string; tag: string };

export function AccessKeyNew({ user }: { user: string }) {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const keys = useUserKeys([user]).byUser.get(user)?.keys ?? [];
  const [step, setStep] = useState(0);
  const [use, setUse] = useState("cli");
  const [description, setDescription] = useState("");
  const [made, setMade] = useState<Made | null>(null);
  const [shown, setShown] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string } | null>(null);
  const back = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "iam", kind: "users", name: user }, search: { tab: "security" } });
  const create: PreviewRequest = { service: "iam", operation: "CreateAccessKey", input: { UserName: user } };
  const tag = (id: string): PreviewRequest => ({ service: "iam", operation: "TagUser", input: { UserName: user, Tags: [{ Key: id, Value: description }] } });
  const requests = made ? [create, ...(made.tag ? [tag(made.id)] : [])] : [create, ...(description.trim() ? [tag("(the new key's ID)")] : [])];
  const submit = async () => {
    setPending(true);
    setFailed(null);
    let at = "CreateAccessKey";
    try {
      const out = (await callKindOp(USERS, "CreateAccessKey", { UserName: user })) as { AccessKey: { AccessKeyId: string; SecretAccessKey: string } };
      const id = out.AccessKey.AccessKeyId;
      const text = description.trim();
      if (text) {
        at = "TagUser";
        await callKindOp(USERS, "TagUser", { UserName: user, Tags: [{ Key: id, Value: text }] });
      }
      setMade({ id, secret: out.AccessKey.SecretAccessKey, tag: text });
      refresh();
      setStep(2);
    } catch (e) {
      setFailed({ operation: at, code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  const download = () => {
    if (!made) return;
    const url = URL.createObjectURL(new Blob([`User name,Access key ID,Secret access key\n${user},${made.id},${made.secret}\n`], { type: "text/csv" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = `${user}_accessKeys.csv`;
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const last = step === 2;
  return (
    <FormPage
      title={last ? "Retrieve access keys" : step === 0 ? "Access key best practices & alternatives" : "Set description tag"}
      sub={last ? <>This is your only time to view or download the newly created access key. <span className="text-muted-foreground">You cannot recover it later.</span></> : step === 0 ? "Avoid using long-term credentials like access keys to improve your security. Consider the use case below and its alternatives." : "Optional: a description to tell this key from another. It is stored as a tag on the user."}
      crumbs={[...crumbs(KINDS.users, user), { label: "Security credentials", to: `/resources/iam/users/${encodeURIComponent(user)}?tab=security` }, { label: last ? "Retrieve access keys" : "Create access key", to: "" }]}
      request={null}
      requests={requests}
      callTitle={last ? "The calls that were made" : "The call this will make"}
      callNote={last ? <>{user} now has {count(keys.length, "access key")}. A new key is Active by default.</> : "CreateAccessKey answers the secret once. It is not stored where the console can show it again."}
      refusal={failed ? { code: `${failed.operation}: ${failed.code}`, message: failed.message } : null}
      wizard={{ steps: STEPS, current: step, furthest: made ? 2 : step, onStep: (i) => { if (!made && i < step) setStep(i); }, onBack: () => setStep(step - 1), onNext: () => (step === 1 ? void submit() : setStep(step + 1)), nextLabel: step === 1 ? "Create access key" : "Next" }}
      hideFooter={last}
      backLabel="Previous"
      submitLabel="Done"
      pending={pending}
      onSubmit={() => (last ? back() : void submit())}
      onCancel={back}
      banner={last ? <Alert tone="good" title="The key pair is active by default, and the user can use it right away." testId="key-made" /> : failed ? <Alert tone="bad" title={failed.operation === "CreateAccessKey" ? "The access key was not created." : "The access key was created, but it could not be tagged."} testId="key-refused-banner">{failed.code}: {failed.message}</Alert> : undefined}
      testId="access-key-new"
    >
      {step === 0 && (
        <FormSection title="Use case" description="IAM's CreateAccessKey takes no use case: this is advice, and nothing here is sent.">
          <CardChoice name="use" value={use} onChange={setUse} options={USES} className="sm:grid-cols-1" />
          <Note>Alternatives to a long-term key: a role your workload assumes for short-lived credentials (<span className="font-mono">sts AssumeRole</span>), or an environment where the SDK finds credentials for you. Access keys are the right tool for a local app or script, which is what doze-aws is for.</Note>
        </FormSection>
      )}
      {step === 1 && (
        <FormSection title="Description tag" optional description={<>Stored as a tag on <span className="font-mono">{user}</span>: the key is the access key ID, the value is the description.</>}>
          <Field label="Description tag value" optional help={<><b>At most 256 characters</b>. Letters, digits, spaces and + - = . _ : / @ are allowed.</>}>
            {(id) => <Input id={id} value={description} maxLength={256} onChange={(e) => setDescription(e.target.value)} placeholder="dev-laptop" autoComplete="off" className="bg-card" data-testid="key-description" />}
          </Field>
        </FormSection>
      )}
      {last && made && (
        <FormSection title="Access key" description="Save both values in a secure place before you choose Done." testId="secret-section">
          <dl className="grid grid-cols-[minmax(8rem,12rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
            <div className="contents"><dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">Access key ID</dt><dd className="border-b py-3 font-mono text-[13.5px] text-bright" data-testid="made-id">{made.id}</dd></div>
            <div className="contents">
              <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">Secret access key</dt>
              <dd className="flex flex-wrap items-center gap-3 border-b py-2.5 font-mono text-[13.5px] break-all text-bright" data-testid="made-secret">
                <span>{shown ? made.secret : "*".repeat(40)}</span>
                <Button type="button" variant="outline" size="sm" className="bg-card" onClick={() => setShown(!shown)} data-testid="secret-toggle">{shown ? <EyeOffIcon /> : <EyeIcon />} {shown ? "Hide" : "Show"}</Button>
                {shown && <CopyButton text={made.secret} label="Copy" what="Secret copied" />}
              </dd>
            </div>
            {made.tag && <div className="contents"><dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">Description tag</dt><dd className="border-b py-3 text-[13.5px] text-bright"><span className="font-mono">{made.tag}</span><span className="mt-0.5 block text-muted-foreground">Stored as a tag on {user}: the key is the access key ID, the value is the description.</span></dd></div>}
          </dl>
          <div className="flex flex-wrap items-center justify-end gap-3 pt-2">
            <Button type="button" variant="outline" className="bg-card" onClick={download} data-testid="key-download"><DownloadIcon /> Download .csv file</Button>
            <Button type="button" onClick={back} data-testid="key-done">Done</Button>
          </div>
        </FormSection>
      )}
    </FormPage>
  );
}

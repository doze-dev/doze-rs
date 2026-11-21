// A stream's Data stream sharing tab (kinesis__stream-sharing, stream-sharing-none) and the Visual policy editor (stream-sharing-create): the resource-based
// policy the stream carries. doze-aws stores it and answers GetResourcePolicy with it; it evaluates nothing from it (crates/aws-kinesis/src/settings.rs).
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { PlusIcon, ShareIcon, TrashIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceDetail } from "../../../api/resources";
import { Field, FormPage, FormSection, PlainConfirm, PolicyEditor, RuleHelp } from "../../../kit";
import { callOp } from "../deep/api";
import { on, useStreamWrite } from "./api";
import { READ_ACTIONS, SHARE_ACTIONS, noPolicy, principalProblem, sharePolicy, statementsOf, type Stream } from "./model";

/** The stream's policy as the service gives it (`{}` when it has none). */
export function usePolicy(name: string) {
  return useQuery({ queryKey: ["resources", "deep", "kinesis", "streams", name, "policy"], queryFn: () => callOp<{ Policy?: string }>(on(name), "GetResourcePolicy", {}), retry: false });
}

export function SharingTab({ s, create }: { s: Stream; create: () => void }) {
  const q = usePolicy(s.name);
  const del = useStreamWrite("DeleteResourcePolicy");
  const [asking, setAsking] = useState(false);
  if (q.error) return <Alert tone="bad" title="The sharing policy could not be read" testId="sharing-error">{String((q.error as Error).message)}</Alert>;
  if (!q.data) return <p className="text-[13.5px] text-muted-foreground">Reading the policy…</p>;
  if (noPolicy(q.data.Policy)) {
    return (
      <EmptyTab icon={ShareIcon} title="No resource policy" testId="sharing-empty" action={<Button type="button" variant="outline" className="bg-card" onClick={create} data-testid="sharing-create"><PlusIcon /> Create sharing policy</Button>}>
        No resource policy is attached, so no other account or principal has been given access to this stream.
      </EmptyTab>
    );
  }
  const policy = q.data.Policy ?? "";
  const statements = statementsOf(policy) ?? [];
  let shown = policy;
  try { shown = JSON.stringify(JSON.parse(policy), null, 2); } catch { /* shown as it is */ }
  return (
    <div className="grid gap-5" data-testid="sharing-policy">
      <section className="rounded-[14px] border bg-card p-6 lg:p-7">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div><h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Data stream sharing</h2><p className="mt-1 text-[13.5px] text-muted-foreground">A resource-based policy on the stream says which identities may use it and which actions they may call.</p></div>
          <div className="flex gap-2"><Button type="button" variant="outline" className="bg-card" onClick={create} data-testid="sharing-replace">Replace policy</Button><Button type="button" variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => { del.reset(); setAsking(true); }} data-testid="sharing-delete"><TrashIcon /> Delete policy</Button></div>
        </div>
        <ol className="mt-4 grid gap-2">
          {statements.map((st, i) => (
            <li key={i} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 rounded-lg border bg-well px-3.5 py-2.5 text-[13.5px]" data-testid="sharing-statement">
              <Badge variant="outline" className={st.effect === "Allow" ? "border-good/40 bg-good/10 text-good" : "border-bad/40 bg-bad/10 text-bad"}>{st.effect}</Badge>
              <span className="min-w-0 flex-1 font-mono text-[12.5px] break-all">{st.principals.join(", ") || "no principal"} may call {st.actions.join(", ")}</span>
              {st.sid && <span className="text-[12px] text-muted-foreground">{st.sid}</span>}
            </li>
          ))}
        </ol>
        <div className="mt-4"><CodeBlock text={shown} wrap={false} max="max-h-96" testId="sharing-json" /></div>
        <RuleHelp className="mt-4">The policy is stored and GetResourcePolicy reads it back. <span className="text-muted-foreground">doze-aws's IAM does not evaluate a stream's resource policy, so it neither grants nor denies a call here.</span></RuleHelp>
      </section>
      <PlainConfirm open={asking} onOpenChange={setAsking} title="Delete sharing policy" call="DeleteResourcePolicy" pending={del.isPending} error={del.error} testId="sharing-delete-dialog" onConfirm={() => del.mutate({ stream: s.name, input: {} }, { onSuccess: () => setAsking(false) })}
        description={<>DeleteResourcePolicy removes the policy from <b className="font-mono font-semibold text-bright">{s.name}</b>. No other account or principal keeps the access it gave.</>} />
    </div>
  );
}

/** Visual policy editor: a principal and the actions shared, written as the guide's read-access policy; the JSON may be written directly. */
export function SharingCreate({ s }: { s: Stream }) {
  const navigate = useNavigate();
  const [principal, setPrincipal] = useState("");
  const [actions, setActions] = useState<string[]>([...READ_ACTIONS]);
  const [json, setJson] = useState<string | null>(null);
  const [tried, setTried] = useState(false);
  const put = useStreamWrite("PutResourcePolicy");
  const written = useMemo(() => sharePolicy(principal.trim() || "arn:aws:iam::<account>:role/<role>", actions, s.arn), [principal, actions, s.arn]);
  const policy = json ?? written;
  const role = /^arn:aws:iam::\d{12}:role\/([\w+=,.@/-]+)$/.exec(principal.trim())?.[1]?.split("/").pop();
  const known = useResourceDetail("iam", "roles", role ?? "", role !== undefined);
  const problem = principalProblem(principal) ?? (actions.length === 0 ? "Choose at least one action." : null);
  const refused = put.error instanceof ApiFailure ? { code: put.error.code, message: put.error.message } : null;
  const back = (done?: string) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "kinesis", kind: "streams", name: s.name }, search: { tab: "sharing", ...(done ? { done } : {}) } });
  return (
    <FormPage
      title="Visual policy editor"
      sub={<>Specify the principal and the exact actions you are sharing access to. <span className="text-muted-foreground">You can also write the JSON policy directly.</span></>}
      crumbs={[{ label: "Resources", to: "/resources" }, { label: "Messaging and events", to: "/resources" }, { label: "Kinesis", to: "/resources/kinesis" }, { label: s.name, to: `/resources/kinesis/streams/${encodeURIComponent(s.name)}` }, { label: "Data stream sharing", to: `/resources/kinesis/streams/${encodeURIComponent(s.name)}?tab=sharing` }]}
      request={{ service: "kinesis", operation: "PutResourcePolicy", input: { ResourceARN: s.arn, Policy: policy } }}
      callNote="The policy goes in as a JSON string. GetResourcePolicy reads it back."
      refusal={refused}
      banner={refused ? <Alert tone="bad" title="The policy was not saved." testId="sharing-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Create policy"
      pending={put.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => back()}
      onSubmit={() => { setTried(true); if (problem && json === null) return; put.mutate({ stream: s.name, input: { Policy: policy } }, { onSuccess: () => back("sharing") }); }}
      testId="sharing-create-page"
    >
      <FormSection title="Principal">
        <Field label="Account ID, IAM user ARN or IAM role ARN" error={tried && json === null ? (principalProblem(principal) ?? undefined) : undefined} hint={role && known.data ? `The role ${role} already exists in this account.` : role && known.isError ? `No role named ${role} exists in this account yet; the policy is kept as written.` : undefined}>
          {(id) => <Input id={id} value={principal} onChange={(e) => { setPrincipal(e.target.value); setJson(null); }} placeholder="arn:aws:iam::123456789012:role/report-job" autoComplete="off" spellCheck={false} className="font-mono" data-testid="share-principal" />}
        </Field>
      </FormSection>
      <FormSection title="Actions" description="Kinesis Data Streams actions that can be shared on a data stream.">
        <div className="grid gap-4">
          {SHARE_ACTIONS.map((a) => (
            <div key={a} className="flex items-start gap-3">
              <Checkbox id={`a-${a}`} checked={actions.includes(a)} onCheckedChange={(v) => { setActions((x) => (v === true ? SHARE_ACTIONS.filter((y) => x.includes(y) || y === a) : x.filter((y) => y !== a))); setJson(null); }} className="mt-0.5" data-testid={`share-${a.split(":")[1]}`} />
              <div className="grid gap-0.5"><Label htmlFor={`a-${a}`} className="font-mono text-[13.5px] font-medium text-bright">{a}</Label><span className="text-[13px] text-muted-foreground">Level of access: Data stream</span></div>
            </div>
          ))}
        </div>
      </FormSection>
      <FormSection title="Policy" description="The policy as JSON, in the shape of the guide's read-access example.">
        <PolicyEditor value={policy} onChange={setJson} kind="resource" label="Policy" rows={18} testId="share-policy" />
        <RuleHelp>Creating a policy replaces any policy already on the stream, so keep every statement you still need.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

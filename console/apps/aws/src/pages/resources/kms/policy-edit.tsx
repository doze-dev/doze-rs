// Edit key policy (kms__key-policy-edit): the policy as JSON, a statement builder beside it that says what the chosen statement
// grants, and the PutKeyPolicy call. doze-aws checks that the policy is a JSON object and stores it as written.
import { PlusIcon, Trash2Icon } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { callKindOp, FormPage, FormSection, PolicyEditor } from "../../../kit";
import { Note } from "../cfg-parts";
import { KEYS, useKeyPolicy } from "./api";
import { parsePolicy, principalWords, type Statement } from "./model";
import { refusalOf } from "./schedule";
import { StateDot } from "./parts";
import { stateWords } from "./model";

const list = (v: unknown): string[] => (Array.isArray(v) ? v.map(String) : typeof v === "string" ? [v] : []);
const principalsOf = (s: Statement): string[] => (typeof s.Principal === "string" ? [s.Principal] : list((s.Principal as { AWS?: unknown } | undefined)?.AWS));

export function PolicyEdit({ d, id, state, back }: { d: ResourceDetail; id: string; state: string; back: () => void }) {
  const qc = useQueryClient();
  const q = useKeyPolicy(id);
  const [text, setText] = useState<string | null>(null);
  const [at, setAt] = useState(0);
  const [pending, setPending] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  useEffect(() => {
    if (q.data && text === null) {
      try { setText(JSON.stringify(JSON.parse(q.data.Policy), null, 2)); } catch { setText(q.data.Policy); }
    }
  }, [q.data, text]);
  const doc = useMemo(() => parsePolicy(text ?? ""), [text]);
  const statements = doc?.Statement ?? [];
  const chosen = statements[Math.min(at, statements.length - 1)];
  const edit = (next: unknown) => setText(JSON.stringify(next, null, 2));
  const add = () => {
    const base = doc ?? { Version: "2012-10-17", Statement: [] };
    edit({ ...base, Statement: [...(base.Statement ?? []), { Sid: `Statement${(base.Statement?.length ?? 0) + 1}`, Effect: "Allow", Principal: { AWS: "" }, Action: "kms:Describe*", Resource: "*" }] });
    setAt(statements.length);
  };
  const remove = () => {
    if (!doc || !chosen) return;
    edit({ ...doc, Statement: statements.filter((s) => s !== chosen) });
    setAt(0);
  };
  const save = async () => {
    if (text === null || !doc) return;
    setPending(true);
    setRefused(null);
    try {
      await callKindOp(KEYS, "PutKeyPolicy", { KeyId: id, PolicyName: "default", Policy: text });
      await qc.invalidateQueries({ queryKey: ["resources", "kindop", "kms", "keys"] });
      back();
    } catch (e) {
      setRefused(refusalOf(e));
    } finally {
      setPending(false);
    }
  };
  const builder = (
    <section className="rounded-[14px] border bg-card p-6" aria-label="Statement builder" data-testid="statement-builder">
      <h2 className="text-[17px] font-semibold tracking-[-0.01em] text-bright">Statement builder</h2>
      {statements.length === 0 ? <p className="mt-2 text-[13px] text-muted-foreground">The policy has no statements yet. Add one, or write it in the JSON.</p> : (
        <>
          <div className="mt-3">
            <Select value={String(Math.min(at, statements.length - 1))} onValueChange={(v) => setAt(Number(v))}>
              <SelectTrigger className="w-full bg-card font-mono text-[13px]" aria-label="Selected statement" data-testid="statement-select"><SelectValue /></SelectTrigger>
              <SelectContent>{statements.map((s, i) => <SelectItem key={i} value={String(i)} className="font-mono">{s.Effect ?? "?"} {s.Sid ?? `statement ${i + 1}`}</SelectItem>)}</SelectContent>
            </Select>
          </div>
          {chosen && (
            <div className="mt-4 grid gap-4 text-[13.5px]" data-testid="statement-details">
              <div><h3 className="mb-1.5 font-medium text-bright">Principals</h3><div className="flex flex-wrap gap-1.5">{principalsOf(chosen).length === 0 ? <span className="text-muted-foreground">None</span> : principalsOf(chosen).map((p) => <span key={p} className="rounded-full border bg-well px-2.5 py-0.5 font-mono text-[12.5px]">{principalWords(p).name}</span>)}</div></div>
              <div><h3 className="mb-1.5 font-medium text-bright">Actions</h3><ul className="grid gap-0.5 font-mono text-[12.5px]">{list(chosen.Action).map((a) => <li key={a}>{a}</li>)}</ul></div>
              <div><h3 className="mb-1.5 font-medium text-bright">Conditions</h3>{chosen.Condition ? <pre className="font-mono text-[12.5px] whitespace-pre-wrap">{JSON.stringify(chosen.Condition)}</pre> : <span className="text-muted-foreground">None</span>}</div>
            </div>
          )}
        </>
      )}
    </section>
  );
  return (
    <FormPage
      title={d.name}
      mono
      titleExtra={<StateDot state={state} words={stateWords(state)} />}
      sub="Key policy, policy view. Edit the JSON, or select a statement and use the statement builder."
      crumbs={[{ label: "Resources", to: "/resources" }, { label: d.group, to: `/resources/${d.service}` }, { label: d.service_title, to: `/resources/${d.service}` }, { label: "Keys", to: "/resources/kms/keys" }, { label: d.name, to: `/resources/kms/keys/${encodeURIComponent(d.id)}` }, { label: "Edit key policy", to: "" }]}
      request={text !== null ? { service: "kms", operation: "PutKeyPolicy", input: { KeyId: id, PolicyName: "default", Policy: "{ ... }" } } : null}
      showInput
      refusal={refused}
      aside={builder}
      submitLabel="Save changes"
      pending={pending}
      disabled={!doc}
      disabledWhy="The policy must be a JSON object."
      onSubmit={() => void save()}
      onCancel={back}
      banner={refused ? <Alert tone="bad" title="The policy was not saved." testId="policy-refused">{refused.code}: {refused.message}</Alert> : undefined}
      testId="key-policy-edit"
    >
      <FormSection title="Key policy" actions={<><Button type="button" variant="outline" className="bg-card" onClick={add} data-testid="statement-add"><PlusIcon /> Add new statement</Button><Button type="button" variant="outline" className="bg-card" onClick={remove} disabled={!chosen} data-testid="statement-remove"><Trash2Icon /> Remove</Button></>} testId="section-policy">
        {text === null ? <p className="text-[13px] text-muted-foreground">Reading the policy…</p> : <PolicyEditor kind="resource" value={text} onChange={setText} label="Key policy" rows={26} testId="policy-editor" />}
      </FormSection>
      <Note testId="policy-note">doze-aws stores the policy as written. With IAM in soft or enforce mode it checks the policy on every request that names this key, and <b>a policy that names nobody locks everyone out</b>, the account root and PutKeyPolicy included. AWS refuses a policy that would stop the caller using PutKeyPolicy again unless the lockout safety check is bypassed; doze-aws checks only that the policy is valid JSON.</Note>
    </FormPage>
  );
}

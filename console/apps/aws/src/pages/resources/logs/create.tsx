// Create log group (logs__log-group-create): a page, with the call it will make beside it; logs__log-group-created is where it lands.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, RadioCards, RuleHelp, useKindOp } from "../../../kit";
import { type CreateForm, createInput, emptyCreate, groupNameProblem, kmsProblem } from "./model";

const GROUPS = { service: "logs", kind: "groups" };

/** `/aws/lambda/<function>`, `/larder/<app>`: how the groups already here are named. */
export function namingOf(names: readonly string[]): string[] {
  const seen: string[] = [];
  for (const n of names) {
    const pattern = n.startsWith("/aws/lambda/") ? "/aws/lambda/<function>" : n.startsWith("/") && n.split("/").length > 2 ? `/${n.split("/")[1]}/<${n.split("/")[1] === "aws" ? "service" : "app"}>` : null;
    if (pattern && !seen.includes(pattern)) seen.push(pattern);
  }
  return seen.slice(0, 3);
}

export function LogsCreate() {
  const navigate = useNavigate();
  const [f, setF] = useState<CreateForm>(emptyCreate);
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<CreateForm>) => setF((x) => ({ ...x, ...patch }));
  const make = useKindOp<Record<string, never>>(GROUPS, "CreateLogGroup");
  const list = useResourceList("logs", "groups");
  const first = list.data?.pages[0];
  const names = useMemo(() => (list.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name), [list.data]);
  const problems = { name: groupNameProblem(f.name), kms: kmsProblem(f.kms) };
  const bad = Object.values(problems).find(Boolean) ?? null;
  const input = useMemo(() => createInput(f), [f]);
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : make.error ? { code: "Error", message: String((make.error as Error).message) } : null;
  const naming = namingOf(names);
  const taken = names.includes(f.name);
  const submit = () => {
    setTried(true);
    if (bad) return;
    make.mutate(input, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "logs", kind: "groups", name: f.name }, search: { done: "created" } }) });
  };
  return (
    <FormPage
      title="Create log group"
      sub="Name the log group. You can't rename an existing log group; to use a different name, create a new one."
      crumbs={[{ label: "Resources", to: "/resources" }, ...(first ? [{ label: first.group, to: "/resources" }, { label: first.service_title, to: "/resources/logs" }] : []), { label: "Log groups", to: "/resources/logs/groups" }]}
      request={f.name ? { service: "logs", operation: "CreateLogGroup", input } : null}
      refusal={refused}
      callNote="Left at their defaults, the KMS key and the deletion protection are left out."
      banner={refused ? <Alert tone="bad" title="Log group not created." testId="create-refused">{refused.code}: {refused.message}</Alert> : undefined}
      submitLabel="Create log group"
      pending={make.isPending}
      disabled={tried && bad !== null}
      disabledWhy={bad ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "logs", second: "groups" }, search: {} })}
      onSubmit={submit}
      testId="log-group-create"
    >
      <FormSection title="Log group details">
        <Field label="Log group name" error={tried ? (problems.name ?? undefined) : undefined} refusal={refused?.code === "ResourceAlreadyExistsException" ? { why: refused.message, suggestion: "Choose another name." } : null} help={<>1 to <b>512</b> characters: letters, digits and <span className="font-mono">_ - / . #</span>, not starting with <span className="font-mono">aws/</span>. Unique within the Region.</>} hint={taken ? `${f.name} already exists in this Region.` : naming.length > 0 ? `The other groups here are named ${naming.join(" or ")}.` : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="/larder/notifications" autoComplete="off" spellCheck={false} className="font-mono" data-testid="group-name" />}
        </Field>
        <div className="grid gap-1.5">
          <Label>Log class</Label>
          <RadioCards name="class" label="Log class" value={f.cls} onChange={(v) => set({ cls: v })} options={[{ value: "STANDARD", title: "Standard", body: "Supports all CloudWatch Logs features." }, { value: "INFREQUENT_ACCESS", title: "Essentials (formerly Infrequent Access)", body: "Supports a subset of CloudWatch Logs features, with a lower ingestion price." }]} />
          <RuleHelp>The class <b>can't be changed after the group is created</b>. On AWS, Essentials does not support metric filters, subscription filters, GetLogEvents or FilterLogEvents. <span className="text-muted-foreground">Stored, not applied locally.</span></RuleHelp>
        </div>
        <Field label="KMS key ARN" optional error={tried ? (problems.kms ?? undefined) : undefined} help={<>Encrypts newly ingested data with your key. Symmetric keys only. <span className="text-muted-foreground">Stored, not applied locally.</span></>}>
          {(id) => <Input id={id} value={f.kms} onChange={(e) => set({ kms: e.target.value })} placeholder="arn:aws:kms:us-east-1:123456789012:key/…" autoComplete="off" spellCheck={false} className="font-mono" data-testid="group-kms" />}
        </Field>
        <div className="grid gap-1.5">
          <Label>Deletion protection</Label>
          <div className="flex items-start gap-3">
            <Checkbox id="protect" checked={f.protect} onCheckedChange={(v) => set({ protect: v === true })} className="mt-0.5" data-testid="group-protect" />
            <div className="grid gap-0.5">
              <Label htmlFor="protect" className="font-normal text-bright">Enabled deletion protection</Label>
              <p className="text-[13px] text-muted-foreground">A protected log group refuses DeleteLogGroup until the protection is turned off.</p>
            </div>
          </div>
        </div>
      </FormSection>
      <p className="max-w-[44rem] text-[13px] leading-5 text-muted-foreground" data-testid="create-teaching">
        A new log group has no retention policy. On AWS its events then never expire; doze-aws drops those older than its <span className="font-mono">--logs-retention-days</span> (7 by default) until you set a retention from the Retention column of the Log groups list.
      </p>
    </FormPage>
  );
}

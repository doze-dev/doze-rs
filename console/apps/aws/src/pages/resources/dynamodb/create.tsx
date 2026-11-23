// Create table (dynamodb__table-create, table-create-custom, table-create-exists): a page, not a dialog. The name and the primary
// key, the choice of Default or Customize settings, and under Customize the capacity mode, encryption at rest and tags, with the
// CreateTable it will make at the right. A name that is taken is the service's refusal, said on the page, the field and the call.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceList } from "../../../api/resources";
import { Field, FieldRefusal, FormPage, FormSection, KeyPicker, LocalStatus, type PreviewRequest, TAG_RULES, useKindOp } from "../../../kit";
import { CardChoice, KeyRow, RowChoice } from "./controls";
import { blankForm, type CreateForm, createInput, problems, whyOff } from "./create-model";
import { crumbs, KIND, useListNav } from "./parts";

/** `a, b and c`, at most `most` of them and how many more. */
const some = (names: string[], most = 4): string => {
  const shown = names.slice(0, most);
  const head = shown.length > 1 ? `${shown.slice(0, -1).join(", ")} and ${shown.at(-1)}` : (shown[0] ?? "");
  return names.length > most ? `${shown.join(", ")} and ${names.length - most} more` : head;
};

export function TableCreate() {
  const go = useListNav();
  const existing = useResourceList("dynamodb", "tables");
  const names = (existing.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name);
  const [f, setF] = useState<CreateForm>(blankForm);
  const [tried, setTried] = useState(false);
  const make = useKindOp<unknown>(KIND, "CreateTable");
  const set = (patch: Partial<CreateForm>) => {
    setF((x) => ({ ...x, ...patch }));
    make.reset();
  };
  const input = createInput(f);
  const request: PreviewRequest | null = input ? { service: "dynamodb", operation: "CreateTable", input } : null;
  const p = problems(f);
  const off = whyOff(f);
  const refused = make.error;
  const code = refused instanceof ApiFailure ? refused.code : "";
  const said = refused ? errorWords(refused) : null;
  const taken = code === "ResourceInUseException";
  const submit = () => {
    setTried(true);
    if (off || !input) return;
    make.mutate(input, { onSuccess: () => go({ dialog: "created", name: f.name }) });
  };
  return (
    <FormPage
      title="Create table"
      sub={f.custom ? "Customize settings: choose the capacity mode, encryption and tags yourself." : "Name it and choose a primary key. Everything else can use the default settings."}
      crumbs={crumbs(undefined, "Create table")}
      request={request}
      refusal={refused && said ? { code: code || "Error", message: said.message } : null}
      banner={
        refused && said ? (
          <Alert tone="bad" title="The table was not created." testId="create-refused">
            <span className="font-mono text-[13px]">{code ? `${code}: ` : ""}{said.message}</span>
            {taken && <span className="mt-1 block">A table name must be unique for the account and Region, and CreateTable refuses one that is in use. Retrying the same request will not help.</span>}
          </Alert>
        ) : undefined
      }
      submitLabel="Create table"
      pending={make.isPending}
      disabled={off !== null}
      disabledWhy={off}
      onCancel={() => go({})}
      onSubmit={submit}
    >
      <FormSection title="Table details" description="The name and the primary key. The key cannot be changed after the table exists.">
        <Field
          label="Table name"
          error={p.name}
          refusal={taken ? { why: <>CreateTable refused this name: <span className="font-mono">{code}</span>.</> } : null}
          help={taken ? undefined : <><b>3 to 255 characters</b>: letters, numbers, underscore, hyphen and period. Names are case-sensitive and unique per Region.{names.length > 0 && <> <span className="font-mono">{some(names)}</span> {names.length === 1 ? "already exists" : "already exist"} here.</>}</>}
        >
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="deliveries" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="table-name" />}
        </Field>
        <Field label="Partition key" help={<>Key attributes are <b>String, Number or Binary</b>. A partition key value is 1 to 2048 bytes.</>}>
          {(id) => <KeyRow nameId={id} name={f.pk} type={f.pkType} onName={(pk) => set({ pk })} onType={(pkType) => set({ pkType })} placeholder="delivery_id" label="Partition key" testId="table-pk" />}
        </Field>
        <Field label={<>Sort key <span className="font-normal text-muted-foreground">optional</span></>} error={p.sk} help="With a sort key, many items can share a partition key; a sort key value is 1 to 1024 bytes.">
          {(id) => <KeyRow nameId={id} name={f.sk} type={f.skType} onName={(sk) => set({ sk })} onType={(skType) => set({ skType })} placeholder="" label="Sort key" testId="table-sk" />}
        </Field>
      </FormSection>
      {/* The board of a refused name draws the details alone (the default settings are not the problem); customized ones stay. */}
      {(!taken || f.custom) && <FormSection title="Table settings" description={f.custom ? "Customize settings is on, so every section below applies." : "Default settings are fine for most tables."}>
        <CardChoice
          name="settings"
          value={f.custom ? "custom" : "default"}
          onChange={(v) => set({ custom: v === "custom" })}
          options={[
            { value: "default", title: "Default settings", description: "On-demand capacity and the default encryption. On AWS this option also turns on auto scaling; doze-aws has no auto scaling." },
            { value: "custom", title: "Customize settings", description: "Choose the capacity mode, encryption and tags yourself." },
          ]}
        />
      </FormSection>}
      {f.custom && (
        <>
          <FormSection title="Read/write capacity settings" description="How the table is billed for reads and writes (BillingMode).">
            <CardChoice
              name="mode"
              value={f.mode}
              onChange={(mode) => set({ mode })}
              options={[
                { value: "PROVISIONED", title: "Provisioned", description: "Set read and write capacity units yourself." },
                { value: "PAY_PER_REQUEST", title: "On-demand", description: "Pay per request. Nothing to size." },
              ]}
            />
            {f.mode === "PROVISIONED" && (
              <div className="grid gap-4 sm:grid-cols-2">
                <Field label="Read capacity" error={p.read} help="ReadCapacityUnits, at least 1.">
                  {(id) => <Input id={id} inputMode="numeric" value={f.read} onChange={(e) => set({ read: e.target.value })} className="bg-card font-mono" data-testid="table-read" />}
                </Field>
                <Field label="Write capacity" error={p.write} help="WriteCapacityUnits, at least 1.">
                  {(id) => <Input id={id} inputMode="numeric" value={f.write} onChange={(e) => set({ write: e.target.value })} className="bg-card font-mono" data-testid="table-write" />}
                </Field>
              </div>
            )}
            <LocalStatus state="stored" reason="doze-aws does not meter or throttle, so no request is ever refused for capacity." />
          </FormSection>
          <FormSection title="Encryption at rest" description="All tables are encrypted. Choose who owns the key.">
            <RowChoice
              name="encryption"
              value={f.encryption}
              onChange={(encryption) => set({ encryption })}
              options={[
                { value: "owned", title: "Owned by Amazon DynamoDB", description: "The default. No charge." },
                { value: "aws", title: "AWS managed key", description: <>Alias <span className="font-mono">aws/dynamodb</span>, in your account. KMS charges apply.</> },
                { value: "customer", title: "Stored in your account, and owned and managed by you", description: <>A customer managed key, for example <span className="font-mono">alias/larder-app</span>. KMS charges apply.</> },
              ]}
            >
              {{
                customer: (
                  <div className="grid max-w-md gap-1.5">
                    <KeyPicker value={f.key} onChange={(key) => set({ key })} aria-label="Customer managed key" />
                    {p.key && <FieldRefusal why={p.key} />}
                  </div>
                ),
              }}
            </RowChoice>
            <LocalStatus state="stored" reason="The choice is recorded and reported by DescribeTable; items live in the data directory either way." />
          </FormSection>
          <FormSection title="Tags" description="Key 1 to 128 characters; value up to 256.">
            <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={tried} testId="create-tags" />
          </FormSection>
        </>
      )}
    </FormPage>
  );
}

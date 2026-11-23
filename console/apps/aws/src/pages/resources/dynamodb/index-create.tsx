// Create global secondary index (dynamodb__index-create): UpdateTable with GlobalSecondaryIndexUpdates. The key is defined here (its
// attributes join the table's definitions), the projection says what the index copies, and the call is shown as it is filled in.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { DetailError, Field, FormPage, FormSection, type PreviewRequest, RuleHelp } from "../../../kit";
import { useOp } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { KeyRow, RowChoice } from "./controls";
import { blankIndex, createIndexInput, type IndexForm, indexProblems, indexWhyOff, knownType } from "./index-model";
import type { Projection } from "./model";
import { crumbs, tableTarget, useTable, useTableNav } from "./parts";

export function IndexCreate({ name }: OwnProps) {
  const table = name!;
  const { table: t, isError, error, refetch } = useTable(table);
  const go = useTableNav(table);
  const [f, setF] = useState<IndexForm>(blankIndex);
  const update = useOp<unknown>(tableTarget(table), "UpdateTable");
  const back = () => go({ tab: "indexes" });
  if (isError && !t) return <DetailError service="dynamodb" kind="tables" name={table} error={error} onRetry={() => void refetch()} />;
  if (!t) return <Skeleton className="h-80 rounded-[14px]" />;
  const set = (patch: Partial<IndexForm>) => {
    setF((x) => ({ ...x, ...patch }));
    update.reset();
  };
  const input = createIndexInput(t, f);
  const request: PreviewRequest | null = input ? { service: "dynamodb", operation: "UpdateTable", input } : null;
  const p = indexProblems(t, f);
  const off = indexWhyOff(t, f);
  const others = [...t.global, ...t.local].map((i) => i.name);
  const refused = update.error;
  const said = refused ? errorWords(refused) : null;
  const code = refused instanceof ApiFailure ? refused.code : "";
  const pkWas = f.pk.trim() ? knownType(t, f.pk) : undefined;
  return (
    <FormPage
      title="Create global secondary index"
      sub={<>Add a global secondary index to <span className="font-mono text-foreground">{table}</span>. Its key must be defined here; the table keeps working while the index builds.</>}
      crumbs={[...crumbs(table), { label: "Indexes", to: `/resources/dynamodb/tables/${encodeURIComponent(table)}?tab=indexes` }, { label: "Create global secondary index", to: "" }]}
      request={request}
      refusal={refused && said ? { code: code || "Error", message: said.message } : null}
      banner={
        refused && said ? (
          <Alert tone="bad" title="The index was not created." testId="create-refused">
            <span className="font-mono text-[13px]">{code ? `${code}: ` : ""}{said.message}</span>
          </Alert>
        ) : undefined
      }
      submitLabel="Create global secondary index"
      pending={update.isPending}
      disabled={off !== null}
      disabledWhy={off}
      onCancel={back}
      onSubmit={() => input && update.mutate(input, { onSuccess: () => go({ tab: "indexes", dialog: "index-created", name: f.name }) })}
    >
      <FormSection title="Index details" description="The key you will query the index by.">
        <Field label="Index name" error={p.name} help={<>3 to 255 characters: letters, numbers, underscore, hyphen and period. Unique among the table&apos;s indexes.{others.length > 0 && <> <span className="font-mono">{others.join(", ")}</span> {others.length === 1 ? "is the only other index" : "are the other indexes"}.</>}</>}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="by-status" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="index-name" />}
        </Field>
        <Field label="Partition key" error={p.pk} help="Each index key attribute must be a scalar: String, Number or Binary." hint={pkWas ? `${f.pk.trim()} is already defined in ${t.name}; its type is kept.` : undefined}>
          {(id) => <KeyRow nameId={id} name={f.pk} type={f.pkType} onName={(pk) => set({ pk, ...(knownType(t, pk) ? { pkType: knownType(t, pk)! } : {}) })} onType={(pkType) => set({ pkType })} placeholder="status" label="Partition key" testId="index-pk" />}
        </Field>
        <Field label={<>Sort key <span className="font-normal text-muted-foreground">optional</span></>} error={p.sk}>
          {(id) => <KeyRow nameId={id} name={f.sk} type={f.skType} onName={(sk) => set({ sk, ...(knownType(t, sk) ? { skType: knownType(t, sk)! } : {}) })} onType={(skType) => set({ skType })} placeholder="" label="Sort key" testId="index-sk" />}
        </Field>
      </FormSection>
      <FormSection title="Projection" description="Which attributes the index copies, besides the table and index keys.">
        <RowChoice<Projection>
          name="projection"
          value={f.projection}
          onChange={(projection) => set({ projection })}
          options={[
            { value: "ALL", title: <span className="font-mono text-[13.5px]">ALL</span>, description: "Every attribute of the item. Queries on the index never need to read the table." },
            { value: "KEYS_ONLY", title: <span className="font-mono text-[13.5px]">KEYS_ONLY</span>, description: "Only the table and index key attributes." },
            { value: "INCLUDE", title: <span className="font-mono text-[13.5px]">INCLUDE</span>, description: "Keys plus 1 to 20 attributes you name; at most 100 named attributes across all indexes of the table." },
          ]}
        >
          {{
            INCLUDE: (
              <Field label="Attributes to copy" error={f.include.trim() === "" ? undefined : p.include} help="Separate the names with commas.">
                {(id) => <Input id={id} value={f.include} onChange={(e) => set({ include: e.target.value })} placeholder="status, total" autoComplete="off" spellCheck={false} className="max-w-md bg-card font-mono" data-testid="index-include" />}
              </Field>
            ),
          }}
        </RowChoice>
        {t.mode === "PAY_PER_REQUEST" ? (
          <RuleHelp>
            <span className="font-mono">{t.name}</span> is <b>on-demand</b>, so the index needs no read or write capacity. On a provisioned table it would need its own.
          </RuleHelp>
        ) : (
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Read capacity" error={p.read} help="ReadCapacityUnits of the index, at least 1.">
              {(id) => <Input id={id} inputMode="numeric" value={f.read} onChange={(e) => set({ read: e.target.value })} className="bg-card font-mono" data-testid="index-read" />}
            </Field>
            <Field label="Write capacity" error={p.write} help="WriteCapacityUnits of the index, at least 1.">
              {(id) => <Input id={id} inputMode="numeric" value={f.write} onChange={(e) => set({ write: e.target.value })} className="bg-card font-mono" data-testid="index-write" />}
            </Field>
          </div>
        )}
      </FormSection>
      <Callout tone="info" testId="index-note">
        Only <b className="font-semibold text-bright">one index can be created or deleted per request</b>. On AWS you cannot query the new index until it is Active; here the backfill finishes inside the call.
      </Callout>
    </FormPage>
  );
}

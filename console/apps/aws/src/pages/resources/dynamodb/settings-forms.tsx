// The two full-page edits of a table's Additional settings: Edit read/write capacity (dynamodb__capacity-edit, UpdateTable) and Turn on
// Time to Live (dynamodb__ttl-edit, UpdateTimeToLive). Each shows the call it will make and says what AWS and doze-aws do with it.
import { useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, LocalStatus, type PreviewRequest, RuleHelp } from "../../../kit";
import { useOp } from "../deep/api";
import type { OwnProps } from "../deep/registry";
import { CardChoice } from "./controls";
import { units } from "./create-model";
import { modeWords } from "./model";
import { crumbs, tableTarget, useTable, useTableNav } from "./parts";

const settingsCrumbs = (table: string, last: string) => [...crumbs(table), { label: "Additional settings", to: `/resources/dynamodb/tables/${encodeURIComponent(table)}?tab=settings` }, { label: last, to: "" }];

export function CapacityEdit({ name }: OwnProps) {
  const table = name!;
  const { table: t, isError, error, refetch } = useTable(table);
  const go = useTableNav(table);
  const update = useOp<unknown>(tableTarget(table), "UpdateTable");
  const [mode, setMode] = useState<"PROVISIONED" | "PAY_PER_REQUEST" | null>(null);
  const [read, setRead] = useState<string | null>(null);
  const [write, setWrite] = useState<string | null>(null);
  if (isError && !t) return <ErrorBlock what="This table could not be read" error={error} onRetry={() => void refetch()} testId="detail-error" />;
  if (!t) return <Skeleton className="h-80 rounded-[14px]" />;
  const m = mode ?? t.mode;
  const r = read ?? String(t.read ?? 5);
  const w = write ?? String(t.write ?? 5);
  const input = m === "PROVISIONED" ? { TableName: table, BillingMode: "PROVISIONED", ProvisionedThroughput: { ReadCapacityUnits: units(r) ?? 0, WriteCapacityUnits: units(w) ?? 0 } } : { TableName: table, BillingMode: "PAY_PER_REQUEST" };
  const request: PreviewRequest = { service: "dynamodb", operation: "UpdateTable", input };
  const same = m === t.mode && (m === "PAY_PER_REQUEST" || (units(r) === t.read && units(w) === t.write));
  const readBad = m === "PROVISIONED" && units(r) === null;
  const writeBad = m === "PROVISIONED" && units(w) === null;
  const why = readBad || writeBad ? "Units are whole numbers, at least 1." : same ? "Nothing is changed yet." : null;
  const said = update.error ? errorWords(update.error) : null;
  return (
    <FormPage
      title="Edit read/write capacity"
      sub={<><span className="font-mono text-foreground">{table}</span>. Switch between paying per request and setting capacity units.</>}
      crumbs={settingsCrumbs(table, "Edit read/write capacity")}
      request={request}
      refusal={update.error && said ? { code: update.error instanceof ApiFailure ? update.error.code : "Error", message: said.message } : null}
      banner={said ? <Alert tone="bad" title="The capacity was not changed." testId="edit-refused"><span className="font-mono text-[13px]">{said.code ? `${said.code}: ` : ""}{said.message}</span></Alert> : undefined}
      submitLabel="Save"
      pending={update.isPending}
      disabled={why !== null}
      disabledWhy={why}
      onCancel={() => go({ tab: "settings" })}
      onSubmit={() => update.mutate(input, { onSuccess: () => go({ tab: "settings", dialog: "capacity-saved" }) })}
    >
      <FormSection title="Capacity mode" description={<><span className="font-mono">{table}</span> is {modeWords(t)} today.</>}>
        <CardChoice
          name="capacity-mode"
          value={m}
          onChange={(v) => (setMode(v), update.reset())}
          options={[
            { value: "PROVISIONED", title: "Provisioned", description: `Set read and write capacity units for the table.${t.mode === "PROVISIONED" ? " This is the current mode." : ""}` },
            { value: "PAY_PER_REQUEST", title: "On-demand", description: `Pay per request.${t.mode === "PAY_PER_REQUEST" ? " This is the current mode." : ""}` },
          ]}
        />
      </FormSection>
      {m === "PROVISIONED" && (
        <FormSection title="Table capacity" description="Units set for the table.">
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Read capacity" error={readBad ? "ReadCapacityUnits is a whole number, at least 1." : undefined} help={<><span className="font-mono">ReadCapacityUnits</span>, at least 1.</>}>
              {(id) => <Input id={id} inputMode="numeric" value={r} onChange={(e) => (setRead(e.target.value), update.reset())} className="bg-card font-mono" data-testid="capacity-read" />}
            </Field>
            <Field label="Write capacity" error={writeBad ? "WriteCapacityUnits is a whole number, at least 1." : undefined} help={<><span className="font-mono">WriteCapacityUnits</span>, at least 1.</>}>
              {(id) => <Input id={id} inputMode="numeric" value={w} onChange={(e) => (setWrite(e.target.value), update.reset())} className="bg-card font-mono" data-testid="capacity-write" />}
            </Field>
          </div>
          <LocalStatus state="stored" reason="doze-aws does not meter or throttle, so these numbers are recorded and reported by DescribeTable and nothing more." />
        </FormSection>
      )}
    </FormPage>
  );
}

export function TtlEdit({ name }: OwnProps) {
  const table = name!;
  const go = useTableNav(table);
  const update = useOp<unknown>(tableTarget(table), "UpdateTimeToLive");
  const [attribute, setAttribute] = useState("");
  const input = { TableName: table, TimeToLiveSpecification: { Enabled: true, AttributeName: attribute } };
  const request: PreviewRequest | null = attribute ? { service: "dynamodb", operation: "UpdateTimeToLive", input } : null;
  const said = update.error ? errorWords(update.error) : null;
  return (
    <FormPage
      title="Turn on Time to Live (TTL)"
      sub={<><span className="font-mono text-foreground">{table}</span>. DynamoDB deletes an item once the number in its TTL attribute is in the past.</>}
      crumbs={settingsCrumbs(table, "Turn on Time to Live (TTL)")}
      request={request}
      refusal={update.error && said ? { code: update.error instanceof ApiFailure ? update.error.code : "Error", message: said.message } : null}
      banner={said ? <Alert tone="bad" title="Time to Live was not turned on." testId="edit-refused"><span className="font-mono text-[13px]">{said.code ? `${said.code}: ` : ""}{said.message}</span></Alert> : undefined}
      submitLabel="Turn on TTL"
      pending={update.isPending}
      disabled={!attribute.trim()}
      disabledWhy="Name the attribute."
      onCancel={() => go({ tab: "settings" })}
      onSubmit={() => update.mutate(input, { onSuccess: () => go({ tab: "settings", dialog: "ttl-saved", name: attribute }) })}
    >
      <FormSection title="TTL attribute" description="The attribute DynamoDB reads on every item.">
        <Field label="TTL attribute name" help={<><b>Case-sensitive</b>, and it must match what your writes use: a mismatch leaves expired items undeleted. To rename it later you turn TTL off and on again. The value is a Number in epoch seconds, for example <span className="font-mono">1794211092</span>; an item whose attribute is not a Number is ignored.</>}>
          {(id) => <Input id={id} value={attribute} onChange={(e) => (setAttribute(e.target.value), update.reset())} placeholder="expires_at" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="ttl-attribute" />}
        </Field>
        <RuleHelp>
          On AWS it takes about an hour for TTL to be on across all partitions, and an expired item is deleted some time after it expires, typically within a few days. doze-aws deletes an expired item as soon as it falls due (at the next call, or when its timer runs), not days later. A second <span className="font-mono">UpdateTimeToLive</span> that asks for the state the table is already in is refused with a <span className="font-mono">ValidationException</span>.
        </RuleHelp>
      </FormSection>
    </FormPage>
  );
}

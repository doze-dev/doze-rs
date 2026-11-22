// Schedule key deletion (kms__key-schedule-deletion): the keys to delete, a waiting period of 7 to 30 days with the day it ends, a
// confirmation, and the call it makes for each key. The keys come from the list (`?do=schedule&sel=<ids>`).
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { DataTable, type DataColumn } from "@doze/ui/components/app/data-table";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Label } from "@doze/ui/components/ui/label";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { DeletionWindow, FormPage, FormSection, purgeDate, windowDays } from "../../../kit";
import { useAllRows } from "../cfg-api";
import { Note } from "../cfg-parts";
import { useKeyActions } from "./key-actions";
import { aliasWords, keyName, keyRows } from "./model";
import { crumbs, stateOf } from "./parts";

export function ScheduleDeletion({ ids }: { ids: string[] }) {
  const navigate = useNavigate();
  const list = useAllRows("kms", "keys", { all: true });
  const aliases = useAllRows("kms", "aliases");
  const { run, pending } = useKeyActions();
  const [days, setDays] = useState("7");
  const [now] = useState(() => Date.now());
  const [confirmed, setConfirmed] = useState(false);
  const [tried, setTried] = useState(false);
  const [refused, setRefused] = useState<{ code: string; message: string } | null>(null);
  const keys = useMemo(() => keyRows(list.rows, list.columns, aliases.rows.map((a) => ({ name: a.name, key: a.cells[aliases.columns.findIndex((c) => c.title === "Key")] ?? "" }))).filter((k) => ids.includes(k.id)), [list.rows, list.columns, aliases.rows, aliases.columns, ids]);
  const n = windowDays(days);
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "kms", second: "keys" }, search: {} });
  const columns: DataColumn<(typeof keys)[number]>[] = [
    { id: "alias", header: "Aliases", min: 160, accessorFn: aliasWords, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{aliasWords(c.row.original)}</span> },
    { id: "id", header: "Key ID", accessorKey: "id", min: 240, meta: { mono: true } },
    { id: "state", header: "Status", accessorFn: (r) => r.stateWords, size: 150, fit: 70, cell: (c) => stateOf(c.row.original) },
  ];
  const first = list.first;
  const submit = async () => {
    setTried(true);
    if (n === null || !confirmed || keys.length === 0) return;
    setRefused(null);
    const done = await run("schedule", keys.map((k) => k.id), { PendingWindowInDays: n });
    if (done.refused) return setRefused({ code: done.refused.code, message: done.refused.message });
    void navigate({ to: "/resources/$service/$second", params: { service: "kms", second: "keys" }, search: { do: "scheduled", sel: done.ids.join(",") } });
  };
  const missing = ids.length === 0 ? "No key was chosen. Go back to the list and select the keys to delete." : null;
  const requests = keys.map((k) => ({ service: "kms", operation: "ScheduleKeyDeletion", input: { KeyId: k.id, PendingWindowInDays: n ?? 7 } }));
  return (
    <FormPage
      title="Schedule key deletion"
      sub="Choose a waiting period and confirm. The key becomes unusable at once and is deleted when the period ends."
      crumbs={[...crumbs(first?.group, first?.service_title), { label: "Customer managed keys", to: "/resources/kms/keys" }]}
      request={null}
      requests={requests.length > 0 ? requests : undefined}
      refusal={refused}
      callNote="The response carries DeletionDate and KeyState PendingDeletion."
      hideFooter
      submitLabel="Schedule deletion"
      onSubmit={() => void submit()}
      onCancel={back}
      testId="key-schedule"
    >
      <Callout tone="bad" lead="Deleting a KMS key is destructive and potentially dangerous." testId="deletion-warning">
        You should proceed only when you are sure that you don&apos;t need to use the KMS key anymore and won&apos;t need to use it in the future. If you are not sure, you should disable the KMS key instead of deleting it. You can cancel the deletion during the waiting period.
      </Callout>
      <section className="grid gap-3" aria-label="Keys to delete">
        <h2 className="text-[17px] font-semibold tracking-[-0.01em] text-bright">Review the KMS keys that you are deleting</h2>
        {missing ? <Callout tone="warn" testId="no-keys">{missing}</Callout> : list.loading || aliases.loading ? <Skeleton className="h-24 rounded-xl" /> : <DataTable aria-label="Keys to delete" data={keys} columns={columns} rowId={(k) => k.id} maxHeight={320} />}
      </section>
      <FormSection title="Waiting period" testId="section-waiting">
        <DeletionWindow mode="choose" noun="key" days={days} onDays={setDays} now={now} label="Waiting period (in days)" bare />
        <Note testId="window-note">
          <b className="font-semibold text-foreground">7 to 30 days; the API default is 30.</b> Until then the {keys.length === 1 ? "key" : "keys"} cannot be used, and Cancel deletion restores {keys.length === 1 ? "it" : "them"}.
          {n !== null && keys.length > 0 && <> With {n}, {keys.length === 1 ? keyName(keys[0]!) : "each key"} is deleted on <b className="font-semibold text-foreground">{purgeDate(now, n)}</b>. When the period ends, KMS deletes the key, its key material and every alias that refers to it, and data encrypted under the key cannot be recovered.</>}
        </Note>
        <div className="flex items-start gap-3" data-refused={tried && !confirmed ? "" : undefined}>
          <Checkbox id="confirm-deletion" checked={confirmed} onCheckedChange={(v) => setConfirmed(v === true)} className="mt-0.5" data-testid="confirm-deletion" />
          <Label htmlFor="confirm-deletion" className="grid gap-0.5 font-normal">
            <span className="font-medium text-bright">I confirm that I want to schedule {keys.length === 1 ? "this key" : "these keys"} for deletion.</span>
            <span className="text-[13px] text-muted-foreground">{keys.length === 1 ? "The key" : "The keys"} will become immediately unusable for cryptographic operations. After the {n ?? days} day waiting period, {keys.length === 1 ? "the key" : "the keys"} will be permanently deleted.</span>
          </Label>
        </div>
        {tried && !confirmed && <p className="text-[13px] text-bad" role="alert" data-testid="confirm-problem">Confirm that you want to schedule the deletion.</p>}
      </FormSection>
      <div className="flex flex-wrap items-center justify-end gap-3 border-t pt-5" data-slot="form-footer">
        <Button type="button" variant="outline" className="bg-card" onClick={back} data-testid="form-cancel">Cancel</Button>
        <Button type="submit" variant="destructive" disabled={pending || (tried && (n === null || !confirmed))} data-testid="form-submit">{pending ? "Working…" : "Schedule deletion"}</Button>
      </div>
    </FormPage>
  );
}

/** The service's refusal as a code and words. */
export const refusalOf = (e: unknown): { code: string; message: string } => (e instanceof ApiFailure ? { code: e.code, message: e.message } : { code: "Error", message: e instanceof Error ? e.message : "No reason was given." });

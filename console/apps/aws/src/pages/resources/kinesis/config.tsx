// A stream's Configuration tab (kinesis__stream-detail, stream-tags-none): the sections Capacity, Data retention period, Server-side encryption,
// Shard level metrics and Tags, each with its Edit and what the setting means here. The edits are dialogs (`edits.tsx`); Manage tags is a page.
import { PencilIcon, TagsIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { Button } from "@doze/ui/components/ui/button";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { type Tag, type TagsAnswer } from "../../../kit";
import { CapacityEdit, EncryptionEdit, MetricsEdit, RetentionEdit, ShardsEdit } from "./edits";
import { type Stream, capacityWords, hoursWords, modeWords } from "./model";

const note = "mt-3 max-w-[40rem] border-l-2 border-input pl-3.5 text-[13px] leading-5 text-muted-foreground";

function Section({ title, actions, children, testId }: { title: string; actions?: React.ReactNode; children: React.ReactNode; testId: string }) {
  return (
    <section className="rounded-[var(--radius-card)] border bg-card p-6 lg:p-7" aria-label={title} data-testid={testId}>
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

function Rows({ rows }: { rows: [string, React.ReactNode][] }) {
  return (
    <dl className="mt-3 grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
      {rows.map(([l, v]) => (
        <div key={l} className="contents">
          <dt className="border-b py-3.5 pr-4 text-[13.5px] text-muted-foreground">{l}</dt>
          <dd className="min-w-0 border-b py-3.5 text-[13.5px] break-words text-bright" data-testid={`field-${l}`}>{v}</dd>
        </div>
      ))}
    </dl>
  );
}

const EditButton = ({ onClick, children = "Edit", testId }: { onClick: () => void; children?: React.ReactNode; testId: string }) => (
  <Button type="button" variant="outline" className="bg-card" onClick={onClick} data-testid={testId}><PencilIcon /> {children}</Button>
);

const TAG_COLUMNS: DataColumn<Tag>[] = [
  { id: "key", header: "Key", accessorKey: "key", size: 320, min: 140, meta: { mono: true }, cell: (c) => <b className="font-medium text-bright">{c.row.original.key}</b> },
  { id: "value", header: "Value", accessorKey: "value", meta: { mono: true }, cell: (c) => c.row.original.value || <span className="text-muted-foreground">(empty)</span> },
];

type Dialog = "capacity" | "shards" | "retention" | "encryption" | "metrics" | null;

export function ConfigTab({ s, metrics, oldest, tags, manage }: { s: Stream; metrics: string[]; oldest: { shard: string; at: number } | null; tags: TagsAnswer | undefined; manage: () => void }) {
  const [dialog, setDialog] = useState<Dialog>(null);
  const close = () => setDialog(null);
  const provisioned = s.mode === "PROVISIONED";
  return (
    <div className="grid gap-5" data-testid="stream-config">
      <Section title="Capacity" testId="section-capacity" actions={<><EditButton onClick={() => setDialog("capacity")} testId="edit-capacity">Edit capacity mode</EditButton>{provisioned && <EditButton onClick={() => setDialog("shards")} testId="edit-shards">Edit provisioned shards</EditButton>}</>}>
        <Rows rows={[["Capacity mode", modeWords(s.mode)], [provisioned ? "Provisioned shards" : "Open shards", s.shards]]} />
        <p className={note}>In AWS each shard takes writes of up to 1 MiB per second and 1,000 records per second and serves reads of up to 2 MiB per second, so {capacityWords(s.shards)}. <span className="text-muted-foreground/90">doze-aws does not throttle, so these figures are for sizing only.</span></p>
      </Section>
      <Section title="Data retention period" testId="section-retention" actions={<EditButton onClick={() => setDialog("retention")} testId="edit-retention" />}>
        <Rows rows={[["Data retention period", hoursWords(s.retention)]]} />
        <p className={note}>Records are readable for {hoursWords(s.retention)}{s.retention === 24 ? " (the default)" : ""} after they are added, up to 365 days. {oldest ? <span className="text-muted-foreground/90">The oldest record the Data Viewer can reach on {oldest.shard} arrived on {dateTimeUtc(oldest.at)}.</span> : <span className="text-muted-foreground/90">No record is in the stream yet.</span>}</p>
      </Section>
      <Section title="Server-side encryption" testId="section-encryption" actions={<EditButton onClick={() => setDialog("encryption")} testId="edit-encryption" />}>
        <Rows rows={[["Server-side encryption", s.encryption === "KMS" ? "Enabled" : "Disabled"], ["KMS key", s.kms ? <span key="k" className="font-mono">{s.kms}</span> : <span key="k" className="text-muted-foreground">None</span>]]} />
        <p className={note}><b className="font-medium text-foreground">Stored, not applied locally.</b> The key id is kept and shown; records are not encrypted with it.</p>
      </Section>
      <Section title="Shard level metrics" testId="section-metrics" actions={<EditButton onClick={() => setDialog("metrics")} testId="edit-metrics" />}>
        <Rows rows={[["Shard-level metrics", metrics.length === 0 ? "None enabled" : <span key="m" className="font-mono">{metrics.join(", ")}</span>]]} />
        <p className={note}><b className="font-medium text-foreground">Stored, not applied locally.</b> The metric names are kept and shown. Nothing produces AWS/Kinesis metrics in doze-aws, so a stream has no Monitoring charts here.</p>
      </Section>
      <Section title="Tags" testId="section-tags" actions={<Button type="button" variant="outline" className="bg-card" onClick={manage} data-testid="manage-tags"><TagsIcon /> Manage tags</Button>}>
        {!tags ? null : tags.tags.length === 0 ? (
          <EmptyTab icon={TagsIcon} title="No tags" className="mt-4 min-h-0" testId="tags-empty" action={<Button type="button" variant="outline" className="bg-card" onClick={manage}>Manage tags</Button>}>
            {s.name} has no tags. Tags are key-value labels for organising streams and tracking their cost.
          </EmptyTab>
        ) : (
          <div className="mt-4"><DataTable aria-label="Tags" data={tags.tags} columns={TAG_COLUMNS} rowId={(t) => t.key} /></div>
        )}
      </Section>
      {dialog === "capacity" && <CapacityEdit s={s} onClose={close} />}
      {dialog === "shards" && <ShardsEdit s={s} onClose={close} />}
      {dialog === "retention" && <RetentionEdit s={s} onClose={close} />}
      {dialog === "encryption" && <EncryptionEdit s={s} onClose={close} />}
      {dialog === "metrics" && <MetricsEdit s={s} now={metrics} onClose={close} />}
    </div>
  );
}

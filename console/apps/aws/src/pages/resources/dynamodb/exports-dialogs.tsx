// What the Exports and streams tab asks in dialogs over it: turn the DynamoDB stream on (UpdateTable), export the table to S3
// (ExportTableToPointInTime, which needs point-in-time recovery) and send every change to a Kinesis data stream
// (EnableKinesisStreamingDestination). Each shows the call it makes and the service's refusal in place.
import { useState } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { useAbout } from "../../../api/queries";
import { Field, ResourcePicker, SectionEdit } from "../../../kit";
import { useOp, useOpRead } from "../deep/api";
import { tableTarget } from "./parts";

export const VIEWS = [
  ["KEYS_ONLY", "Only the key attributes of the changed item."],
  ["NEW_IMAGE", "The entire item as it appears after it was changed."],
  ["OLD_IMAGE", "The entire item as it appeared before it was changed."],
  ["NEW_AND_OLD_IMAGES", "Both the new and the old image of the item."],
] as const;

function Choose({ value, onChange, options, label, id }: { value: string; onChange: (v: string) => void; options: readonly (readonly [string, string])[]; label: string; id?: string }) {
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger id={id} aria-label={label} className="w-full bg-card font-mono text-[13px]" data-testid={`choose-${label}`}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map(([v, words]) => (
          <SelectItem key={v} value={v} className="font-mono text-[13px]" title={words}>
            {v}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export function StreamOn({ table, onClose, onDone }: { table: string; onClose: () => void; onDone: () => void }) {
  const [view, setView] = useState<string>("NEW_AND_OLD_IMAGES");
  const update = useOp<unknown>(tableTarget(table), "UpdateTable");
  const input = { TableName: table, StreamSpecification: { StreamEnabled: true, StreamViewType: view } };
  return (
    <SectionEdit open onOpenChange={(o) => !o && onClose()} title="Turn on DynamoDB stream" description={<>A record of every change to <span className="font-mono">{table}</span>, readable for 24 hours.</>} request={{ service: "dynamodb", operation: "UpdateTable", input }} saveLabel="Turn on stream" pending={update.isPending} error={update.error} onSave={() => update.mutate(input, { onSuccess: onDone })} testId="stream-on">
      <Field label="View type" help={VIEWS.find(([v]) => v === view)?.[1]}>
        {(id) => <Choose id={id} value={view} onChange={setView} options={VIEWS} label="View type" />}
      </Field>
    </SectionEdit>
  );
}

type Backups = { ContinuousBackupsDescription?: { PointInTimeRecoveryDescription?: { PointInTimeRecoveryStatus?: string } } };

export function ExportToS3({ table, arn, onClose, onDone }: { table: string; arn: string; onClose: () => void; onDone: (id: string) => void }) {
  const t = tableTarget(table);
  const backups = useOpRead<Backups>(t, "DescribeContinuousBackups", {});
  const pitr = useOp<unknown>(t, "UpdateContinuousBackups");
  const make = useOp<{ ExportDescription?: { ExportArn?: string } }>(t, "ExportTableToPointInTime");
  const [bucket, setBucket] = useState<string | null>(null);
  const [prefix, setPrefix] = useState("");
  const [format, setFormat] = useState("DYNAMODB_JSON");
  const on = backups.data?.ContinuousBackupsDescription?.PointInTimeRecoveryDescription?.PointInTimeRecoveryStatus === "ENABLED";
  const input = { TableArn: arn, S3Bucket: bucket ?? "", ...(prefix.trim() ? { S3Prefix: prefix.trim() } : {}), ExportFormat: format };
  return (
    <SectionEdit
      open
      onOpenChange={(o) => !o && onClose()}
      title="Export to S3"
      description={<>A snapshot of <span className="font-mono">{table}</span> written to an S3 bucket in this doze-aws.</>}
      request={{ service: "dynamodb", operation: "ExportTableToPointInTime", input }}
      saveLabel="Export to S3"
      pending={make.isPending}
      error={make.error}
      disabled={!bucket || !on}
      disabledWhy={!on ? "Point-in-time recovery is off." : "Choose the bucket."}
      onSave={() => make.mutate(input, { onSuccess: (a) => onDone(a.ExportDescription?.ExportArn?.split("/").at(-1) ?? "") })}
      testId="export-dialog"
    >
      {backups.data && !on && (
        <Callout tone="warn" lead="Point-in-time recovery is off." testId="pitr-off">
          A full export reads the table as it was at a time, from the recovery log, so ExportTableToPointInTime is refused without it.
          <span className="mt-2 block">
            <Button type="button" variant="outline" size="sm" className="bg-card" disabled={pitr.isPending} onClick={() => pitr.mutate({ PointInTimeRecoverySpecification: { PointInTimeRecoveryEnabled: true } })} data-testid="pitr-on">
              {pitr.isPending ? "Turning on…" : "Turn on point-in-time recovery"}
            </Button>
          </span>
        </Callout>
      )}
      <Field label="S3 bucket">{(id) => <ResourcePicker id={id} service="s3" kind="buckets" noun="bucket" by="name" value={bucket} onChange={setBucket} />}</Field>
      <Field label={<>S3 prefix <span className="font-normal text-muted-foreground">optional</span></>} help="The export is written under this prefix, in AWSDynamoDB/<export id>/.">
        {(id) => <Input id={id} value={prefix} onChange={(e) => setPrefix(e.target.value)} placeholder="exports" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="export-prefix" />}
      </Field>
      <Field label="Format">{(id) => <Choose id={id} value={format} onChange={setFormat} options={[["DYNAMODB_JSON", "JSON lines, in DynamoDB's typed JSON."], ["ION", "Amazon Ion text."]]} label="Format" />}</Field>
    </SectionEdit>
  );
}

export function KinesisOn({ table, onClose, onDone }: { table: string; onClose: () => void; onDone: () => void }) {
  const about = useAbout().data;
  const enable = useOp<unknown>(tableTarget(table), "EnableKinesisStreamingDestination");
  const [stream, setStream] = useState<string | null>(null);
  const [precision, setPrecision] = useState("default");
  const arn = stream ? `arn:aws:kinesis:${about?.region ?? "us-east-1"}:${about?.account ?? ""}:stream/${stream}` : "";
  const input = { TableName: table, StreamArn: arn, ...(precision !== "default" ? { EnableKinesisStreamingConfiguration: { ApproximateCreationDateTimePrecision: precision } } : {}) };
  return (
    <SectionEdit open onOpenChange={(o) => !o && onClose()} title="Turn on Kinesis data stream" description={<>Send every change of <span className="font-mono">{table}</span> to a Kinesis data stream.</>} request={{ service: "dynamodb", operation: "EnableKinesisStreamingDestination", input }} saveLabel="Turn on" pending={enable.isPending} error={enable.error} disabled={!stream} disabledWhy="Choose the stream." onSave={() => enable.mutate(input, { onSuccess: onDone })} testId="kinesis-on">
      <Field label="Kinesis data stream">{(id) => <ResourcePicker id={id} service="kinesis" kind="streams" noun="stream" by="name" value={stream} onChange={setStream} />}</Field>
      <Field label="Timestamp precision" help="The precision of ApproximateCreationDateTime in each record.">
        {(id) => <Choose id={id} value={precision} onChange={setPrecision} options={[["default", "The service's default."], ["MILLISECOND", "Milliseconds."], ["MICROSECOND", "Microseconds."]]} label="Precision" />}
      </Field>
    </SectionEdit>
  );
}

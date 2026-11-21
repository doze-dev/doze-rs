// A table's Exports and streams tab (dynamodb__table-exports, exports-none): its DynamoDB stream (turned on and off with UpdateTable),
// its exports to S3 (ExportTableToPointInTime, ListExports) and its Kinesis data stream destination. The boards call all three "not
// available in doze-aws"; the service builds each (crates/aws-dynamodb/src/{tables_update,export,kinesis}.rs), so the tab does them.
import { UploadIcon } from "lucide-react";
import { useState } from "react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { useOp, useOpRead } from "../deep/api";
import { Card } from "../deep/parts";
import { ExportToS3, KinesisOn, StreamOn } from "./exports-dialogs";
import type { TableInfo } from "./model";
import { Ask, Said, tableTarget } from "./parts";
import { Dash, Facts, OnOff } from "./tab-parts";

type Exported = { ExportSummaries?: { ExportArn: string; ExportStatus?: string; ExportType?: string }[] };
type Destinations = { KinesisDataStreamDestinations?: { StreamArn: string; DestinationStatus?: string; DestinationStatusDescription?: string }[] };

const EXPORTS: DataColumn<NonNullable<Exported["ExportSummaries"]>[number]>[] = [
  { id: "id", header: "Export", accessorFn: (r) => r.ExportArn, size: 320, min: 160, meta: { mono: true }, cell: (c) => <span className="font-mono text-[13px] text-bright" title={c.row.original.ExportArn}>{c.row.original.ExportArn.split("/").at(-1)}</span> },
  { id: "status", header: "ExportStatus", accessorFn: (r) => r.ExportStatus ?? "", size: 180, meta: { mono: true } },
  { id: "type", header: "ExportType", accessorFn: (r) => r.ExportType ?? "FULL_EXPORT", size: 180, meta: { mono: true } },
];

export function ExportsTab({ t }: { t: TableInfo }) {
  const target = tableTarget(t.name);
  const exports = useOpRead<Exported>(target, "ListExports", {});
  const kinesis = useOpRead<Destinations>(target, "DescribeKinesisStreamingDestination", {});
  const update = useOp<unknown>(target, "UpdateTable");
  const disable = useOp<unknown>(target, "DisableKinesisStreamingDestination");
  const [open, setOpen] = useState<"stream" | "export" | "kinesis" | "stream-off" | "kinesis-off" | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const close = () => setOpen(null);
  const finished = (words: string) => () => (close(), setDone(words));
  const listed = exports.data?.ExportSummaries ?? [];
  const live = (kinesis.data?.KinesisDataStreamDestinations ?? []).find((d) => d.DestinationStatus !== "DISABLED");
  return (
    <div className="grid gap-6">
      {done && <ResultBanner title={done} onDismiss={() => setDone(null)} testId="exports-done" />}
      <Card title="DynamoDB stream details" lede="A log of item changes kept for up to 24 hours, readable by consumers such as Lambda." testId="stream-card">
        <div className="grid gap-5">
          <div>
            {t.stream.enabled ? (
              <Button variant="outline" className="bg-card" onClick={() => setOpen("stream-off")} data-testid="stream-off-open">Turn off</Button>
            ) : (
              <Button variant="outline" className="bg-card" onClick={() => setOpen("stream")} data-testid="stream-on-open">Turn on</Button>
            )}
          </div>
          <Facts
            rows={[
              ["StreamEnabled", <OnOff key="e" on={t.stream.enabled} />],
              ["StreamViewType", t.stream.view ?? Dash],
              ["LatestStreamArn", t.stream.arn ? <span className="font-mono text-[13px]" key="a">{t.stream.arn}</span> : Dash],
            ]}
          />
          <Said>
            A table whose stream is on can be read by a Lambda function through an event source mapping. AWS keeps stream records for 24 hours; doze-aws does the same.
          </Said>
        </div>
      </Card>
      <Card title="Exports to S3" lede="Snapshots of the table written to an S3 bucket." testId="exports-card">
        <div className="grid gap-5">
          <div>
            <Button variant="outline" className="bg-card" onClick={() => setOpen("export")} data-testid="export-open">
              <UploadIcon /> Export to S3
            </Button>
          </div>
          {exports.isPending ? (
            <Skeleton className="h-20 rounded-xl" />
          ) : listed.length === 0 ? (
            <EmptyTab title="No exports" testId="exports-none">{t.name} has not been exported. An export writes the table&apos;s data to S3.</EmptyTab>
          ) : (
            <DataTable aria-label="Exports" data={listed} columns={EXPORTS} rowId={(r) => r.ExportArn} />
          )}
        </div>
      </Card>
      <Card title="Amazon Kinesis data stream details" lede="Send every change to a Kinesis data stream." testId="kinesis-card">
        <div className="grid gap-5">
          <div>
            {live ? (
              <Button variant="outline" className="bg-card" onClick={() => setOpen("kinesis-off")} data-testid="kinesis-off-open">Turn off</Button>
            ) : (
              <Button variant="outline" className="bg-card" onClick={() => setOpen("kinesis")} data-testid="kinesis-on-open">Turn on</Button>
            )}
          </div>
          <Facts
            rows={[
              ["Stream", live ? <span className="font-mono text-[13px]" key="s">{live.StreamArn.split("/").at(-1)}</span> : Dash],
              ["DestinationStatus", live?.DestinationStatus ?? Dash],
            ]}
          />
        </div>
      </Card>
      {open === "stream" && <StreamOn table={t.name} onClose={close} onDone={finished(`The DynamoDB stream of ${t.name} is on.`)} />}
      {open === "export" && <ExportToS3 table={t.name} arn={t.arn} onClose={close} onDone={(id) => (close(), setDone(`The export ${id} was started.`))} />}
      {open === "kinesis" && <KinesisOn table={t.name} onClose={close} onDone={finished(`${t.name} now sends every change to its Kinesis data stream.`)} />}
      {open === "stream-off" && (
        <Ask open onOpenChange={(o) => !o && (close(), update.reset())} title="Turn off the DynamoDB stream?" verb="Turn off stream" call="UpdateTable" pending={update.isPending} error={update.error} testId="stream-off-dialog" onConfirm={() => update.mutate({ StreamSpecification: { StreamEnabled: false } }, { onSuccess: finished(`The DynamoDB stream of ${t.name} is off.`) })}>
          <p>Its records go with it, and a Lambda function reading <span className="font-mono">{t.name}</span> through an event source mapping stops receiving changes. Turning it on again makes a new stream.</p>
        </Ask>
      )}
      {open === "kinesis-off" && live && (
        <Ask open onOpenChange={(o) => !o && (close(), disable.reset())} title="Turn off the Kinesis data stream?" verb="Turn off" call="DisableKinesisStreamingDestination" pending={disable.isPending} error={disable.error} testId="kinesis-off-dialog" onConfirm={() => disable.mutate({ StreamArn: live.StreamArn }, { onSuccess: finished(`${t.name} no longer sends its changes to Kinesis.`) })}>
          <p>Changes of <span className="font-mono">{t.name}</span> stop going to <span className="font-mono">{live.StreamArn.split("/").at(-1)}</span>. Records already sent stay on the stream.</p>
        </Ask>
      )}
    </div>
  );
}

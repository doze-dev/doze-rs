// A secret's Versions tab (secretsmanager__secret-versions): the versions by staging label, and how to roll back with the AWS CLI.
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { useAbout } from "../../../api/queries";
import { Card, Note } from "../cfg-parts";
import { useVersions } from "./api";
import { type Version, versions, withStage } from "./model";

export const RECENT_ID = "versions";
const COLUMNS: DataColumn<Version>[] = [
  { id: "id", header: "Version ID", accessorKey: "id", min: 260, meta: { mono: true } },
  { id: "stages", header: "Staging labels", accessorFn: (v) => v.stages.join(", "), size: 220, cell: (c) => (c.row.original.stages.length > 0 ? c.row.original.stages.join(", ") : <span className="text-muted-foreground">–</span>) },
  { id: "created", header: "Created date", accessorFn: (v) => v.created ?? 0, size: 220, cell: (c) => (c.row.original.created ? dateTimeUtc(c.row.original.created) : "–") },
];

export function TabVersions({ name }: { name: string }) {
  const q = useVersions(name);
  const endpoint = useAbout().data?.endpoint ?? "http://127.0.0.1:4566";
  if (!q.data) return <Skeleton className="h-48 rounded-[var(--radius-card)]" data-testid="versions-loading" />;
  const vs = versions(q.data.Versions);
  const cur = withStage(vs, "AWSCURRENT");
  const prev = withStage(vs, "AWSPREVIOUS");
  const cli = `aws secretsmanager update-secret-version-stage \\\n  --endpoint-url ${endpoint} \\\n  --secret-id ${name} \\\n  --version-stage AWSCURRENT \\\n  --move-to-version-id ${prev?.id ?? "<the version to make current>"} \\\n  --remove-from-version-id ${cur?.id ?? "<the current version>"}`;
  return (
    <div className="grid gap-5" data-testid="versions-tab">
      <Card title={`Versions (${vs.length})`} lede="Secrets Manager does not keep a linear history. It tracks versions by staging label." testId="versions-card"><DataTable aria-label="Versions" data={vs} columns={COLUMNS} rowId={(v) => v.id} maxHeight={420} /></Card>
      <Note testId="versions-note"><b>AWSCURRENT</b> is what readers get by default, <b>AWSPREVIOUS</b> is the version it replaced, and <b>AWSPENDING</b> appears only while a rotation is running. A version with no label is deprecated and Secrets Manager can delete it; ListSecretVersionIds leaves such versions out unless IncludeDeprecated is set.</Note>
      <Card title="Roll back with the AWS CLI" lede="To roll a secret back, move the AWSCURRENT label to the previous version with update-secret-version-stage in the AWS CLI. Secrets Manager moves AWSPREVIOUS to the other version by itself." testId="rollback">
        <div className="relative"><CopyButton text={cli} label="Copy" what="Copied" iconOnly variant="ghost" className="absolute top-1.5 right-1.5 z-10" /><CodeBlock text={cli} testId="rollback-cli" /></div>
      </Card>
    </div>
  );
}

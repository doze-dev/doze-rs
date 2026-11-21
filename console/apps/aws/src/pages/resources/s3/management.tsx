// A bucket's Management tab: its lifecycle rules, and Create lifecycle rule.
import { PlusIcon } from "lucide-react";
import { type DataColumn, DataTable } from "@doze/ui/components/app/data-table";
import { Badge } from "@doze/ui/components/ui/badge";
import { Button } from "@doze/ui/components/ui/button";
import { Card } from "../deep/parts";
import { NONE, useSetting } from "./api";
import { lifecycleRules, managementSentence, type RuleView } from "./config";
import { NoneYet, Reading } from "./parts";
import type { Versioning } from "./words";

export function useManagement(name: string, enabled: boolean) {
  return useSetting(name, "GetBucketLifecycleConfiguration", NONE.lifecycle, {}, enabled);
}

export function sentenceOf(name: string, q: ReturnType<typeof useManagement>, versioning: Versioning): string | null {
  if (!q.isSuccess && !q.absent) return null;
  return managementSentence(name, lifecycleRules(q.data), versioning);
}

const COLUMNS: DataColumn<RuleView>[] = [
  { id: "name", header: "Lifecycle rule name", size: 260, min: 160, accessorKey: "name", meta: { mono: true } },
  { id: "status", header: "Status", size: 120, accessorFn: (r) => (r.enabled ? 1 : 0), cell: (c) => <Badge variant="outline" className={c.row.original.enabled ? "border-good/40 bg-good/10 text-good" : ""}>{c.row.original.enabled ? "Enabled" : "Disabled"}</Badge> },
  { id: "scope", header: "Scope", size: 200, min: 120, accessorKey: "scope" },
  { id: "actions", header: "Actions", accessorFn: (r) => r.actions.join("; "), cell: (c) => <span className="grid gap-0.5">{c.row.original.actions.map((a) => <span key={a}>{a}</span>)}</span> },
];

export function ManagementTab({ name, q, go }: { name: string; q: ReturnType<typeof useManagement>; go: (action: string) => void }) {
  const rules = lifecycleRules(q.data);
  const create = (primary: boolean) => (
    <Button variant={primary ? "default" : "outline"} className={primary ? undefined : "bg-card"} onClick={() => go("lifecycle-create")} data-testid="lifecycle-create-open">
      <PlusIcon /> Create lifecycle rule
    </Button>
  );
  return (
    <div data-testid="management">
      <Reading q={q} what="The lifecycle rules" height="h-40">
        {rules.length === 0 ? (
          <NoneYet title="No lifecycle rules" testId="lifecycle-none" action={create(false)}>
            {name} has no lifecycle rules, so objects stay until someone deletes them.
          </NoneYet>
        ) : (
          <Card title={`Lifecycle rules (${rules.length})`} lede="Use lifecycle rules to define actions you want S3 to take during an object's lifetime, such as expiring (deleting) it after a number of days." actions={create(true)}>
            <DataTable aria-label="Lifecycle rules" data={rules} columns={COLUMNS} rowId={(r) => r.name} rowHeight={64} maxHeight={520} />
          </Card>
        )}
      </Reading>
    </div>
  );
}

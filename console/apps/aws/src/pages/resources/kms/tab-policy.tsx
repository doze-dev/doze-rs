// The Key policy tab (kms__key-detail): the policy as the sections the console writes (administrators, deletion, users) when it is
// one the console wrote, else as the JSON it is; Switch to policy view shows the JSON, and Edit opens the policy's own page.
import { PencilIcon } from "lucide-react";
import { useState } from "react";
import { CodeBlock } from "@doze/ui/components/app/code-block";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Card, Facts, Note } from "../cfg-parts";
import { useKeyPolicy } from "./api";
import { policySections, principalWords } from "./model";

const chip = "rounded-full border bg-well px-2.5 py-0.5 font-mono text-[12.5px] text-bright";

/** `[larder-admin] role`, `[lambda-exec] [report-job] roles in this account`. */
export function Principals({ arns, suffix = "" }: { arns: readonly string[]; suffix?: string }) {
  if (arns.length === 0) return <span className="text-muted-foreground">None</span>;
  const words = arns.map(principalWords);
  const kinds = [...new Set(words.map((w) => w.kind))];
  const noun = kinds.length === 1 && kinds[0] !== "other" ? `${kinds[0]}${arns.length === 1 ? "" : "s"}` : "";
  return (
    <span className="flex flex-wrap items-center gap-1.5" data-testid="principals">
      {words.map((w) => <span key={w.name + w.kind} className={chip}>{w.kind === "other" || kinds.length > 1 ? `${w.kind === "other" ? "" : `${w.kind}/`}${w.name}` : w.name}</span>)}
      {noun && <span className="text-muted-foreground">{noun}{suffix}</span>}
    </span>
  );
}

export function TabPolicy({ id, editable, go }: { id: string; editable: boolean; go: () => void }) {
  const q = useKeyPolicy(id);
  const [asked, setAsked] = useState<"default" | "policy">("default");
  if (q.isError) return <ErrorBlock what="The key policy could not be read" error={q.error} onRetry={() => void q.refetch()} testId="policy-error" />;
  if (!q.data) return <Skeleton className="h-56 rounded-[var(--radius-card)]" data-testid="policy-loading" />;
  const sections = policySections(q.data.Policy);
  const view = sections ? asked : "policy";
  let pretty = q.data.Policy;
  try { pretty = JSON.stringify(JSON.parse(q.data.Policy), null, 2); } catch { /* shown as stored */ }
  return (
    <div className="grid gap-5" data-testid="policy-tab">
      <Card
        title="Key policy"
        lede={view === "default" ? "Default view. The console wrote this policy when the key was created, so it can show it as sections." : sections ? "Policy view. The JSON the key carries." : "Policy view. This policy was not written by the console, so it is shown as the JSON it is."}
        actions={
          <>
            {sections && <Button type="button" variant="outline" className="bg-card" onClick={() => setAsked(view === "default" ? "policy" : "default")} data-testid="policy-switch">{view === "default" ? "Switch to policy view" : "Switch to default view"}</Button>}
            {view === "policy" && editable && <Button type="button" variant="outline" className="bg-card" onClick={go} data-testid="policy-edit"><PencilIcon /> Edit</Button>}
          </>
        }
        testId="policy-card"
      >
        {view === "default" && sections ? (
          <Facts facts={[
            { label: "Key administrators", value: <Principals arns={sections.admins} />, testId: "fact-admins" },
            ...(sections.admins.length > 0 ? [{ label: "Key deletion", value: sections.canDelete ? "Key administrators can delete this key" : "Key administrators cannot delete this key" }] : []),
            { label: "Key users", value: <Principals arns={sections.users} suffix=" in this account" />, testId: "fact-users" },
          ]} />
        ) : (
          <CodeBlock text={pretty} max="max-h-[34rem]" testId="policy-json" />
        )}
      </Card>
      {view === "default" && <Note testId="policy-note"><b>Key administrators</b> manage the key: enable and disable it, edit this policy, turn on rotation. They cannot use it. <b>Key users</b> can encrypt, decrypt, re-encrypt and generate data keys.</Note>}
    </div>
  );
}

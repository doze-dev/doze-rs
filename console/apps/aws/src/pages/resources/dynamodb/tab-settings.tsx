// A table's Additional settings tab (dynamodb__table-settings): read/write capacity (Edit is a page), Time to Live (Turn on is a page,
// Turn off asks), encryption, deletion protection (a checkbox and Save changes) and tags.
import { PencilIcon } from "lucide-react";
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Label } from "@doze/ui/components/ui/label";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { useOp, useOpRead } from "../deep/api";
import { Card } from "../deep/parts";
import { type TableInfo, encryptionWords, modeWords } from "./model";
import { Ask, tableTarget } from "./parts";
import { SettingsTags } from "./settings-tags";
import { Facts, OnOff, WithSub } from "./tab-parts";

type Ttl = { TimeToLiveDescription?: { TimeToLiveStatus?: string; AttributeName?: string } };

const STORED = "Stored, not applied locally.";

export function SettingsTab({ t, go }: { t: TableInfo; go: (action: string) => void }) {
  const target = tableTarget(t.name);
  const ttl = useOpRead<Ttl>(target, "DescribeTimeToLive", {});
  const update = useOp<unknown>(target, "UpdateTable");
  const off = useOp<unknown>(target, "UpdateTimeToLive");
  const [protect, setProtect] = useState<boolean | null>(null);
  const [turning, setTurning] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  const status = ttl.data?.TimeToLiveDescription?.TimeToLiveStatus ?? "DISABLED";
  const attribute = ttl.data?.TimeToLiveDescription?.AttributeName;
  const enabled = status === "ENABLED" || status === "ENABLING";
  const want = protect ?? t.protect;
  const failed = update.error ? errorWords(update.error) : null;
  return (
    <div className="grid gap-6">
      {saved && <ResultBanner title={saved} onDismiss={() => setSaved(null)} testId="settings-saved" />}
      <Card title="Read/write capacity" lede="How the table is billed for reads and writes." testId="capacity">
        <div className="grid gap-4">
          <div>
            <Button variant="outline" className="bg-card" onClick={() => go("capacity-edit")} data-testid="capacity-edit-open">
              <PencilIcon /> Edit
            </Button>
          </div>
          <Facts
            rows={[
              ["Capacity mode", <WithSub key="m" sub={STORED}>{modeWords(t)}</WithSub>],
              ...(t.mode === "PROVISIONED" ? ([["Read capacity units", t.read ?? 0], ["Write capacity units", t.write ?? 0]] as [string, React.ReactNode][]) : []),
            ]}
          />
        </div>
      </Card>
      <Card title="Time to Live (TTL)" lede="Delete items automatically once a timestamp attribute has passed." testId="ttl">
        <div className="grid gap-4">
          <div>
            {enabled ? (
              <Button variant="outline" className="bg-card" onClick={() => setTurning(true)} data-testid="ttl-off-open">Turn off</Button>
            ) : (
              <Button variant="outline" className="bg-card" onClick={() => go("ttl-edit")} data-testid="ttl-edit-open">Turn on</Button>
            )}
          </div>
          <Facts rows={[["TimeToLiveStatus", <OnOff key="s" on={enabled} yes={status} no={status} />], ["AttributeName", attribute ? <span className="font-mono text-[13.5px]" key="a">{attribute}</span> : null]]} />
        </div>
      </Card>
      <Card title="Encryption" lede="Every table is encrypted at rest." testId="encryption">
        <Facts rows={[["Encryption type", <WithSub key="e" sub={STORED}>{encryptionWords(t.encryption)}</WithSub>]]} />
      </Card>
      <Card title="Deletion protection" lede="While on, DeleteTable is refused for everyone." testId="protection">
        <div className="grid gap-4">
          <div>
            <Button
              variant="outline"
              className="bg-card"
              disabled={want === t.protect || update.isPending}
              onClick={() => update.mutate({ DeletionProtectionEnabled: want }, { onSuccess: () => (setProtect(null), setSaved(`Deletion protection is ${want ? "on" : "off"} for ${t.name}.`)) })}
              data-testid="protection-save"
            >
              {update.isPending ? "Saving…" : "Save changes"}
            </Button>
          </div>
          <div className="flex items-start gap-3">
            <Checkbox id="protect" checked={want} onCheckedChange={(v) => setProtect(v === true)} className="mt-0.5" data-testid="protection-check" />
            <Label htmlFor="protect" className="grid gap-0.5 font-normal">
              <span className="font-medium text-bright">Enable delete protection</span>
              <span className="text-[13px] text-muted-foreground">Currently {t.protect ? "on" : "off"} for {t.name}.</span>
            </Label>
          </div>
          {failed && <Alert tone="bad" title={`UpdateTable was refused${failed.code ? `: ${failed.code}` : ""}`} testId="protection-error">{failed.message}</Alert>}
        </div>
      </Card>
      <SettingsTags name={t.name} />
      {turning && (
        <Ask
          open
          onOpenChange={(o) => !o && (setTurning(false), off.reset())}
          title="Turn off Time to Live?"
          verb="Turn off TTL"
          call="UpdateTimeToLive"
          pending={off.isPending}
          error={off.error}
          testId="ttl-off-dialog"
          onConfirm={() => off.mutate({ TimeToLiveSpecification: { Enabled: false, AttributeName: attribute ?? "" } }, { onSuccess: () => (setTurning(false), setSaved(`Time to Live is off for ${t.name}.`)) })}
        >
          <p>
            Items of <span className="font-mono">{t.name}</span> stop expiring by <span className="font-mono">{attribute}</span>.
          </p>
        </Ask>
      )}
    </div>
  );
}

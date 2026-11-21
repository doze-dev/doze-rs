// The Edit dialogs of a REST stage (apigateway__rest-stage): its details, its logs and tracing, its variables. Each is one UpdateStage with
// the patch of what changed; the service's refusal (execution logging needs the account's CloudWatch Logs role) is said in the dialog.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { filled, newRow, rowsFrom, rowsValid, type KeyValueRow } from "@doze/ui/components/app/key-values";
import { Button } from "@doze/ui/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@doze/ui/components/ui/dialog";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Switch } from "@doze/ui/components/ui/switch";
import { Field, RuleHelp, Segmented } from "../../../kit";
import { REST, type Patch, useWrite } from "./api";
import { type Details, type Logging, detailsOf, detailsPatches, levelWords, loggingOf, loggingPatches, variablesPatches } from "./stage-model";
import type { Stage } from "./types";
import { variableProblem } from "./words";

export type EditKind = "details" | "logging" | "variables";

function Shell({ open, onOpenChange, title, description, children, patches, api, stage, onDone, valid = true, testId }: { open: boolean; onOpenChange: (o: boolean) => void; title: string; description: string; children: React.ReactNode; patches: Patch[]; api: string; stage: string; onDone: () => void; valid?: boolean; testId: string }) {
  const save = useWrite(REST, "UpdateStage");
  const failed = save.error ? errorWords(save.error) : null;
  return (
    <Dialog open={open} onOpenChange={(o) => { onOpenChange(o); if (!o) save.reset(); }}>
      <DialogContent className="sm:max-w-[600px]" data-testid={testId}>
        <DialogHeader>
          <DialogTitle className="text-[21px] font-semibold">{title}</DialogTitle>
          <DialogDescription className="text-[14px] leading-[22px]">{description}</DialogDescription>
        </DialogHeader>
        <form className="grid gap-5" onSubmit={(e) => { e.preventDefault(); if (!valid || save.isPending) return; if (patches.length === 0) { onDone(); return; } save.mutate({ restApiId: api, stageName: stage, patchOperations: patches } as never, { onSuccess: onDone }); }}>
          {children}
          {failed && <Alert tone="bad" title={`UpdateStage was refused${failed.code ? `: ${failed.code}` : ""}`} testId={`${testId}-error`}>{failed.message}</Alert>}
          <DialogFooter>
            <Button type="button" variant="outline" className="bg-card" onClick={() => onOpenChange(false)}>Cancel</Button>
            <Button type="submit" disabled={!valid || save.isPending} data-testid={`${testId}-save`}>{save.isPending ? "Saving…" : "Save changes"}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

const Toggle = ({ id, label, checked, onChange, note }: { id: string; label: string; checked: boolean; onChange: (v: boolean) => void; note?: React.ReactNode }) => (
  <div className="flex items-start gap-3"><Switch id={id} checked={checked} onCheckedChange={onChange} className="mt-0.5" data-testid={id} /><div className="grid gap-0.5"><Label htmlFor={id}>{label}</Label>{note && <p className="text-[13px] text-muted-foreground">{note}</p>}</div></div>
);

export function StageEdit({ kind, stage, api, onClose }: { kind: EditKind; stage: Stage; api: string; onClose: (saved: boolean) => void }) {
  const [d, setD] = useState<Details>(detailsOf(stage));
  const [l, setL] = useState<Logging>(loggingOf(stage));
  const [rows, setRows] = useState<KeyValueRow[]>(() => [...rowsFrom(Object.entries(stage.variables ?? {}).map(([key, value]) => ({ key, value }))), newRow()]);
  const common = { open: true, onOpenChange: (o: boolean) => !o && onClose(false), api, stage: stage.stageName, onDone: () => onClose(true) };
  if (kind === "details") {
    const bad = d.throttle && (!/^\d+(\.\d+)?$/.test(d.rate) || !/^\d+$/.test(d.burst));
    return (
      <Shell {...common} title="Edit stage details" description={`Stage ${stage.stageName}. The name of a stage cannot be changed.`} patches={detailsPatches(stage, d)} valid={!bad} testId="stage-edit-details">
        <Field label="Stage description" optional>{(id) => <Input id={id} value={d.description} onChange={(e) => setD({ ...d, description: e.target.value })} autoComplete="off" data-testid="stage-description" />}</Field>
        <Toggle id="stage-cache" label="Provision API cache" checked={d.cache} onChange={(cache) => setD({ ...d, cache })} note="Stored, not applied locally: no response is cached." />
        <Toggle id="stage-throttle" label="Throttling" checked={d.throttle} onChange={(throttle) => setD({ ...d, throttle })} note="Stored, not applied locally: nothing is throttled here. A usage plan's throttle is." />
        {d.throttle && (
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Rate" error={bad ? "Enter a number." : undefined}>{(id) => <Input id={id} value={d.rate} onChange={(e) => setD({ ...d, rate: e.target.value })} inputMode="decimal" data-testid="stage-rate" />}</Field>
            <Field label="Burst">{(id) => <Input id={id} value={d.burst} onChange={(e) => setD({ ...d, burst: e.target.value })} inputMode="numeric" data-testid="stage-burst" />}</Field>
          </div>
        )}
      </Shell>
    );
  }
  if (kind === "logging") {
    const bad = l.access && (l.destination.trim() === "" || l.format.trim() === "");
    return (
      <Shell {...common} title="Edit logs and tracing" description={`Stage ${stage.stageName}.`} patches={loggingPatches(stage, l)} valid={!bad} testId="stage-edit-logging">
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">CloudWatch Logs</span>
          <Segmented<Logging["level"]> value={l.level} onChange={(level) => setL({ ...l, level })} label="CloudWatch Logs" testId="stage-level" options={(["OFF", "ERROR", "INFO"] as const).map((v) => ({ value: v, label: levelWords(v) }))} />
          <RuleHelp>Turning the execution log on needs the account&apos;s CloudWatch Logs role (UpdateAccount, as on AWS); doze-aws then writes <span className="font-mono">API-Gateway-Execution-Logs_{"<api id>"}/{stage.stageName}</span>.</RuleHelp>
        </div>
        <Toggle id="stage-trace" label="Data tracing" checked={l.dataTrace} onChange={(dataTrace) => setL({ ...l, dataTrace })} note="Writes request and response payloads to the logs; it can record sensitive data." />
        <Toggle id="stage-metrics" label="Detailed metrics" checked={l.metrics} onChange={(metrics) => setL({ ...l, metrics })} note="Stored, not applied locally: doze-aws emits no API Gateway metrics." />
        <Toggle id="stage-access" label="Custom access logging" checked={l.access} onChange={(access) => setL({ ...l, access })} />
        {l.access && (
          <>
            <Field label="Access log destination ARN">{(id) => <Input id={id} value={l.destination} onChange={(e) => setL({ ...l, destination: e.target.value })} placeholder="arn:aws:logs:us-east-1:…:log-group:/larder/api" className="font-mono" data-testid="stage-destination" />}</Field>
            <Field label="Log format">{(id) => <Input id={id} value={l.format} onChange={(e) => setL({ ...l, format: e.target.value })} placeholder="$context.requestId $context.status" className="font-mono" data-testid="stage-format" />}</Field>
          </>
        )}
      </Shell>
    );
  }
  const kept = filled(rows);
  const problem = kept.map((r) => variableProblem(r.key, r.value)).find(Boolean) ?? null;
  return (
    <Shell {...common} title="Edit stage variables" description={`Name and value pairs for stage ${stage.stageName}. A Lambda proxy integration receives them in the event.`} patches={variablesPatches(stage.variables, kept.map((r) => ({ name: r.key, value: r.value })))} valid={rowsValid(rows, { maxRows: 100 }) && problem === null} testId="stage-edit-variables">
      <KeyValueEditor rows={rows} onChange={setRows} rules={{ maxRows: 100 }} addLabel="Add variable" keyLabel="Name" valueLabel="Value" removeLabel="Remove" testId="stage-variables" />
      {problem && <p className="text-[13px] text-bad" role="alert" data-testid="variable-problem">{problem}</p>}
      <RuleHelp>Names may hold letters, digits and underscores; a value matches <span className="font-mono">[A-Za-z0-9-._~:/?#&amp;=,]+</span>.</RuleHelp>
    </Shell>
  );
}

// The edits of a stream's Configuration, each a dialog with the call it makes beside it (kinesis__stream-capacity-edit, stream-shards-edit,
// stream-retention-edit, stream-encryption-edit, stream-metrics-edit). Each setting is its own call; the service's refusal is said in place.
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { Field, RadioCards, RuleHelp, SectionEdit, callKindOp } from "../../../kit";
import { STREAMS, useStreamWrite } from "./api";
import { type Mode, METRICS, type Stream, count, daysWords, hoursWords, metricsDiff, modeWords, retentionOp, retentionProblem, shardsProblem } from "./model";

type Edit = { s: Stream; onClose: () => void };

/** Capacity mode: On-demand or Provisioned. Switching only changes the mode; the shards the stream has stay. */
export function CapacityEdit({ s, onClose }: Edit) {
  const [mode, setMode] = useState<Mode>(s.mode === "ON_DEMAND" ? "ON_DEMAND" : "PROVISIONED");
  const w = useStreamWrite("UpdateStreamMode");
  return (
    <SectionEdit open onOpenChange={(o) => !o && onClose()} title="Edit capacity mode" description={<>Choose how <b className="font-mono font-semibold text-bright">{s.name}</b> gets its capacity. It is {modeWords(s.mode)} now.</>}
      request={{ service: "kinesis", operation: "UpdateStreamMode", input: { StreamARN: s.arn, StreamModeDetails: { StreamMode: mode } } }}
      onSave={() => w.mutate({ stream: s.name, input: { StreamModeDetails: { StreamMode: mode } } }, { onSuccess: onClose })} pending={w.isPending} error={w.error} disabled={modeWords(s.mode) === modeWords(mode)} disabledWhy={`It already is ${modeWords(mode)}.`} testId="capacity-dialog">
      <RadioCards<Mode> name="cap" label="Capacity mode" value={mode} onChange={setMode} className="sm:grid-cols-1" options={[{ value: "ON_DEMAND", title: "On-demand", body: "Kinesis manages the shards." }, { value: "PROVISIONED", title: "Provisioned", body: "You manage the shard count." }]} />
      <RuleHelp>{mode === "PROVISIONED" ? `Switching keeps the ${count(s.shards, "shard")} the stream has; from then on, in provisioned mode you adjust the shard count yourself.` : `Switching keeps the ${count(s.shards, "shard")} the stream has; doze-aws does not resize them, and it throttles nothing in either mode.`}</RuleHelp>
    </SectionEdit>
  );
}

/** Provisioned shards: UpdateShardCount with UNIFORM_SCALING, the only scaling type the API accepts. */
export function ShardsEdit({ s, onClose }: Edit) {
  const [target, setTarget] = useState(String(s.shards));
  const w = useStreamWrite("UpdateShardCount");
  const problem = shardsProblem(target);
  const same = Number(target) === s.shards;
  return (
    <SectionEdit open onOpenChange={(o) => !o && onClose()} title="Edit provisioned shards" description={<>Set a new shard count for <b className="font-mono font-semibold text-bright">{s.name}</b>. It has {count(s.shards, "open shard")} now.</>}
      request={{ service: "kinesis", operation: "UpdateShardCount", input: { StreamName: s.name, TargetShardCount: Number(target) || 0, ScalingType: "UNIFORM_SCALING" } }}
      onSave={() => w.mutate({ stream: s.name, input: { TargetShardCount: Number(target), ScalingType: "UNIFORM_SCALING" } }, { onSuccess: onClose })} pending={w.isPending} error={w.error} disabled={problem !== null || same} disabledWhy={problem ?? "That is the number it has."} testId="shards-dialog">
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="grid gap-1.5"><Label htmlFor="sh-now">Current shards</Label><Input id="sh-now" value={s.shards} disabled className="bg-muted font-mono" /></div>
        <Field label="New shard count" error={problem ?? undefined}>{(id) => <Input id={id} value={target} onChange={(e) => setTarget(e.target.value)} inputMode="numeric" autoComplete="off" className="font-mono" data-testid="shard-target" />}</Field>
      </div>
      <div className="grid gap-1.5"><Label htmlFor="sh-type">Scaling type</Label><Input id="sh-type" value="UNIFORM_SCALING" disabled className="bg-muted font-mono" /><p className="text-[13px] text-muted-foreground">The only value the API accepts.</p></div>
      <RuleHelp>Uniform scaling closes every open shard and opens new ones that tile the whole hash range; each new shard names the shard that owned its starting hash. A closed shard keeps its records until the retention sweep drops them, and is reclaimed once it is empty.</RuleHelp>
      <p className="text-[13.5px] text-muted-foreground" data-testid="shards-after">Open shards <b className="text-bright">{s.shards}</b> now, <b className="text-bright">{problem ? "–" : Number(target)}</b> after</p>
    </SectionEdit>
  );
}

/** Data retention period, in hours; Increase or Decrease is chosen by which way it goes. */
export function RetentionEdit({ s, onClose }: Edit) {
  const [hours, setHours] = useState(String(s.retention));
  const problem = retentionProblem(hours);
  const n = Number(hours);
  const op = problem ? null : retentionOp(s.retention, n);
  const inc = useStreamWrite("IncreaseStreamRetentionPeriod");
  const dec = useStreamWrite("DecreaseStreamRetentionPeriod");
  const w = op === "DecreaseStreamRetentionPeriod" ? dec : inc;
  return (
    <SectionEdit open onOpenChange={(o) => !o && onClose()} title="Edit data retention period" description={<>How long records stay readable after they are added to <b className="font-mono font-semibold text-bright">{s.name}</b>. It keeps them {hoursWords(s.retention)} now.</>}
      request={{ service: "kinesis", operation: op ?? "IncreaseStreamRetentionPeriod", input: { StreamName: s.name, RetentionPeriodHours: n || 0 } }}
      onSave={() => op && w.mutate({ stream: s.name, input: { RetentionPeriodHours: n } }, { onSuccess: onClose })} pending={w.isPending} error={w.error} disabled={problem !== null || op === null} disabledWhy={problem ?? "That is the period it has."} testId="retention-dialog">
      <Field label="Data retention period" error={problem ?? undefined} hint={!problem && daysWords(n) ? `That is ${daysWords(n)}. The API takes hours (RetentionPeriodHours).` : "The API takes hours (RetentionPeriodHours)."}>
        {(id) => <div className="relative max-w-[16rem]"><Input id={id} value={hours} onChange={(e) => setHours(e.target.value)} inputMode="numeric" autoComplete="off" className="pr-16 font-mono" data-testid="retention-hours" /><span className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-muted-foreground">hours</span></div>}
      </Field>
      <RuleHelp>From <b>24 hours</b> (the default) up to <b>8,760 hours</b> (365 days). Raising it calls IncreaseStreamRetentionPeriod, lowering it DecreaseStreamRetentionPeriod. Records past the window are dropped by a background sweep that runs about once a minute, and records that already expired do not come back.</RuleHelp>
    </SectionEdit>
  );
}

/** Server-side encryption: on with a KMS key (StartStreamEncryption) or off (StopStreamEncryption). Stored, not applied. */
export function EncryptionEdit({ s, onClose }: Edit) {
  const [on, setOn] = useState(s.encryption === "KMS");
  const [key, setKey] = useState(s.kms ?? "");
  const aliases = useQuery({ queryKey: ["kinesis", "aliases"], queryFn: async () => ((await callKindOp<{ Aliases?: { AliasName: string }[] }>(STREAMS, "ListAliases", {})).Aliases ?? []).map((a) => a.AliasName).sort(), retry: false });
  const start = useStreamWrite("StartStreamEncryption");
  const stop = useStreamWrite("StopStreamEncryption");
  const was = s.encryption === "KMS";
  const w = on ? start : stop;
  const options = [...new Set([...(aliases.data ?? []), ...(s.kms ? [s.kms] : [])])];
  const request = on ? { service: "kinesis", operation: "StartStreamEncryption", input: { StreamName: s.name, EncryptionType: "KMS", KeyId: key } } : { service: "kinesis", operation: "StopStreamEncryption", input: { StreamName: s.name, EncryptionType: "KMS", KeyId: s.kms ?? key } };
  return (
    <SectionEdit open onOpenChange={(o) => !o && onClose()} title="Edit server-side encryption" description={<>Choose a KMS key to use as the master key for the encryption of <b className="font-mono font-semibold text-bright">{s.name}</b>.</>}
      request={request} onSave={() => w.mutate({ stream: s.name, input: request.input }, { onSuccess: onClose })} pending={w.isPending} error={w.error}
      disabled={(on && key === "") || (on === was && (!on || key === s.kms))} disabledWhy={on && key === "" ? "Choose a key." : "Nothing changed."} testId="encryption-dialog">
      <div className="flex items-center gap-3"><Switch id="sse" checked={on} onCheckedChange={setOn} data-testid="sse-switch" /><Label htmlFor="sse" className="font-normal">Server-side encryption {on ? "on" : "off"}</Label></div>
      {on && (
        <Field label="KMS key">
          {(id) => <Select value={key} onValueChange={setKey}><SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="sse-key"><SelectValue placeholder="Choose a key" /></SelectTrigger><SelectContent>{options.map((o) => <SelectItem key={o} value={o} className="font-mono">{o}</SelectItem>)}</SelectContent></Select>}
        </Field>
      )}
      <RuleHelp><b>Stored, not applied locally.</b> On calls StartStreamEncryption with the key, off calls StopStreamEncryption. The key id is kept and shown, but records are not encrypted with it. The key must exist and be enabled, or producers and consumers are refused.</RuleHelp>
    </SectionEdit>
  );
}

/** Shard-level metrics: the seven metric names, kept and shown. Enable and Disable are the two calls the difference needs. */
export function MetricsEdit({ s, now, onClose }: Edit & { now: string[] }) {
  const [want, setWant] = useState<string[]>(now);
  const enable = useStreamWrite("EnableEnhancedMonitoring");
  const disable = useStreamWrite("DisableEnhancedMonitoring");
  const diff = metricsDiff(now, want);
  const call = diff.enable.length > 0 ? { op: "EnableEnhancedMonitoring", names: diff.enable } : { op: "DisableEnhancedMonitoring", names: diff.disable };
  const save = () => {
    void (async () => {
      try {
        if (diff.enable.length > 0) await enable.mutateAsync({ stream: s.name, input: { ShardLevelMetrics: diff.enable } });
        if (diff.disable.length > 0) await disable.mutateAsync({ stream: s.name, input: { ShardLevelMetrics: diff.disable } });
        onClose();
      } catch {
        /* said in the dialog */
      }
    })();
  };
  return (
    <SectionEdit open onOpenChange={(o) => !o && onClose()} title="Edit shard level metrics" description={<>Choose the shard-level metrics for <b className="font-mono font-semibold text-bright">{s.name}</b>. {now.length === 0 ? "None are enabled now." : `${now.length} ${now.length === 1 ? "is" : "are"} enabled now.`}</>}
      request={{ service: "kinesis", operation: call.op, input: { StreamName: s.name, ShardLevelMetrics: call.names } }} onSave={save} pending={enable.isPending || disable.isPending} error={enable.error ?? disable.error}
      disabled={diff.enable.length + diff.disable.length === 0} disabledWhy="Nothing changed." testId="metrics-dialog">
      <div className="grid gap-3">
        {METRICS.map((m) => (
          <div key={m.name} className="flex items-start gap-3">
            <Checkbox id={`m-${m.name}`} checked={want.includes(m.name)} onCheckedChange={(v) => setWant((w) => (v === true ? [...w, m.name] : w.filter((x) => x !== m.name)))} className="mt-0.5" data-testid={`metric-${m.name}`} />
            <div className="grid gap-0.5"><Label htmlFor={`m-${m.name}`} className="font-mono text-[13.5px] font-medium text-bright">{m.name}</Label><span className="text-[13px] text-muted-foreground">{m.about}</span></div>
          </div>
        ))}
      </div>
      <RuleHelp><b>Stored, not applied locally.</b> The names are kept and shown. Nothing produces AWS/Kinesis metrics in doze-aws, so no data follows.</RuleHelp>
    </SectionEdit>
  );
}

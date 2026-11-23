// Create data stream (kinesis__stream-create, stream-create-provisioned, stream-create-refused): a page with the call it will make beside it. On-demand is
// the default; Provisioned takes a shard count, sized by an estimator from the traffic; a name in use is the service's refusal, said on the field.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, type KeyValueRow, rowsValid } from "@doze/ui/components/app/key-values";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { useResourceDetail, useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, RadioCards, RuleHelp, TAG_RULES, useKindOp, useServiceCrumbs, useTags } from "../../../kit";
import { STREAMS } from "./api";
import { type CreateForm, type Mode, capacityWords, count, createInput, emptyCreate, estimate, figure, list, nameProblem, shardsProblem, streamOf, streamRows } from "./model";

export function KinesisCreate() {
  const navigate = useNavigate();
  const base = useServiceCrumbs("kinesis");
  const [f, setF] = useState<CreateForm>(emptyCreate);
  const [rows, setRows] = useState<KeyValueRow[]>([newRow()]);
  const [est, setEst] = useState({ size: "2", rate: "3,000", apps: "2" });
  const [tried, setTried] = useState(false);
  const set = (patch: Partial<CreateForm>) => {
    setF((x) => ({ ...x, ...patch }));
    // A refusal belongs to the form it answered: a new name or another mode is a new try.
    if ("name" in patch || "mode" in patch) make.reset();
  };
  const make = useKindOp<Record<string, never>>(STREAMS, "CreateStream");
  const lister = useResourceList("kinesis", "streams");
  const about = useAbout().data;
  const existing = useMemo(() => (lister.data?.pages.flatMap((p) => p.rows) ?? []).map((r) => r.name), [lister.data]);
  const sample = existing[0] ?? "";
  const lives = useMemo(() => streamRows(lister.data?.pages.flatMap((p) => p.rows) ?? []).find((r) => r.mode === "PROVISIONED")?.name ?? "", [lister.data]);
  const livesOn = useResourceDetail("kinesis", "streams", lives, lives !== "" && f.mode === "PROVISIONED");
  const livesShards = livesOn.data ? streamOf(lives, livesOn.data.fields).shards : null;
  const sampleTags = useTags({ service: "kinesis", kind: "streams", name: sample }, sample !== "");
  const estimated = estimate(figure(est.size), figure(est.rate), figure(est.apps));
  const input = useMemo(() => createInput({ ...f, tags: rows }), [f, rows]);
  const problems = { name: nameProblem(f.name), shards: f.mode === "PROVISIONED" ? shardsProblem(f.shards) : null };
  const bad = Object.values(problems).find(Boolean) ?? (rowsValid(rows, TAG_RULES) ? null : "Fix the tags first.");
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : make.error ? { code: "Error", message: String((make.error as Error).message) } : null;
  const inUse = refused?.code === "ResourceInUseException";
  const submit = () => {
    setTried(true);
    if (bad) return;
    make.mutate(input, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "kinesis", kind: "streams", name: f.name }, search: { done: "created" } }) });
  };
  const shards = Number(f.shards) || 0;
  const tagNames = (sampleTags.data?.tags ?? []).map((t) => t.key);
  return (
    <FormPage
      title="Create data stream"
      sub={f.mode === "PROVISIONED" ? <>With the provisioned mode you set the number of shards. <span className="text-muted-foreground">The estimator below sizes it from your traffic.</span></> : <>Name the stream and choose how its capacity is managed. <span className="text-muted-foreground">On-demand is selected by default.</span></>}
      crumbs={[...base, { label: "Data streams", to: "/resources/kinesis/streams" }, ...(f.mode === "PROVISIONED" ? [{ label: "Create data stream: Provisioned", to: "/resources/kinesis/streams?create=yes" }] : [])]}
      request={f.name ? { service: "kinesis", operation: "CreateStream", input } : null}
      refusal={refused}
      callNote={inUse ? "A duplicate name is refused with ResourceInUseException." : f.mode === "PROVISIONED" ? "A provisioned stream is created with the shard count you give." : "CreateStream names the stream, its capacity mode and its tags."}
      banner={refused ? <Alert tone="bad" title="The stream was not created." testId="create-refused">CreateStream answered {refused.code}: {refused.message} {inUse ? "Choose another name." : ""}</Alert> : undefined}
      submitLabel="Create data stream"
      pending={make.isPending}
      disabled={tried && bad !== null}
      disabledWhy={bad ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "kinesis", second: "streams" }, search: {} })}
      onSubmit={submit}
      testId="stream-create"
    >
      <FormSection title="Data stream name" testId="section-name">
        <Field label="Name" error={tried ? (problems.name ?? undefined) : undefined} refusal={inUse && refused ? { why: `${refused.code}: ${refused.message}` } : null} help={<>1 to <b>128</b> characters: letters, digits, underscores, hyphens and periods. The name is unique in your account and Region.</>} hint={existing.length > 0 ? `${list(existing.slice(0, 2))} already ${existing.slice(0, 2).length === 1 ? "exists" : "exist"} in ${about?.region ?? "this Region"}.` : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="my-data-stream" autoComplete="off" spellCheck={false} className="font-mono" data-testid="stream-name" />}
        </Field>
      </FormSection>
      <FormSection title="Data stream capacity" description="The capacity mode decides how throughput is provided." testId="section-capacity">
        <RadioCards<Mode> name="mode" label="Capacity mode" value={f.mode} onChange={(mode) => set({ mode, ...(mode === "PROVISIONED" && estimated ? { shards: String(estimated.shards) } : {}) })} options={[{ value: "ON_DEMAND", title: "On-demand", body: "No capacity planning. Kinesis manages the shards for you." }, { value: "PROVISIONED", title: "Provisioned", body: "You choose the number of shards and scale them yourself." }]} />
        {f.mode === "ON_DEMAND" ? (
          <RuleHelp>An on-demand stream starts with <b>4 shards</b> in doze-aws. Nothing throttles here, so capacity never limits a write.</RuleHelp>
        ) : (
          <Field label="Number of shards" error={tried ? (problems.shards ?? undefined) : undefined} help={<>The capacity of the stream is the sum of its shards. In AWS each shard takes writes of up to <b>1 MiB per second</b> and <b>1,000 records per second</b> and serves reads of up to <b>2 MiB per second</b>, so {shards > 0 ? capacityWords(shards) : "N shards is N MiB per second in"}. <span className="text-muted-foreground">doze-aws does not throttle, so these limits are for sizing only.</span>{lives !== "" && livesShards !== null ? <> <b className="font-mono font-semibold text-bright">{lives}</b> runs on {count(livesShards, "shard")}.</> : null}</>}>
            {(id) => <Input id={id} value={f.shards} onChange={(e) => set({ shards: e.target.value })} inputMode="numeric" autoComplete="off" className="max-w-[12rem]" data-testid="stream-shards" />}
          </Field>
        )}
      </FormSection>
      {f.mode === "PROVISIONED" && (
        <FormSection title="Estimate the number of shards you'll need" description="Enter the average record size, the maximum records written per second and the number of consuming applications." collapsible defaultOpen testId="section-estimate">
          <div className="grid gap-5 md:grid-cols-2">
            {([["size", "Average record size", "KB"], ["rate", "Max records written", "per second"]] as const).map(([k, label, unit]) => (
              <div key={k} className="grid gap-1.5">
                <Label htmlFor={`est-${k}`}>{label}</Label>
                <div className="relative"><Input id={`est-${k}`} value={est[k]} onChange={(e) => { const next = { ...est, [k]: e.target.value }; setEst(next); const e2 = estimate(figure(next.size), figure(next.rate), figure(next.apps)); if (e2) set({ shards: String(e2.shards) }); }} inputMode="numeric" autoComplete="off" className="pr-24" data-testid={`est-${k}`} /><span className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-muted-foreground">{unit}</span></div>
              </div>
            ))}
            <div className="grid gap-1.5">
              <Label htmlFor="est-apps">Number of consuming applications</Label>
              <Input id="est-apps" value={est.apps} onChange={(e) => { const next = { ...est, apps: e.target.value }; setEst(next); const e2 = estimate(figure(next.size), figure(next.rate), figure(next.apps)); if (e2) set({ shards: String(e2.shards) }); }} inputMode="numeric" autoComplete="off" data-testid="est-apps" />
            </div>
          </div>
          <div className="grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] gap-y-1 text-[13.5px]" data-testid="est-result">
            <span className="text-muted-foreground">Estimated number of shards</span>
            <span><b className="text-bright" data-testid="est-shards">{estimated ? estimated.shards : "–"}</b><br /><span className="text-[12.5px] text-muted-foreground">{estimated ? estimated.formula : "Enter all three figures."}</span></span>
          </div>
          <RuleHelp>The formula is ceiling(max(incoming write bandwidth in KiB / 1,024, outgoing read bandwidth in KiB / 2,048)), where the outgoing read bandwidth is the incoming write bandwidth times the number of consuming applications. The average record size is rounded up to the nearest 1 KB.</RuleHelp>
        </FormSection>
      )}
      <FormSection title="Tags" description="Tags are labels you define to organise and track costs. They are added when the stream is created." testId="section-tags">
        <KeyValueEditor rows={rows} onChange={setRows} rules={TAG_RULES} showErrors={tried} removeLabel="Remove" testId="form-tags" />
        <RuleHelp>A tag has a key and an optional value; keys and values are case-sensitive.{sample && tagNames.length > 0 ? <> <span className="text-muted-foreground">{sample} carries {list(tagNames)}.</span></> : null}</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

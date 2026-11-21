// Create event bus (eventbridge__bus-create, bus-create-refused): the name, encryption and a dead-letter queue, an archive made with it, and tags,
// with the calls it will make beside it. A refusal keeps what was typed, says so on the name, and shows what came back.
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Combobox } from "@doze/ui/components/app/combobox";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { newRow, rowsValid } from "@doze/ui/components/app/key-values";
import { Input } from "@doze/ui/components/ui/input";
import { Switch } from "@doze/ui/components/ui/switch";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceDetail } from "../../../api/resources";
import { Field, FormPage, FormSection, LocalStatus, QueuePicker, RadioCards, RuleHelp, TAG_RULES, callKindOp, useServiceCrumbs } from "../../../kit";
import { BUSES, busArn, useBuses, useEbOp } from "./api";
import { type BusForm, archiveNameProblem, archiveSummary, busNameProblem, createArchiveInput, createBusInput, defaultArchiveName, emptyBus, encryptionSummary, tagsSummary } from "./bus-model";
import { useWhere, when } from "./parts";
import { list } from "./words";

function useAliases() {
  return useQuery({ queryKey: ["eventbridge", "aliases"], queryFn: async () => ((await callKindOp<{ Aliases?: { AliasName: string }[] }>(BUSES, "ListAliases", {})).Aliases ?? []).map((a) => a.AliasName).sort(), retry: false });
}

export function BusCreate() {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const { region, account } = useWhere();
  const [f, setF] = useState<BusForm>({ ...emptyBus, tags: [newRow()] });
  const [tried, setTried] = useState(false);
  const make = useEbOp<{ EventBusArn: string }>(BUSES, "CreateEventBus");
  const archive = useEbOp(BUSES, "CreateArchive");
  const buses = useBuses();
  const aliases = useAliases();
  const key = useResourceDetail("kms", "keys", f.kms ?? "", f.cmk && (f.kms ?? "").startsWith("alias/"));
  const fact = (label: string) => key.data?.fields.find((x) => x.label === label)?.value;
  const set = (patch: Partial<BusForm>) => {
    setF((x) => ({ ...x, ...patch }));
    if ("name" in patch) make.reset();
  };
  const taken = (buses.data?.EventBuses ?? []).map((b) => b.Name);
  const mine = taken.filter((n) => n !== "default");
  const problem = busNameProblem(f.name) ?? (f.archive ? archiveNameProblem(f.archiveName) : null) ?? (f.cmk && !f.kms ? "Choose the key." : null) ?? (rowsValid(f.tags, TAG_RULES) ? null : "Fix the tags first.");
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : make.error ? { code: "Error", message: String((make.error as Error).message) } : null;
  const exists = refused?.code === "ResourceAlreadyExistsException";
  const input = useMemo(() => createBusInput(f), [f]);
  const requests = [
    { service: "events", operation: "CreateEventBus", input },
    ...(f.archive ? [{ service: "events", operation: "CreateArchive", input: createArchiveInput(f, busArn(region, account, f.name || "<name>")) }] : []),
  ];
  const submit = () => {
    setTried(true);
    if (problem) return;
    make.mutate(input as Record<string, never>, {
      onSuccess: (made) => {
        const done = () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "eventbridge", kind: "buses", name: f.name }, search: { done: "created" } });
        if (f.archive) archive.mutate(createArchiveInput(f, made.EventBusArn) as Record<string, never>, { onSuccess: done });
        else done();
      },
    });
  };
  const existing = buses.data?.EventBuses?.find((b) => b.Name === f.name);
  const collapsed = refused !== null;
  const stage = archive.error ? "The bus was created, but its archive was not." : "The event bus wasn't created.";
  const failure = refused ?? (archive.error instanceof ApiFailure ? { code: archive.error.code, message: archive.error.message } : null);
  return (
    <FormPage
      title="Create event bus"
      sub="Name the bus, then set what it should do. Everything below the name is optional and can be changed later."
      crumbs={[...crumbs, { label: "Event buses", to: "/resources/eventbridge/buses" }]}
      request={requests[0] ?? null}
      requests={requests}
      refusal={failure}
      callNote={failure ? <>The call failed{archive.error ? " after the bus was made" : " before anything was created"}, so {archive.error ? "the archive was not made" : <><b className="font-semibold text-foreground">CreateArchive was never sent</b></>}. What you typed is kept.</> : <>The archive is a separate call, made after the bus exists. Empty sections are left out.</>}
      banner={failure ? <Alert tone="bad" title={stage} testId="create-refused">{archive.error ? "CreateArchive" : "CreateEventBus"} answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create"
      pending={make.isPending || archive.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "buses" }, search: {} })}
      onSubmit={submit}
      testId="bus-create"
    >
      <FormSection title="Event bus details" testId="section-details">
        <Field label="Name" error={tried ? (busNameProblem(f.name) ?? undefined) : undefined} refusal={exists ? { why: <>A bus named <b className="font-semibold">{f.name}</b> already exists.</>, suggestion: "Pick another name." } : null} help={<><b>1 to 256</b> characters: letters, digits, dot, hyphen and underscore. A custom bus name can't contain / and can't be <span className="font-mono">default</span>.</>} hint={exists && existing ? <><b className="font-mono font-semibold text-foreground">{f.name}</b> was created at {when(existing.CreationTime)}; {list(taken.filter((n) => n !== f.name).map((n) => n))} {taken.length - 1 === 1 ? "is" : "are"} taken too.</> : mine.length > 0 ? <>{list(mine.slice(0, 3))} already {mine.length === 1 ? "exists" : "exist"} in {region}.</> : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value, ...(f.archiveName === defaultArchiveName(f.name) || f.archiveName === "" ? { archiveName: defaultArchiveName(e.target.value) } : {}) })} placeholder="larder-audit" autoComplete="off" spellCheck={false} className="font-mono" data-testid="bus-name" />}
        </Field>
        <Field label="Description" optional>{(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="bus-description" />}</Field>
      </FormSection>
      <FormSection key={`enc-${collapsed}`} title="Encryption" description="Which KMS key protects the events stored on this bus." collapsible={collapsed} defaultOpen={!collapsed} summary={encryptionSummary(f)} testId="section-encryption">
        <RadioCards<"aws" | "cmk"> name="enc" label="Encryption key" value={f.cmk ? "cmk" : "aws"} onChange={(v) => set({ cmk: v === "cmk" })} options={[{ value: "aws", title: "Use AWS owned key", body: "EventBridge owns and manages the key. This is the default." }, { value: "cmk", title: "Use customer managed key", body: "A KMS key in your account, which you control." }]} />
        <div className={f.cmk ? "grid items-start gap-4 md:grid-cols-2" : "grid gap-4"}>
        {f.cmk && (
          <Field label="Customer managed key" testId="field-kms" hint={f.kms ? <>Key status: <b className="font-semibold text-foreground">{fact("State") ?? "reading"}</b>. Alias <span className="font-mono">{f.kms}</span>.</> : undefined}>
            {(id) => <Combobox id={id} options={(aliases.data ?? []).map((a) => ({ value: a, label: a }))} value={f.kms} onChange={(kms) => set({ kms })} placeholder={aliases.isPending ? "Reading keys…" : "Choose a key"} search="Search keys…" empty="No key alias matches." aria-label="Customer managed key" className="w-full" />}
          </Field>
        )}
        <Field label="Dead-letter queue" optional testId="field-dlq">{(id) => <QueuePicker id={id} by="name" value={f.dlq ? f.dlq.split(":").pop()! : null} onChange={(n) => set({ dlq: n ? `arn:aws:sqs:${region}:${account}:${n}` : null })} />}</Field>
        </div>
        <LocalStatus state="stored" reason={<>On AWS, events that can't be encrypted go to the dead-letter queue, if you set one. doze-aws keeps both settings and does not encrypt anything, and parks nothing in the queue (<span className="font-mono">crates/aws-eventbridge/src/buses.rs</span>).</>} />
      </FormSection>
      <FormSection key={`arc-${collapsed}`} title="Archives" optional description="Keep events so you can replay them later." collapsible={collapsed} defaultOpen={!collapsed} summary={archiveSummary(f)} testId="section-archives">
        <div className="flex items-start gap-3">
          <Switch id="archive-on" checked={f.archive} onCheckedChange={(archive) => set({ archive, archiveName: f.archiveName || defaultArchiveName(f.name) })} className="mt-0.5" data-testid="archive-on" />
          <div className="grid gap-0.5"><Label htmlFor="archive-on">Enabled</Label></div>
        </div>
        {f.archive && (
          <>
            <Field label="Archive name" error={tried ? (archiveNameProblem(f.archiveName) ?? undefined) : undefined}>{(id) => <Input id={id} value={f.archiveName} onChange={(e) => set({ archiveName: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="archive-name" />}</Field>
            <Field label="Description" optional>{(id) => <Input id={id} value={f.archiveDescription} onChange={(e) => set({ archiveDescription: e.target.value })} autoComplete="off" data-testid="archive-description" />}</Field>
            <RuleHelp>The name can't be changed afterwards. An archive made here keeps events indefinitely and has no filter; set retention and an event pattern on the archive later.</RuleHelp>
          </>
        )}
      </FormSection>
      <FormSection key={`tags-${collapsed}`} title="Tags" optional description="Labels that help you find and organise the bus." collapsible={collapsed} defaultOpen={!collapsed} summary={tagsSummary(f)} testId="section-tags">
        <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={tried} removeLabel="Remove" testId="form-tags" />
        <RuleHelp>Up to <b>50</b> tags per resource. A key can be <b>128</b> characters and a value <b>256</b>; keys are case-sensitive and can't start with <span className="font-mono">aws:</span>.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

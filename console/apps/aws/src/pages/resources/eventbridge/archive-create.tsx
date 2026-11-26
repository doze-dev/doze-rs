// Create archive (eventbridge__archive-create): its name, the bus it takes events from, how long it keeps them, and an optional pattern, with the call it makes.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { BusPicker, Field, FormPage, FormSection, RadioCards, RuleHelp, useServiceCrumbs } from "../../../kit";
import { ARCHIVES, busArn, useArchives, useEbOp } from "./api";
import { type ArchiveForm, archiveProblem, createArchiveInput, daysProblem, emptyArchive, nameProblem } from "./archive-model";
import { JsonBox } from "./json-box";
import { useWhere } from "./parts";
import { busOfArn, list } from "./words";

export function ArchiveCreate({ bus }: { bus?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const { region, account } = useWhere();
  const [f, setF] = useState<ArchiveForm>(emptyArchive(bus ?? "default"));
  const [tried, setTried] = useState(false);
  const make = useEbOp(ARCHIVES, "CreateArchive");
  const all = useArchives().data?.Archives ?? [];
  const taken = all.map((a) => a.ArchiveName);
  const onBus = all.filter((a) => busOfArn(a.EventSourceArn) === f.bus).map((a) => a.ArchiveName);
  const problem = archiveProblem(f, taken);
  const set = (p: Partial<ArchiveForm>) => { setF((x) => ({ ...x, ...p })); make.reset(); };
  const input = useMemo(() => createArchiveInput(f, busArn(region, account, f.bus)), [f, region, account]);
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  return (
    <FormPage
      title="Create archive"
      sub="Define the archive, then optionally filter what goes into it. AWS shows these as two steps, Define the archive and Build the event pattern (optional); here they sit on one page."
      crumbs={[...crumbs, { label: "Archives", to: "/resources/eventbridge/archives" }]}
      request={{ service: "events", operation: "CreateArchive", input }}
      refusal={refused}
      callNote="Leave RetentionDays out, or send 0, to keep events indefinitely."
      banner={refused ? <Alert tone="bad" title="The archive wasn't created." testId="create-refused">CreateArchive answered <span className="font-mono font-semibold text-bright">{refused.code}</span>: “{refused.message}”</Alert> : undefined}
      submitLabel="Create archive"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "archives" }, search: {} })}
      onSubmit={() => {
        setTried(true);
        if (!problem) make.mutate(input as never, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "eventbridge", kind: "archives", name: f.name }, search: { done: "created" } }) });
      }}
      testId="archive-create"
    >
      <FormSection title="Archive detail" testId="section-detail">
        <Field label="Name" error={tried ? (nameProblem(f.name) ?? (taken.includes(f.name) ? `${f.name} is already an archive.` : undefined) ?? undefined) : undefined} help={<><b>1 to 48</b> characters: letters, digits, dot, hyphen and underscore. Unique in the account and Region, and it can't be changed later.</>}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="orders-30d" autoComplete="off" spellCheck={false} className="font-mono" data-testid="archive-name" />}
        </Field>
        <Field label="Description" optional>{(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="archive-description" />}</Field>
        <Field label="Source" testId="field-source" help="The bus the archive takes events from. It can't be changed once the archive exists, and replays go back to this same bus." hint={onBus.length > 0 ? <><b className="font-mono font-semibold text-foreground">{f.bus}</b> already has {list(onBus)}.</> : undefined}>
          {(id) => <BusPicker id={id} value={f.bus} onChange={(b) => b && set({ bus: b })} />}
        </Field>
        <div className="grid gap-3" data-testid="field-retention">
          <p className="text-[14px] font-medium text-bright">Retention period</p>
          <RadioCards<"indefinite" | "days"> name="retention" label="Retention period" value={f.indefinite ? "indefinite" : "days"} onChange={(v) => set({ indefinite: v === "indefinite" })} options={[{ value: "indefinite", title: "Indefinite", body: "Never delete events." }, { value: "days", title: "Set a retention period", body: "Enter the number of days after which EventBridge deletes the events." }]} />
          {!f.indefinite && (
            <Field label="Days" error={tried ? (daysProblem(f) ?? undefined) : undefined}>{(id) => <div className="relative max-w-[14rem]"><Input id={id} value={f.days} onChange={(e) => set({ days: e.target.value })} inputMode="numeric" className="pr-14" data-testid="archive-days" /><span className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-[13px] text-muted-foreground">days</span></div>}</Field>
          )}
          <RuleHelp><b>Applied here:</b> doze-aws removes the events older than the retention when the next event arrives, at most once an hour.</RuleHelp>
        </div>
      </FormSection>
      <FormSection title="Event pattern" optional description="Only events that match are archived. Leave it empty to archive every event on the bus." testId="section-pattern">
        <JsonBox value={f.filter ? f.pattern : ""} onChange={(pattern) => set({ pattern, filter: pattern.trim() !== "" })} label="Event pattern" rows={7} placeholder={emptyArchive("").pattern} testId="archive-pattern" />
        <RuleHelp>Same pattern language as a rule. Events that are replays never enter an archive.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

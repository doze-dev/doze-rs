// Start new replay (eventbridge__replay-start): an archive, the bus it goes back to, all rules or some, and a window of event time (UTC).
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Combobox } from "@doze/ui/components/app/combobox";
import { UtcDateTimePicker } from "@doze/ui/components/app/utc-datetime-picker";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, RadioCards, RuleHelp, useKindOp, useServiceCrumbs } from "../../../kit";
import { REPLAYS, archiveArn, ruleArn, useArchives, useReplays, useRulesOf } from "./api";
import { State, useWhere } from "./parts";
import { emptyReplay, nameProblem, replayProblem, startReplayInput, type ReplayForm } from "./replay-model";
import { busOfArn, count, list } from "./words";

export function ReplayStart({ archive }: { archive?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const { region, account } = useWhere();
  const [f, setF] = useState<ReplayForm>(() => emptyReplay(archive ?? null));
  const [tried, setTried] = useState(false);
  const start = useKindOp(REPLAYS, "StartReplay");
  const archives = useArchives().data?.Archives ?? [];
  const replays = useReplays().data?.Replays ?? [];
  const chosen = archives.find((a) => a.ArchiveName === f.archive);
  const bus = chosen ? busOfArn(chosen.EventSourceArn) : null;
  const rules = useRulesOf(bus ?? "default", bus !== null).data?.Rules ?? [];
  const set = (p: Partial<ReplayForm>) => { setF((x) => ({ ...x, ...p })); start.reset(); };
  const taken = replays.map((r) => r.ReplayName);
  const ran = replays.filter((r) => r.ReplayName !== f.name).sort((a, b) => (b.ReplayStartTime ?? 0) - (a.ReplayStartTime ?? 0))[0];
  const problem = replayProblem(f, taken);
  const input = useMemo(() => (chosen && bus ? startReplayInput(f, archiveArn(region, account, chosen.ArchiveName), chosen.EventSourceArn, (r) => ruleArn(region, account, bus, r)) : startReplayInput(f, archiveArn(region, account, f.archive ?? "<archive>"), "<bus>", (r) => r)), [f, chosen, bus, region, account]);
  const refused = start.error instanceof ApiFailure ? { code: start.error.code, message: start.error.message } : null;
  return (
    <FormPage
      title="Start new replay"
      sub="Pick an archive and a window of event time. Events in the window are sent to the bus the archive came from."
      crumbs={[...crumbs, { label: "Replays", to: "/resources/eventbridge/replays" }]}
      request={{ service: "events", operation: "StartReplay", input }}
      refusal={refused}
      callNote="A replay runs in the background here as on AWS: the call returns while it is Starting, the replay is Running as its events go back to the bus, and Completed when the window is done. It can be cancelled until then."
      banner={refused ? <Alert tone="bad" title="The replay wasn't started." testId="start-refused">StartReplay answered <span className="font-mono font-semibold text-bright">{refused.code}</span>: “{refused.message}”</Alert> : undefined}
      submitLabel="Start replay"
      pending={start.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "replays" }, search: {} })}
      onSubmit={() => {
        setTried(true);
        if (!problem) start.mutate(input as never, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "eventbridge", kind: "replays", name: f.name }, search: {} }) });
      }}
      testId="replay-start"
    >
      <FormSection title="Replay detail" testId="section-detail">
        <Field label="Name" error={tried ? (nameProblem(f.name) ?? (taken.includes(f.name) ? `${f.name} already ran.` : undefined) ?? undefined) : undefined} help={<><b>1 to 64</b> characters: letters, digits, dot, hyphen and underscore.</>} hint={ran ? <><b className="font-mono font-semibold text-foreground">{ran.ReplayName}</b> ran already.</> : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="payments-redo" autoComplete="off" spellCheck={false} className="font-mono" data-testid="replay-name" />}
        </Field>
        <Field label="Description" optional>{(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="replay-description" />}</Field>
        <Field label="Source" testId="field-archive" error={tried && !f.archive ? "Choose the archive to replay." : undefined} hint={chosen ? <><b className="font-mono font-semibold text-foreground">{chosen.ArchiveName}</b> holds {count(chosen.EventCount ?? 0, "event")}.</> : undefined}>
          {(id) => <Combobox id={id} options={archives.map((a) => ({ value: a.ArchiveName, label: a.ArchiveName, hint: busOfArn(a.EventSourceArn) }))} value={f.archive} onChange={(archive) => set({ archive, rules: [] })} placeholder={archives.length === 0 ? "There are no archives yet" : "Choose an archive"} search="Search archives…" empty="No archive matches." aria-label="Source archive" className="w-full" />}
        </Field>
        <Field label="Destination" testId="field-destination" help="Events can only go back to the bus that emitted them, so the destination is fixed by the archive.">
          {(id) => <Input id={id} value={bus ?? ""} readOnly placeholder="Chosen with the archive" className="bg-muted font-mono text-muted-foreground" data-testid="replay-destination" />}
        </Field>
        <div className="grid gap-3" data-testid="field-rules">
          <p className="text-[14px] font-medium text-bright">Which rules receive the replayed events.</p>
          <RadioCards<"all" | "some"> name="rules" label="Which rules" value={f.all ? "all" : "some"} onChange={(v) => set({ all: v === "all" })} options={[{ value: "all", title: "All rules", body: bus ? `Every rule on ${bus} that matches.` : "Every rule on the bus that matches." }, { value: "some", title: "Specify rules", body: "Only the rules you pick." }]} />
          {!f.all && (
            <ul className="grid gap-2" data-testid="rule-list">
              {rules.length === 0 && <li className="text-[13px] text-muted-foreground">{bus ? `${bus} has no rules.` : "Choose an archive first."}</li>}
              {rules.map((r) => (
                <li key={r.Name} className="flex items-center gap-3 rounded-lg border bg-card px-3 py-2">
                  <Checkbox id={`rule-${r.Name}`} checked={f.rules.includes(r.Name)} onCheckedChange={(on) => set({ rules: on ? [...f.rules, r.Name] : f.rules.filter((x) => x !== r.Name) })} data-testid={`rule-${r.Name}`} />
                  <Label htmlFor={`rule-${r.Name}`} className="font-mono text-[13px] font-medium">{r.Name}</Label>
                  <span className="ml-auto"><State state={r.State} /></span>
                </li>
              ))}
            </ul>
          )}
          <RuleHelp>Each replayed event carries a <span className="font-mono">replay-name</span> field, so targets can tell a replay from live traffic.{rules.length > 0 && !f.all ? ` ${list(rules.map((r) => r.Name))} are on ${bus}.` : ""}</RuleHelp>
        </div>
      </FormSection>
      <FormSection title="Replay time frame" description="Only events that happened between the start and end time are replayed. The time is the time inside the event." testId="section-window">
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="grid gap-1.5"><Label htmlFor="replay-start">Start time</Label><UtcDateTimePicker id="replay-start" value={f.startMs} onChange={(startMs) => set({ startMs })} aria-label="Start time, UTC" /></div>
          <div className="grid gap-1.5"><Label htmlFor="replay-end">End time</Label><UtcDateTimePicker id="replay-end" value={f.endMs} onChange={(endMs) => set({ endMs })} aria-label="End time, UTC" /></div>
        </div>
        {tried && f.endMs !== null && f.startMs !== null && f.endMs <= f.startMs && <p className="text-[13px] text-bad" role="alert">The end must be after the start.</p>}
        <RuleHelp>On AWS, events can reach an archive a few minutes late, so wait about 10 minutes before replaying recent ones. A replay does not remove events from the archive; you can replay the same window again.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

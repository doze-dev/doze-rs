// The write dialogs of a group (boards group-reset-offsets, group-delete, group-offset-delete and their refused states): each
// is an overlay over the page it was opened from, routed by `?dialog=`. Reset offsets previews with a GET (it reads
// OffsetFetch and ListOffsets and writes nothing) and sends exactly the choice the preview shows. A refusal stays in the
// dialog in Kafka's own name and words, with the console's reading: the rule, what would pass, and the call refused.
import { useNavigate } from "@tanstack/react-router";
import { EyeIcon, RotateCcwIcon, Trash2Icon } from "lucide-react";
import { useEffect, useState } from "react";
import { Controller, useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { useDeleteGroup, useDeleteOffsets, useForgetGroup, useDeletePreview, useOffsetsPreview, useResetOffsets, useResetPreview } from "@/api/groups";
import type { GroupHead } from "@/api/types/GroupHead";
import type { GroupWriteOutcome } from "@/api/types/GroupWriteOutcome";

type Refusal = Extract<GroupWriteOutcome, { outcome: "refused" }>;
type Invalid = Extract<GroupWriteOutcome, { outcome: "invalid" }>;
import type { Lag } from "@/api/types/Lag";
import type { ResetRequest } from "@/api/types/ResetRequest";
import { UtcDateTimePicker } from "@/components/app/utc-datetime-picker";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { number } from "@/lib/format";
import { Done, DoneFooter } from "@/components/app/write-done";
import { CallBlock, Callout, GroupSpans, RefusalCallout, Rules } from "./parts";
import { partList, plural } from "./words";

type Dialog = "reset" | "delete" | "delete-offsets";

function membersNames(h: GroupHead): string {
  return h.members.length === 1 ? `${h.members[0]!.id} is a member.` : `${h.members.length} members are in it.`;
}

function refusalOf(o: GroupWriteOutcome | undefined): Refusal | null {
  return o?.outcome === "refused" ? o : null;
}

function invalidOf(o: GroupWriteOutcome | undefined): Invalid | null {
  return o?.outcome === "invalid" ? o : null;
}

/** What Kafka refused, as the classic refused page has it: the refusal, what would pass, the call. */
function Refused({ r, tail }: { r: Refusal; tail?: string }) {
  const g = r.refused;
  return (
    <div className="flex flex-col gap-4" data-testid="refused">
      <RefusalCallout code={r.code} number={r.number} message={r.message} rule={g && g.rule.length > 0 ? <GroupSpans spans={g.rule} /> : undefined} />
      {g && g.steps.length > 0 && (
        <div className="rounded-xl border bg-card p-4">
          <h3 className="mb-3 text-[14.5px] font-semibold text-bright">What would pass</h3>
          <Rules rows={g.steps.map((s) => ({ blocking: s.blocking, title: s.title, sub: s.sub, value: s.value }))} />
        </div>
      )}
      {g && <CallBlock lines={g.call} title="The call that was refused" />}
      {tail && <p className="text-[13px] text-muted-foreground">{tail}</p>}
    </div>
  );
}

function lagAfter(l: Lag): string {
  switch (l.kind) {
    case "behind":
      return number(l.n);
    case "caught_up":
      return "0";
    case "past_end":
      return `${number(l.n)} past the end`;
    default:
      return "—";
  }
}

const MODES = [
  { value: "earliest", title: "Earliest", sub: "The log start of each partition, asked of ListOffsets" },
  { value: "latest", title: "Latest", sub: "The log end of each partition, asked of ListOffsets" },
  { value: "offset", title: "A specific offset", sub: "One offset for every partition, kept within each log" },
  { value: "time", title: "A time", sub: "The first offset at or after it; the log end where there is none" },
  { value: "shift", title: "Shift by", sub: "Back (negative) or forward from where each partition is" },
  { value: "current", title: "Where it is now", sub: "Each committed offset, kept within its log" },
] as const;

type ResetForm = { topic: string; to: string; value: string; time: number | null };

function asRequest(f: ResetForm): ResetRequest {
  return { topic: f.topic, to: f.to, value: f.to === "time" ? (f.time === null ? "" : String(f.time)) : f.to === "offset" || f.to === "shift" ? f.value.trim() : "" };
}

function ResetDialog({ id, head, onDone }: { id: string; head: GroupHead; onDone: () => void }) {
  const form = useForm<ResetForm>({ defaultValues: { topic: "", to: "latest", value: "", time: null } });
  const [asked, setAsked] = useState<ResetRequest>({ topic: "", to: "latest", value: "" });
  const preview = useResetPreview(id, asked, true);
  const reset = useResetOffsets(id);
  const v = preview.data;
  // The first answer names the topic the dialog starts on (the first the group committed on).
  useEffect(() => {
    if (v && !form.getValues("topic") && v.topic) form.setValue("topic", v.topic);
  }, [v, form]);
  const watched = useWatch({ control: form.control });
  const chosen = asRequest({ topic: watched.topic ?? "", to: watched.to ?? "latest", value: watched.value ?? "", time: watched.time ?? null });
  const same = v !== undefined && chosen.topic === (asked.topic || v.topic) && chosen.to === asked.to && chosen.value === asked.value && !preview.isFetching;
  const moves = v && same && !v.error ? v.rows.filter((r) => r.after !== null).length : 0;
  const refusal = refusalOf(reset.data);
  const invalid = invalidOf(reset.data);
  const to = watched.to ?? "latest";
  const doPreview = form.handleSubmit((f) => {
    reset.reset();
    setAsked(asRequest(f));
  });
  const [done, setDone] = useState<{ n: number; topic: string } | null>(null);
  const send = () =>
    reset.mutate(asked.topic ? asked : { ...asked, topic: v?.topic ?? "" }, {
      onSuccess: (d) => {
        if (d.outcome !== "done") return;
        setDone({ n: d.done, topic: asked.topic || (v?.topic ?? "") });
      },
    });
  const shownTopic = v?.topic ?? "";
  if (done) {
    return (
      <>
        <DialogHeader>
          <DialogTitle>Reset offsets of {id}</DialogTitle>
          <DialogDescription>The broker took the commit.</DialogDescription>
        </DialogHeader>
        <Done testId="reset-done">
          <b className="font-semibold text-bright">Reset {plural(done.n, "offset")} on {done.topic}.</b> The group reads from them the next time a member starts.
        </Done>
        <DoneFooter onDone={onDone} />
      </>
    );
  }
  return (
    <>
      <DialogHeader>
        <DialogTitle>Reset offsets of {id}</DialogTitle>
        <DialogDescription>Moves where {id} will read next. Nothing is read or deleted; only the committed offsets change.</DialogDescription>
      </DialogHeader>
      <div className="flex max-h-[min(70vh,44rem)] flex-col gap-4 overflow-y-auto pr-1" data-testid="reset-body">
        {head.members.length === 0 ? (
          <Callout tone="ok">
            <b className="font-semibold text-bright">{id} has no members,</b> so Kafka accepts a commit with no generation. With a member in the group it would be refused.
          </Callout>
        ) : (
          <Callout tone="warn" data-testid="reset-not-empty">
            <b className="font-semibold text-bright">This group is not empty.</b> {membersNames(head)} A reset is a commit with no generation, which Kafka accepts only while nobody is in the group, so it will refuse with UNKNOWN_MEMBER_ID. Stop the app first, or try it to see the answer.
          </Callout>
        )}
        {v && v.topics.length === 0 ? (
          <p className="text-[13.5px] text-muted-foreground">{id} has committed no offset, so there is nothing to move. The first commit from its app makes a row here.</p>
        ) : (
          <form onSubmit={doPreview} className="flex flex-col gap-4" id="reset-form">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="reset-topic">Topic</Label>
              <Controller
                control={form.control}
                name="topic"
                render={({ field }) => (
                  <Select value={field.value || shownTopic} onValueChange={field.onChange}>
                    <SelectTrigger id="reset-topic" className="w-full bg-card font-mono" data-testid="reset-topic">
                      <SelectValue placeholder="Topic" />
                    </SelectTrigger>
                    <SelectContent>
                      {(v?.topics ?? []).map((t) => (
                        <SelectItem key={t} value={t} className="font-mono">
                          {t}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label id="reset-to-label">Reset to</Label>
              <Controller
                control={form.control}
                name="to"
                render={({ field }) => (
                  <RadioGroup value={field.value} onValueChange={field.onChange} aria-labelledby="reset-to-label" className="grid gap-1 sm:grid-cols-2" data-testid="reset-mode">
                    {MODES.map((m) => (
                      <label key={m.value} htmlFor={`reset-to-${m.value}`} className="flex cursor-pointer items-start gap-3 rounded-lg border border-transparent p-2.5 hover:bg-accent/50 has-[[data-state=checked]]:border-primary/30 has-[[data-state=checked]]:bg-accent/60">
                        <RadioGroupItem value={m.value} id={`reset-to-${m.value}`} className="mt-0.5" />
                        <span className="min-w-0">
                          <span className="block text-[13.5px] font-medium text-bright">{m.title}</span>
                          <span className="block text-[12px] text-muted-foreground">{m.sub}</span>
                        </span>
                      </label>
                    ))}
                  </RadioGroup>
                )}
              />
            </div>
            {/* The field of the mode that takes a value, in a slot of reserved height: choosing a mode moves nothing below it. */}
            <div className="min-h-[62px]">
              {(to === "offset" || to === "shift") && (
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="reset-value">{to === "offset" ? "Offset" : "Records (negative goes back)"}</Label>
                  <Input id="reset-value" {...form.register("value")} inputMode="numeric" autoComplete="off" spellCheck={false} placeholder={to === "offset" ? "16104" : "-500"} className="w-56 bg-card font-mono" data-testid="reset-value" />
                </div>
              )}
              {to === "time" && (
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="reset-time">Time, UTC</Label>
                  <Controller control={form.control} name="time" render={({ field }) => <UtcDateTimePicker id="reset-time" value={field.value} onChange={field.onChange} />} />
                </div>
              )}
            </div>
          </form>
        )}
        {v?.error && same && (
          <p role="alert" className="text-[13px] text-bad" data-testid="reset-error">
            {v.error}
          </p>
        )}
        {invalid && (
          <p role="alert" className="text-[13px] text-bad">
            {invalid.message}
          </p>
        )}
        {v && v.topics.length > 0 && (
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between">
              <span className="text-[11.5px] font-medium tracking-[0.06em] text-muted-foreground uppercase">Preview</span>
              {!same && <span className="text-[12px] text-warn">The choice changed: preview it again before resetting.</span>}
            </div>
            <div className="overflow-hidden rounded-xl border bg-card">
              <table className="w-full table-fixed text-[13px]" data-testid="reset-preview" aria-label="Preview">
                <thead className="bg-well text-[11.5px] tracking-[0.04em] text-muted-foreground uppercase">
                  <tr>
                    <th className="h-9 px-3 text-left font-medium">Partition</th>
                    <th className="h-9 px-3 text-right font-medium">Committed now</th>
                    <th className="h-9 px-3 text-right font-medium">After</th>
                    <th className="h-9 px-3 text-right font-medium">Lag after</th>
                  </tr>
                </thead>
                <tbody className={same ? undefined : "opacity-60"}>
                  {v.rows.map((r) => (
                    <tr key={r.partition} className="border-t" data-row={`${v.topic}-${r.partition}`}>
                      <td className="h-9 truncate px-3 font-mono">
                        {v.topic}-{r.partition}
                      </td>
                      <td className="h-9 px-3 text-right font-mono tabular-nums">{r.committed === null ? <span className="text-muted-foreground">none</span> : number(r.committed)}</td>
                      <td className="h-9 px-3 text-right font-mono text-bright tabular-nums" data-testid="reset-after">
                        {r.after === null ? <span className="text-muted-foreground">stays</span> : number(r.after)}
                      </td>
                      <td className="h-9 px-3 text-right font-mono tabular-nums">{lagAfter(r.lag_after)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <CallBlock lines={v.call} />
          </div>
        )}
        {!v && <Skeleton className="h-64 rounded-xl" />}
        {refusal && <Refused r={refusal} />}
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={onDone.bind(null)} type="button" data-testid="dialog-cancel">
          {refusal ? "Close" : "Cancel"}
        </Button>
        <Button variant="outline" type="submit" form="reset-form" disabled={!v || v.topics.length === 0} data-testid="reset-preview-button">
          <EyeIcon /> Preview
        </Button>
        <Button onClick={send} disabled={moves === 0 || reset.isPending} data-testid="reset-submit">
          <RotateCcwIcon /> Reset {plural(moves, "offset")}
        </Button>
      </DialogFooter>
    </>
  );
}

function DeleteDialog({ id, head, onDone, onDeleted }: { id: string; head: GroupHead; onDone: () => void; onDeleted: () => void }) {
  const preview = useDeletePreview(id, true);
  const del = useDeleteGroup(id);
  const v = preview.data;
  const n = v ? v.offsets.reduce((a, [, ps]) => a + ps.length, 0) : 0;
  const named = v ? v.offsets.map(([t, ps]) => partList(ps.map((p) => ({ topic: t, partition: p })))) : [];
  const refusal = refusalOf(del.data);
  return (
    <>
      <DialogHeader>
        <DialogTitle>Delete {id}?</DialogTitle>
        <DialogDescription>
          Deleting a group removes the group and its {v ? plural(n, "committed offset") : "committed offsets"}
          {n > 0 && `: ${named.join(", ")}`}. The app would start again from its <code className="font-mono">auto.offset.reset</code>.
        </DialogDescription>
      </DialogHeader>
      <div className="flex max-h-[min(70vh,40rem)] flex-col gap-4 overflow-y-auto pr-1">
        {head.members.length > 0 && !refusal && (
          <Callout tone="warn" data-testid="delete-not-empty">
            <b className="font-semibold text-bright">This group is not empty.</b> {membersNames(head)} DeleteGroups only accepts an empty group, so Kafka will refuse with NON_EMPTY_GROUP. Stop the app first, or try it to see the answer.
          </Callout>
        )}
        {v && !refusal && <CallBlock lines={v.call} />}
        {refusal && <Refused r={refusal} tail="When no member is left the group is Empty and the same call deletes it." />}
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={onDone} data-testid="dialog-cancel">
          {refusal ? "Close" : "Cancel"}
        </Button>
        {!refusal && (
          <Button
            variant="destructive"
            disabled={!v || del.isPending}
            data-testid="delete-submit"
            onClick={() =>
              del.mutate(undefined, {
                onSuccess: (d) => {
                  if (d.outcome !== "done") return;
                  toast.success(`Deleted ${id}.`);
                  onDeleted();
                },
              })
            }
          >
            <Trash2Icon /> Delete group
          </Button>
        )}
      </DialogFooter>
    </>
  );
}

function OffsetsDialog({ id, head, onDone }: { id: string; head: GroupHead; onDone: () => void }) {
  const preview = useOffsetsPreview(id, true);
  const del = useDeleteOffsets(id);
  const v = preview.data;
  const [picked, setPicked] = useState<string[] | null>(null);
  const chosen = picked ?? v?.chosen ?? [];
  const n = v ? v.topics.filter(([t]) => chosen.includes(t)).reduce((a, [, ps]) => a + ps.length, 0) : 0;
  const refusal = refusalOf(del.data);
  const toggle = (t: string) => {
    del.reset();
    setPicked(chosen.includes(t) ? chosen.filter((x) => x !== t) : [...chosen, t]);
  };
  const [done, setDone] = useState<number | null>(null);
  if (done !== null) {
    return (
      <>
        <DialogHeader>
          <DialogTitle>Delete committed offsets of {id}?</DialogTitle>
          <DialogDescription>The broker took the delete.</DialogDescription>
        </DialogHeader>
        <Done testId="offsets-done">
          <b className="font-semibold text-bright">Deleted {plural(done, "committed offset")}.</b> A consumer that joins {id} starts at its <code className="font-mono">auto.offset.reset</code> on them.
        </Done>
        <DoneFooter onDone={onDone} />
      </>
    );
  }
  return (
    <>
      <DialogHeader>
        <DialogTitle>Delete committed offsets of {id}?</DialogTitle>
        <DialogDescription>
          Forgets the offsets this group committed on a topic. The group's next consumer starts from its <code className="font-mono">auto.offset.reset</code>. Records are not touched.
        </DialogDescription>
      </DialogHeader>
      <div className="flex max-h-[min(70vh,40rem)] flex-col gap-4 overflow-y-auto pr-1">
        {!v ? (
          <Skeleton className="h-32 rounded-xl" />
        ) : v.topics.length === 0 ? (
          <p className="text-[13.5px] text-muted-foreground">{id} has committed no offset.</p>
        ) : (
          <fieldset className="flex flex-col gap-1">
            <legend className="mb-2 text-sm font-medium">Topic</legend>
            {v.topics.map(([t, ps]) => (
              // The checkbox and its label are siblings (shadcn's pattern): a label wrapping the control toggled it twice.
              <div key={t} className="flex items-start gap-3 rounded-lg p-2.5 hover:bg-accent/50">
                <Checkbox id={`od-${t}`} checked={chosen.includes(t)} onCheckedChange={() => toggle(t)} className="mt-0.5" data-testid="offsets-topic" />
                <label htmlFor={`od-${t}`} className="min-w-0 flex-1 cursor-pointer" data-testid="offsets-topic-label">
                  <span className="block font-mono text-[13.5px] text-bright">{t}</span>
                  <span className="block truncate text-[12px] text-muted-foreground">
                    {plural(ps.length, "offset")}: {ps.slice(0, 4).map((p) => `${t}-${p}`).join(", ")}
                    {ps.length > 4 ? ", ..." : ""}
                  </span>
                </label>
              </div>
            ))}
          </fieldset>
        )}
        <Callout>
          <b className="font-semibold text-bright">Why Kafka may refuse.</b> A topic a member is subscribed to is refused with GROUP_SUBSCRIBED_TO_TOPIC (86).{" "}
          {head.members.length === 0 ? `${id} has no members, so nothing here is.` : `${id} has members, so the topics they read are.`}
        </Callout>
        {refusal && <RefusalCallout code={refusal.code} number={refusal.number} message={refusal.message} />}
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={onDone} data-testid="dialog-cancel">
          Cancel
        </Button>
        <Button
          variant="destructive"
          disabled={n === 0 || del.isPending}
          data-testid="offsets-submit"
          onClick={() =>
            del.mutate(
              { topics: chosen },
              {
                onSuccess: (d) => {
                  if (d.outcome !== "done") return;
                  setDone(d.done);
                },
              },
            )
          }
        >
          <Trash2Icon /> Delete {plural(n, "offset")}
        </Button>
      </DialogFooter>
    </>
  );
}

/** The dialog the address names, over whichever tab of the group is showing. */
export function GroupDialogs({ id, head, dialog, onClose }: { id: string; head: GroupHead; dialog?: Dialog; onClose: () => void }) {
  const navigate = useNavigate();
  const forget = useForgetGroup();
  return (
    <Dialog open={!!dialog} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[640px]" data-testid={dialog ? `dialog-${dialog}` : undefined} aria-describedby={undefined}>
        {dialog === "reset" && (
          <ResetDialog
            id={id}
            head={head}
            onDone={() => {
              onClose();
            }}
          />
        )}
        {dialog === "delete" && <DeleteDialog id={id} head={head} onDone={onClose} onDeleted={() => void navigate({ to: "/groups", replace: true }).then(() => forget(id))} />}
        {dialog === "delete-offsets" && <OffsetsDialog id={id} head={head} onDone={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

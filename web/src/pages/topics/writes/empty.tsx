// Empty a topic from its page (CON-1590, CON-1591): every partition's records up to its high watermark in one DeleteRecords,
// as kafka-delete-records.sh sends it. The preview is the server's: what goes per partition, the groups whose commits fall
// below the new start (each can be moved to it with the same write), what stays; the name typed to confirm; Kafka's refusal
// before the button when the topic only compacts; the result in the same dialog.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { EraserIcon, TriangleAlertIcon } from "lucide-react";
import { useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import type { EmptyRecordsForm } from "@/api/types/EmptyRecordsForm";
import type { RecordsEmptiedAnswer } from "@/api/types/RecordsEmptiedAnswer";
import { afterWrite, detailOf, post, topicPath, useEmptyForm, writeKeys } from "@/api/topic-writes";
import { DoneFooter } from "@/components/app/write-done";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { DialogClose } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { bytes } from "@/lib/kafka";
import { number, plural } from "@/lib/format";
import { CallBlock, Done, FieldError, RefusalCallout, StickyFooter } from "./parts";

function Parts({ f }: { f: EmptyRecordsForm }) {
  return (
    <div className="max-h-56 overflow-auto rounded-xl border bg-card" data-testid="empty-parts">
      <table className="w-full table-fixed text-[13px]">
        <colgroup>
          <col />
          <col style={{ width: "6.5rem" }} />
          <col style={{ width: "7rem" }} />
          <col style={{ width: "7rem" }} />
        </colgroup>
        <thead className="sticky top-0 bg-well text-left text-[11.5px] tracking-[0.04em] text-muted-foreground uppercase">
          <tr>
            <th className="h-9 px-3 font-medium">Partition</th>
            <th className="px-3 text-right font-medium">Records</th>
            <th className="px-3 text-right font-medium">New start</th>
            <th className="px-3 text-right font-medium">On disk</th>
          </tr>
        </thead>
        <tbody className="font-mono text-[12.5px] tabular-nums">
          {f.view.parts.map((p) => (
            <tr key={p.partition} className="border-t" data-testid="empty-part">
              <td className="h-9 truncate px-3 whitespace-nowrap" title={`${f.view.topic}-${p.partition}`}>
                {f.view.topic}-{p.partition}
              </td>
              <td className={p.records ? "px-3 text-right text-bright" : "px-3 text-right text-muted-foreground"}>{number(p.records)}</td>
              <td className="px-3 text-right" title={`log start ${number(p.log_start)} now`}>
                {number(p.high_watermark)}
              </td>
              <td className="px-3 text-right text-muted-foreground">{p.bytes === null ? "–" : bytes(p.bytes)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Groups({ f, chosen, toggle }: { f: EmptyRecordsForm; chosen: string[]; toggle: (g: string) => void }) {
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-warn/30 bg-warn/5 p-4" data-testid="empty-groups">
      <div className="flex gap-3 text-[13px] leading-[20px]">
        <TriangleAlertIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-warn" />
        <p className="min-w-0 break-words text-muted-foreground">
          <Spans spans={f.groups_warning} />
        </p>
      </div>
      <fieldset className="flex flex-col gap-2 pl-8">
        <legend className="mb-1 text-[12.5px] font-medium text-bright">Also move to the new start</legend>
        {f.view.groups.map((g) => {
          const id = `eg-${g.group}`;
          return (
            <div key={g.group} className="flex items-center gap-2.5">
              <Checkbox id={id} checked={chosen.includes(g.group)} onCheckedChange={() => toggle(g.group)} data-testid="empty-group" data-group={g.group} />
              <Label htmlFor={id} className="font-mono text-[12.5px] font-normal">
                {g.group}
              </Label>
              <span className="text-[12px] text-muted-foreground">{g.members > 0 ? `${plural(g.members, "member")}: Kafka refuses to move a group with members` : "no members"}</span>
            </div>
          );
        })}
      </fieldset>
    </div>
  );
}

function Emptied({ a }: { a: RecordsEmptiedAnswer["emptied"] }) {
  return (
    <div className="flex flex-col gap-4">
      <Done testId="emptied">
        <b className="font-semibold text-bright">
          {a.topic} is empty: <span data-testid="emptied-records">{number(a.records)}</span> {a.records === 1 ? "record is" : "records are"} no longer served.
        </b>{" "}
        Each partition starts where it ended: {a.starts.map(([p, o]) => `${a.topic}-${p} at ${number(o)}`).join(", ")}.
      </Done>
      {a.moved.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-[13px]" data-testid="emptied-moved">
          {a.moved.map((m) => (
            <li key={m.group} className="flex gap-2">
              <code className="font-mono">{m.group}</code>
              <span className={m.refused ? "text-refused" : "text-muted-foreground"}>{m.refused ? `not moved: ${m.refused}` : `moved to the new start (${plural(m.moved, "partition")})`}</span>
            </li>
          ))}
        </ul>
      )}
      <DoneFooter />
    </div>
  );
}

export function EmptyTopicBody({ topic, open }: { topic: string; open: boolean }) {
  const qc = useQueryClient();
  const form = useForm<{ confirm: string }>({ defaultValues: { confirm: "" } });
  const typed = useWatch({ control: form.control, name: "confirm" });
  const [chosen, setChosen] = useState<string[] | null>(null);
  const preview = useEmptyForm(topic, open);
  const send = useMutation({
    mutationFn: (v: { confirm: string; groups: string[] }) => post<RecordsEmptiedAnswer>(topicPath(topic, "/empty"), [["confirm", v.confirm], ...v.groups.map((g) => ["group", g] as [string, string])]),
    onSuccess: () => {
      // The preview of this topic is now another (nothing to remove): opened again, the dialog asks the server again.
      qc.removeQueries({ queryKey: writeKeys.form("empty", topic, "") });
      void afterWrite(qc);
    },
  });
  const failed = detailOf<EmptyRecordsForm>(send.error);
  const f = failed ?? preview.data;
  if (send.data) return <Emptied a={send.data.emptied} />;
  // Every group below the new start is ticked to move until the person unticks it: a consumer told OFFSET_OUT_OF_RANGE is
  // the surprise this saves them.
  const groups = chosen ?? (f?.view.groups.filter((g) => g.members === 0).map((g) => g.group) ?? []);
  const toggle = (g: string) => setChosen(groups.includes(g) ? groups.filter((x) => x !== g) : [...groups, g]);
  const blocked = !f || f.view.records === 0 || (!!f.refusal && !failed);
  return (
    <form className="flex flex-col gap-5" noValidate onSubmit={form.handleSubmit((x) => send.mutate({ confirm: x.confirm, groups }))} data-testid="empty-form">
      <p className="text-[14px] leading-[22px] text-foreground" data-testid="empty-what">
        {f ? <Spans spans={f.summary} /> : " "}
      </p>
      {f && <Parts f={f} />}
      {f && f.view.groups.length > 0 && <Groups f={f} chosen={groups} toggle={toggle} />}
      {f?.refusal && (
        <div className="flex flex-col gap-2">
          <RefusalCallout r={f.refusal} />
          <p className="text-[13px] text-muted-foreground" data-testid="empty-refused">
            {f.refused}{" "}
            <Link to="/data" search={{ dialog: "empty", topic }} className="font-medium text-link hover:underline">
              Empty it on the Data page
            </Link>
          </p>
        </div>
      )}
      {f && !f.refusal && (
        <ul className="flex list-disc flex-col gap-1 pl-5 text-[12.5px] text-muted-foreground" data-testid="empty-keeps">
          {f.keeps.map((k) => (
            <li key={k}>{k}</li>
          ))}
        </ul>
      )}
      <div className="flex flex-col gap-2">
        <Label htmlFor="ec">
          Type <code className="font-mono">{topic}</code> to confirm
        </Label>
        <Input id="ec" className="font-mono" autoComplete="off" spellCheck={false} disabled={blocked} aria-invalid={!!failed?.view.error} {...form.register("confirm", { onChange: () => send.reset() })} data-testid="empty-confirm" />
        <FieldError>{failed?.view.error ?? undefined}</FieldError>
      </div>
      {f && <CallBlock lines={f.view.call} />}
      {send.error && !failed && <FieldError>{send.error.message}</FieldError>}
      <StickyFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button type="submit" variant="destructive" disabled={blocked || typed !== topic || send.isPending} data-testid="empty-submit">
          <EraserIcon /> {f?.button ?? "Empty"}
        </Button>
      </StickyFooter>
    </form>
  );
}

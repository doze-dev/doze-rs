// Add partitions (boards topic-add-partitions, -refused) and Delete records (topic-delete-records, -refused): the count or
// the offset, a preview the server words as you type (what it adds, what it removes and who is below it), the call, and in
// the same dialog the result or Kafka's refusal.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { TriangleAlertIcon } from "lucide-react";
import { useEffect } from "react";
import { Controller, useForm, useWatch } from "react-hook-form";
import type { AddForm } from "@/api/types/AddForm";
import type { AddedAnswer } from "@/api/types/AddedAnswer";
import type { RecordsDeletedAnswer } from "@/api/types/RecordsDeletedAnswer";
import type { RecordsForm } from "@/api/types/RecordsForm";
import type { RecordsRefusedAnswer } from "@/api/types/RecordsRefusedAnswer";
import { afterWrite, detailOf, post, queryOf, topicPath, useAddForm, useDebounced, useRecordsForm } from "@/api/topic-writes";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { DialogClose } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { number } from "@/lib/format";
import { CallBlock, Done, FieldError, RefusalCallout, TeachLines, Teaching, StickyFooter } from "./parts";
import { DoneFooter } from "@/components/app/write-done";

function Footer({ children }: { children: React.ReactNode }) {
  return (
    <StickyFooter>
      <DialogClose asChild>
        <Button type="button" variant="outline">
          Cancel
        </Button>
      </DialogClose>
      {children}
    </StickyFooter>
  );
}


export function AddPartitionsBody({ topic, open }: { topic: string; open: boolean }) {
  const qc = useQueryClient();
  const form = useForm<{ count: string }>({ defaultValues: { count: "" } });
  const count = useWatch({ control: form.control, name: "count" });
  const q = useDebounced(queryOf({ count }), 200);
  const preview = useAddForm(topic, q, open);
  const first = preview.data;
  // The field starts at the server's suggestion (one more than now) the first time the dialog is answered.
  const suggested = first?.view.count;
  useEffect(() => {
    if (suggested !== undefined && form.getValues("count") === "" && !form.formState.isDirty) form.setValue("count", suggested);
  }, [suggested, form]);
  const send = useMutation({
    mutationFn: (v: { count: string }) => post<AddedAnswer>(topicPath(topic, "/add-partitions"), v),
    onSuccess: () => void afterWrite(qc),
  });
  const failed = detailOf<AddForm>(send.error);
  const shown = failed ?? preview.data;
  const v = shown?.view;
  if (send.data) {
    const a = send.data;
    return (
      <div className="flex flex-col gap-4">
        <Done testId="added">
          <b className="font-semibold text-bright">
            {a.topic} has <span data-testid="added-to">{a.to}</span> partitions now.
          </b>{" "}
          It had {a.from}. The new ones start empty; producers and consumers see them in their next Metadata.
        </Done>
        <DoneFooter />
      </div>
    );
  }
  return (
    <form className="flex flex-col gap-5" noValidate onSubmit={form.handleSubmit((x) => send.mutate(x))} data-testid="add-form">
      <div className="flex flex-col gap-2">
        <Label htmlFor="ac">
          Partitions after <span className="font-normal text-muted-foreground">now {v?.have ?? "…"}</span>
        </Label>
        <div className="relative">
          <Input id="ac" inputMode="numeric" className="pr-56 font-mono" autoComplete="off" aria-invalid={!!(failed?.view.error ?? v?.error)} {...form.register("count", { onChange: () => send.reset() })} data-testid="count" />
          <span className="pointer-events-none absolute inset-y-0 right-3 flex max-w-52 items-center truncate font-mono text-[12px] text-muted-foreground" data-testid="adds">
            {shown?.adds ?? ""}
          </span>
        </div>
        <FieldError>{failed?.view.error ?? undefined}</FieldError>
      </div>
      {shown && !failed?.view.refusal && (
        <Teaching defaultOpen>
          <TeachLines t={shown.teach} />
          <p className="text-[12.5px] text-muted-foreground">Replica assignment is ignored: this broker is the only replica. Never fewer: to get fewer partitions, create a new topic.</p>
        </Teaching>
      )}
      {failed?.view.refusal && <RefusalCallout r={failed.view.refusal} />}
      {failed?.view.answer ? <CallBlock title="The answer" lines={failed.view.answer} testId="the-answer" /> : v && <CallBlock lines={v.call} />}
      {send.error && !failed && <FieldError>{send.error.message}</FieldError>}
      <Footer>
        <Button type="submit" disabled={send.isPending} data-testid="add-submit">
          {shown?.button ?? "Add partitions"}
        </Button>
      </Footer>
    </form>
  );
}

type RecordsValues = { partition: string; offset: string };

export function DeleteRecordsBody({ topic, partition, open }: { topic: string; partition?: number; open: boolean }) {
  const qc = useQueryClient();
  const form = useForm<RecordsValues>({ defaultValues: { partition: String(partition ?? 0), offset: "" } });
  const watched = useWatch({ control: form.control }) as RecordsValues;
  const q = useDebounced(queryOf({ partition: watched.partition, offset: watched.offset }), 200);
  const preview = useRecordsForm(topic, q, open);
  const send = useMutation({
    mutationFn: (v: RecordsValues) => post<RecordsDeletedAnswer>(topicPath(topic, "/delete-records"), v),
    onSuccess: () => void afterWrite(qc),
  });
  const refused = detailOf<RecordsRefusedAnswer>(send.error);
  const invalid = send.error && !refused?.refused ? detailOf<RecordsForm>(send.error) : undefined;
  const shown = invalid ?? preview.data;
  const v = shown?.view;
  if (send.data) {
    const a = send.data;
    return (
      <div className="flex flex-col gap-4">
        <Done testId="records-deleted">
          <b className="font-semibold text-bright">
            The log of {`${a.topic}-${a.partition}`} starts at <span data-testid="low-watermark">{number(a.low_watermark)}</span> now.
          </b>{" "}
          The records before it are no longer served. It is kept through a restart.
        </Done>
        <DoneFooter />
      </div>
    );
  }
  return (
    <form className="flex flex-col gap-5" noValidate onSubmit={form.handleSubmit((x) => send.mutate({ ...x, offset: x.offset || String(v?.log_start ?? "") }))} data-testid="records-form">
      <div className="grid gap-4 sm:grid-cols-[10rem_1fr]">
        <div className="flex min-w-0 flex-col gap-2">
          <Label htmlFor="rp">Partition</Label>
          <Controller
            control={form.control}
            name="partition"
            render={({ field }) => (
              <Select
                value={field.value}
                onValueChange={(x) => {
                  field.onChange(x);
                  form.setValue("offset", "");
                  send.reset();
                }}
              >
                <SelectTrigger id="rp" className="w-full font-mono" data-testid="records-partition">
                  <SelectValue>{field.value}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {(shown?.partitions ?? [Number(field.value)]).map((p) => (
                    <SelectItem key={p} value={String(p)} className="font-mono">
                      {p}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          />
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          <Label htmlFor="ro">Delete everything before offset</Label>
          <div className="relative">
            <Input id="ro" inputMode="numeric" className="pr-52 font-mono" autoComplete="off" placeholder={v ? String(v.log_start) : ""} aria-invalid={!!invalid?.view.error} {...form.register("offset", { onChange: () => send.reset() })} data-testid="offset" />
            <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center font-mono text-[12px] text-muted-foreground tabular-nums">{v ? `start ${number(v.log_start)} · end ${number(v.high_watermark)}` : ""}</span>
          </div>
          <FieldError>{invalid?.view.error ?? undefined}</FieldError>
        </div>
      </div>
      {shown && (
        <div className="flex gap-3 rounded-xl border border-warn/30 bg-warn/5 p-4 text-[13px] leading-[20px]" data-testid="records-preview">
          <TriangleAlertIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-warn" />
          <p className="min-w-0 break-words text-muted-foreground">
            <Spans spans={shown.warning} />
          </p>
        </div>
      )}
      {shown && (
        <Teaching>
          <TeachLines t={shown.teach} />
          <p className="text-[12.5px] text-muted-foreground">On a topic that only compacts this is refused (POLICY_VIOLATION).</p>
        </Teaching>
      )}
      {refused && (
        <div className="flex flex-col gap-3" data-testid="records-refused">
          <p className="text-[13.5px] text-foreground">{refused.sentence}</p>
          <RefusalCallout r={refused.refused.refusal} />
          <CallBlock title="The answer" lines={refused.refused.answer} testId="the-answer" />
        </div>
      )}
      {!refused && v && <CallBlock lines={v.call} />}
      {send.error && !refused && !invalid && <FieldError>{send.error.message}</FieldError>}
      <Footer>
        <Button type="submit" variant="destructive" disabled={send.isPending || !shown} data-testid="records-submit">
          {shown?.button ?? "Delete records"}
        </Button>
      </Footer>
    </form>
  );
}

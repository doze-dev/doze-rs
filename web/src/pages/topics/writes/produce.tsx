// Produce to a topic (boards topic-produce, topic-produce-keyless, topic-produced): one record, the call as it will be
// sent, and in the same dialog what the broker answered: the offset it chose, or its refusal in Kafka's words.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { PlusIcon, SendIcon, XIcon } from "lucide-react";
import { useMemo } from "react";
import { Controller, useFieldArray, useForm, useWatch } from "react-hook-form";
import type { ProduceRefusedAnswer } from "@/api/types/ProduceRefusedAnswer";
import type { ProducedAnswer } from "@/api/types/ProducedAnswer";
import type { ProduceView } from "@/api/types/ProduceView";
import { afterWrite, detailOf, post, queryOf, topicPath, useDebounced, useProduceForm } from "@/api/topic-writes";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { DialogClose } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { number } from "@/lib/format";
import { CallBlock, Done, FieldError, RefusalCallout, TeachLines, Teaching, StickyFooter } from "./parts";

const MOST_HEADERS = 8;
const AUTO = "auto";

type Values = { partition: string; key: string; value: string; headers: { name: string; value: string }[]; acks: "-1" | "1" | "0" };

function fieldsOf(v: Values): Record<string, string> {
  const out: Record<string, string> = { partition: v.partition === AUTO ? "" : v.partition, key: v.key, value: v.value, acks: v.acks };
  v.headers
    .filter((h) => h.name !== "" || h.value !== "")
    .slice(0, MOST_HEADERS)
    .forEach((h, i) => {
      out[`hn${i}`] = h.name;
      out[`hv${i}`] = h.value;
    });
  return out;
}

/** What the value is, as JSON: nothing for plain text, a tick for JSON, the parser's words for JSON that is broken. */
function jsonHint(value: string): { ok: boolean; text: string } | null {
  const t = value.trim();
  if (!t || !/^[[{"]/.test(t)) return null;
  try {
    JSON.parse(t);
    return { ok: true, text: "Valid JSON. The broker stores the bytes as typed." };
  } catch (e) {
    return { ok: false, text: `Not valid JSON (${(e as Error).message}). It is sent as typed: Kafka does not read values.` };
  }
}

function Result({ a, onAnother }: { a: ProducedAnswer; onAnother: () => void }) {
  const p = a.produced;
  return (
    <div className="flex flex-col gap-4">
      <Done testId="produced">
        <b className="font-semibold text-bright">
          Sent to <code className="font-mono">{`${p.topic}-${p.partition}`}</code> at offset <span data-testid="produced-offset">{number(p.offset)}</span>.
        </b>{" "}
        Timestamp {a.clock} UTC, {a.acked}.
      </Done>
      <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 rounded-xl border p-4 text-[13px]">
        <dt className="text-muted-foreground">Partition</dt>
        <dd className="font-mono" data-testid="produced-partition">
          {p.partition}
        </dd>
        <dt className="text-muted-foreground">Offset</dt>
        <dd className="font-mono">{number(p.offset)}</dd>
        <dt className="text-muted-foreground">Key</dt>
        <dd className="truncate font-mono">{p.row.key || <span className="text-muted-foreground">none</span>}</dd>
        <dt className="text-muted-foreground">Value</dt>
        <dd className="truncate font-mono">{p.row.value || <span className="text-muted-foreground">none</span>}</dd>
        <dt className="text-muted-foreground">For read_committed</dt>
        <dd>{p.row.committed ? "Committed" : "Above the last stable offset"}</dd>
      </dl>
      <div className="rounded-xl border p-4 text-[13px] leading-[20px]">
        <h3 className="font-semibold text-bright">Is it visible?</h3>
        <p className="mt-1 text-foreground">{a.visible_lead}</p>
        <p className="mt-1 text-muted-foreground">{a.visible}</p>
      </div>
      <CallBlock title="The answer" lines={p.answer} testId="the-answer" note="The log chose the offset; the producer only supplies the record. -1 means the topic keeps the producer's time (CreateTime)." />
      <StickyFooter>
        <Button variant="outline" onClick={onAnother} data-testid="produce-another">
          Produce another
        </Button>
        <DialogClose asChild>
          <Button>Done</Button>
        </DialogClose>
      </StickyFooter>
    </div>
  );
}

export function ProduceBody({ topic, open }: { topic: string; open: boolean }) {
  const qc = useQueryClient();
  const form = useForm<Values>({ defaultValues: { partition: AUTO, key: "", value: "", headers: [{ name: "", value: "" }], acks: "-1" } });
  const headers = useFieldArray({ control: form.control, name: "headers" });
  const watched = useWatch({ control: form.control }) as Values;
  const q = useDebounced(queryOf(fieldsOf({ ...form.getValues(), ...watched })), 250);
  const preview = useProduceForm(topic, q, open);
  const send = useMutation({
    mutationFn: (v: Values) => post<ProducedAnswer>(topicPath(topic, "/produce"), fieldsOf(v)),
    onSuccess: () => void afterWrite(qc),
  });
  const refused = detailOf<ProduceRefusedAnswer>(send.error);
  const invalid = send.error && !refused?.refused ? detailOf<ProduceView>(send.error) : undefined;
  const view = preview.data?.view;
  const errors = refused?.refused.view.errors ?? invalid?.errors;
  const hint = useMemo(() => jsonHint(watched.value ?? ""), [watched.value]);

  if (send.data) {
    return (
      <Result
        a={send.data}
        onAnother={() => {
          send.reset();
          form.reset({ ...form.getValues(), value: "" });
        }}
      />
    );
  }
  const parts = view?.partitions ?? 0;
  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={form.handleSubmit((v) => send.mutate(v))}
      data-testid="produce-form"
      noValidate
    >
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-2">
          <Label htmlFor="pp">Partition</Label>
          <Controller
            control={form.control}
            name="partition"
            render={({ field }) => (
              <Select value={field.value} onValueChange={field.onChange}>
                <SelectTrigger id="pp" className="w-full font-mono" data-testid="partition" aria-invalid={!!errors?.partition}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={AUTO}>auto (the partitioner picks)</SelectItem>
                  {Array.from({ length: parts }, (_, p) => (
                    <SelectItem key={p} value={String(p)} className="font-mono">
                      {p}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          />
          {errors?.partition ? <FieldError>{errors.partition}</FieldError> : <p className="text-[12.5px] text-muted-foreground">Auto, or 0 to {Math.max(0, parts - 1)}. Auto sends a key where Kafka's default partitioner does.</p>}
        </div>
        <div className="flex min-w-0 flex-col gap-2">
          <Label htmlFor="pk">
            Key <span className="font-normal text-muted-foreground">{view?.compacts ? "required on this topic" : "optional on this topic"}</span>
          </Label>
          <Input id="pk" className="font-mono" autoComplete="off" spellCheck={false} aria-invalid={!!errors?.key} {...form.register("key")} data-testid="key" />
          <FieldError>{errors?.key}</FieldError>
        </div>
      </div>
      <div className="flex flex-col gap-2">
        <Label htmlFor="pv">Value</Label>
        <Textarea id="pv" rows={3} spellCheck={false} className="max-h-48 min-h-20 font-mono text-[13px]" {...form.register("value")} data-testid="value" placeholder='{"order_id": "ord-1"}' />
        <p className={hint && !hint.ok ? "text-[12.5px] text-warn" : "text-[12.5px] text-muted-foreground"} data-testid="json-hint">
          {hint ? hint.text : "Text or JSON. Leave it empty for a record with no value (on a compacted topic, a deletion)."}
        </p>
      </div>
      <fieldset className="flex flex-col gap-2">
        <legend className="mb-2 text-sm font-medium">
          Headers <span className="font-normal text-muted-foreground">{headers.fields.length > 0 ? `up to ${MOST_HEADERS}` : "none"}</span>
        </legend>
        {headers.fields.map((h, i) => (
          <div key={h.id} className="grid grid-cols-[1fr_1fr_auto] gap-2" data-testid="header-row">
            <Input className="font-mono" placeholder="name" aria-label={`Header name ${i + 1}`} autoComplete="off" {...form.register(`headers.${i}.name`)} />
            <Input className="font-mono" placeholder="value" aria-label={`Header value ${i + 1}`} autoComplete="off" {...form.register(`headers.${i}.value`)} />
            <Button type="button" variant="ghost" size="icon" aria-label={`Remove header ${i + 1}`} onClick={() => headers.remove(i)}>
              <XIcon />
            </Button>
          </div>
        ))}
        <div>
          <Button type="button" variant="outline" size="sm" disabled={headers.fields.length >= MOST_HEADERS} onClick={() => headers.append({ name: "", value: "" })} data-testid="add-header">
            <PlusIcon /> Add header
          </Button>
        </div>
      </fieldset>
      <div className="flex flex-col gap-2">
        <span className="text-sm font-medium" id="acks-l">
          acks
        </span>
        <Controller
          control={form.control}
          name="acks"
          render={({ field }) => (
            <ToggleGroup type="single" variant="outline" aria-labelledby="acks-l" value={field.value} onValueChange={(v) => v && field.onChange(v)} className="self-start" data-testid="acks">
              <ToggleGroupItem value="-1" className="px-3">
                all (-1)
              </ToggleGroupItem>
              <ToggleGroupItem value="1" className="px-3">
                1
              </ToggleGroupItem>
              <ToggleGroupItem value="0" className="px-3">
                0
              </ToggleGroupItem>
            </ToggleGroup>
          )}
        />
        <FieldError>{errors?.acks}</FieldError>
      </div>
      {preview.data && (
        <Teaching>
          <TeachLines t={preview.data.acks_teach} />
          <p className="text-[12.5px] leading-[19px] text-muted-foreground">
            <Spans spans={preview.data.key_help} />
          </p>
        </Teaching>
      )}
      {refused && (
        <div className="flex flex-col gap-3" data-testid="produce-refused">
          <p className="text-[13.5px] text-foreground">{refused.sentence}</p>
          <RefusalCallout r={refused.refused.refusal} />
          {refused.refused.record_errors.map((e) => (
            <p key={e} className="text-[12.5px] text-muted-foreground">
              The broker said of the record: {e}
            </p>
          ))}
          <CallBlock title="The answer" lines={refused.refused.answer} testId="the-answer" />
        </div>
      )}
      {send.error && !refused && !invalid && <FieldError>{send.error.message}</FieldError>}
      {!refused && view && <CallBlock lines={view.call} />}
      <StickyFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button type="submit" disabled={send.isPending} data-testid="send">
          <SendIcon /> {refused ? "Send again" : "Send"}
        </Button>
      </StickyFooter>
    </form>
  );
}

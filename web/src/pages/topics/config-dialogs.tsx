// The Config tab's dialogs (boards config-edit, -edit-list, -edit-lag, -refused, -refused-value, -saved, -reset): the edit
// dialog in its three shapes with Check (`validate_only`) and Save, what the broker answered in the same dialog (saved,
// would take it, refused in Kafka's words with what would pass), and Reset.
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CheckIcon } from "lucide-react";
import { useState } from "react";
import { Controller, useForm, useWatch } from "react-hook-form";
import type { ConfigEditAnswer } from "@/api/types/ConfigEditAnswer";
import type { ConfigEditForm } from "@/api/types/ConfigEditForm";
import type { ConfigOp } from "@/api/types/ConfigOp";
import type { ConfigRefusedAnswer } from "@/api/types/ConfigRefusedAnswer";
import type { ConfigResetDone } from "@/api/types/ConfigResetDone";
import type { ConfigResetForm } from "@/api/types/ConfigResetForm";
import type { ConfigSavedAnswer } from "@/api/types/ConfigSavedAnswer";
import type { ConfigValue } from "@/api/types/ConfigValue";
import { afterWrite, detailOf, post, queryOf, topicPath, useConfigEditForm, useConfigResetForm, useDebounced } from "@/api/topic-writes";
import { Spans } from "@/components/app/spans";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import { DoneFooter } from "@/components/app/write-done";
import { CallBlock, Done, FieldError, RefusalCallout, SpanLines, Teaching, StickyFooter } from "./writes/parts";

const MIN_LAG = "min.compaction.lag.ms";
const MAX_LAG = "max.compaction.lag.ms";
const SOURCE: Record<string, string> = { dynamic_topic: "Dynamic topic", dynamic_broker: "Dynamic broker", dynamic_default: "Dynamic broker, all", static_flag: "Static flag", default: "Default" };

type Values = { op: ConfigOp; value: string; other: string };

function Valued({ v }: { v: ConfigValue | null }) {
  if (!v) return <span className="text-muted-foreground">not set</span>;
  return (
    <>
      <code className="font-mono break-all">{v.value}</code> <span className="text-[12px] text-muted-foreground">{typeof v.source === "string" ? SOURCE[v.source] : "Other"}</span>
    </>
  );
}

function Saved({ a }: { a: ConfigSavedAnswer }) {
  return (
    <div className="flex flex-col gap-4" data-testid="config-saved">
      <Done>
        <b className="font-semibold text-bright">Saved.</b> IncrementalAlterConfigs answered <code className="font-mono">0</code> for <code className="font-mono">{a.saved.head.name}</code>. <Spans spans={a.sentence} />
      </Done>
      <div className="overflow-hidden rounded-xl border">
        <div className="grid grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,1fr)] gap-3 border-b bg-well px-4 py-2 text-[11.5px] font-medium tracking-[0.05em] text-muted-foreground uppercase">
          <span>Config</span>
          <span>Before</span>
          <span>Now</span>
        </div>
        {a.saved.changes.map((c) => (
          <div key={c.name} className="grid grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,1fr)] gap-3 border-b px-4 py-2.5 text-[13px] last:border-b-0" data-testid="saved-change">
            <code className="font-mono break-all text-bright">{c.name}</code>
            <span>
              <Valued v={c.before} />
            </span>
            <span data-testid="saved-now">
              <Valued v={c.now} />
            </span>
          </div>
        ))}
      </div>
      {a.effect && (
        <div className="rounded-xl border p-4 text-[13px] leading-[20px]">
          <h3 className="font-semibold text-bright">What it does to this log</h3>
          <p className="mt-1">{a.effect}</p>
          <p className="mt-1 text-muted-foreground">
            A change does not rewrite anything. {a.reader} ({a.when})
          </p>
        </div>
      )}
      <StickyFooter>
        <DialogClose asChild>
          <Button data-testid="saved-done">Done</Button>
        </DialogClose>
      </StickyFooter>
    </div>
  );
}

function RefusedState({ r, onAgain }: { r: ConfigRefusedAnswer; onAgain: (pairs: [string, string][]) => void }) {
  const f = r.refused;
  return (
    <div className="flex flex-col gap-4" data-testid="config-refused">
      <p className="text-[13.5px] text-foreground">{r.sentence}</p>
      <RefusalCallout r={f.refusal} />
      <CallBlock title="What you sent" lines={f.request} testId="config-sent" note={f.kind === "lags" ? "Both keys were in one request, so both were refused." : undefined} />
      {(f.pass.length > 0 || f.kind === "lags") && (
        <section className="rounded-xl border p-4 text-[13px]" data-testid="would-pass">
          <h3 className="font-semibold text-bright">What would pass</h3>
          {f.kind === "lags" && <p className="mt-1 text-muted-foreground">min is how long a record waits at least, max how long at most: min cannot be above max.{f.pass.length > 0 ? " Kafka took these when asked with validate_only." : ""}</p>}
          {f.pass.map((p, i) => (
            <pre key={i} className="mt-2 overflow-x-auto rounded-lg bg-well px-3 py-2 font-mono text-[12.5px] whitespace-pre">
              {p.lines.join("\n")}
              {p.result ? `\n  -> ${p.result}` : ""}
            </pre>
          ))}
        </section>
      )}
      {f.others.length > 0 && (
        <section className="rounded-xl border p-4 text-[13px]">
          <h3 className="font-semibold text-bright">The same words for other ranges</h3>
          <p className="mt-1 text-muted-foreground">Kafka names the config, the value, and the rule it broke. These are its answers to those values now.</p>
          <dl className="mt-2 grid grid-cols-[minmax(0,1fr)_minmax(0,2fr)] gap-x-4 gap-y-1.5 font-mono text-[12px]">
            {f.others.map(([send, said]) => (
              <div key={send} className="contents">
                <dt className="break-all">{send}</dt>
                <dd className="break-words text-muted-foreground">{said}</dd>
              </div>
            ))}
          </dl>
        </section>
      )}
      <StickyFooter>
        <DialogClose asChild>
          <Button variant="outline">Close</Button>
        </DialogClose>
        <Button onClick={() => onAgain(r.again)} data-testid="edit-again">
          Edit again
        </Button>
      </StickyFooter>
    </div>
  );
}

const OPS: { op: ConfigOp; name: string; sub: string }[] = [
  { op: "append", name: "Append", sub: "add to the list" },
  { op: "subtract", name: "Subtract", sub: "take out of it" },
  { op: "set", name: "Set", sub: "replace it all" },
];

/** The dialog's first answer: the fields start at what the server says the dialog holds (the value now; a list starts
 * empty, appending). */
function EditBody({ topic, k, open }: { topic: string; k: string; open: boolean }) {
  const first = useConfigEditForm(topic, queryOf({ key: k }), open);
  if (first.error && !first.data) return <FieldError>{first.error.message}</FieldError>;
  if (!first.data) return <Skeleton className="h-48 w-full" />;
  return <EditForm topic={topic} k={k} open={open} initial={first.data} />;
}

function EditForm({ topic, k, open, initial }: { topic: string; k: string; open: boolean; initial: ConfigEditForm }) {
  const qc = useQueryClient();
  const form = useForm<Values>({ defaultValues: { op: initial.view.input.op, value: initial.view.input.value, other: initial.view.input.other } });
  const w = useWatch({ control: form.control }) as Values;
  const q = useDebounced(queryOf({ key: k, op: w.op, value: w.value, other: w.other }), 200);
  const preview = useConfigEditForm(topic, q, open);
  const send = useMutation({
    mutationFn: (x: Values & { check: boolean }) => post<ConfigEditAnswer>(topicPath(topic, "/config/edit"), { key: k, op: x.op, value: x.value, other: x.other, ...(x.check ? { check: "1" } : {}) }),
    onSuccess: (a) => {
      if (a.kind === "saved") void afterWrite(qc);
    },
  });
  const refused = send.error ? detailOf<ConfigRefusedAnswer>(send.error) : undefined;
  const invalid = send.error && !refused?.refused ? detailOf<ConfigEditForm>(send.error) : undefined;
  const checked = send.data?.kind === "checked" ? send.data.form : undefined;
  const shown = checked ?? invalid ?? preview.data ?? initial;
  if (send.data?.kind === "saved") return <Saved a={send.data.answer} />;
  if (refused?.refused) {
    return (
      <RefusedState
        r={refused}
        onAgain={(pairs) => {
          const next = { ...form.getValues() };
          for (const [name, value] of pairs) next[name as "value" | "other"] = value;
          form.reset(next);
          send.reset();
        }}
      />
    );
  }
  const v = shown.view;
  const error = invalid?.view.error ?? send.error?.message;
  const field = (name: "value" | "other", id: string, label: React.ReactNode, unit: string | null) => (
    <div className="flex flex-col gap-2">
      <Label htmlFor={id}>{label}</Label>
      <div className="relative">
        <Input id={id} className="h-10 pr-40 font-mono" autoComplete="off" spellCheck={false} aria-invalid={!!error} {...form.register(name, { onChange: () => send.reset() })} data-testid={`config-${name}`} />
        <span className="pointer-events-none absolute inset-y-0 right-3 flex max-w-36 items-center truncate font-mono text-[12px] text-muted-foreground">{unit ?? ""}</span>
      </div>
    </div>
  );
  return (
    <form className="flex flex-col gap-5" noValidate onSubmit={form.handleSubmit((x) => send.mutate({ ...x, check: false }))} data-testid="config-edit-form">
      {v.form === "list" && (
        <div className="flex flex-col gap-2">
          <span className="text-sm font-medium" id="op-l">
            It is a list. What to do with it?
          </span>
          <Controller
            control={form.control}
            name="op"
            render={({ field: f }) => (
              <RadioGroup
                aria-labelledby="op-l"
                value={f.value}
                onValueChange={(x) => {
                  f.onChange(x);
                  send.reset();
                }}
                className="grid gap-2 sm:grid-cols-3"
                data-testid="config-op"
              >
                {OPS.map((o) => (
                  <Label key={o.op} htmlFor={`op-${o.op}`} className={cn("flex cursor-pointer items-start gap-3 rounded-lg border px-3 py-2.5 font-normal hover:bg-well/60", f.value === o.op && "border-primary bg-primary/5")}>
                    <RadioGroupItem id={`op-${o.op}`} value={o.op} className="mt-0.5" />
                    <span>
                      <span className="block text-[13.5px] font-medium text-bright">{o.name}</span>
                      <span className="block text-[12px] text-muted-foreground">{o.sub}</span>
                    </span>
                  </Label>
                ))}
              </RadioGroup>
            )}
          />
          {w.op !== "set" && (
            <p className="flex flex-wrap items-center gap-1.5 text-[12.5px] text-muted-foreground">
              Now:
              {shown.have.map((e) => (
                <code key={e} className="rounded-md bg-well px-1.5 py-0.5 font-mono text-foreground">
                  {e}
                </code>
              ))}
              <span>{w.op === "append" ? "+" : "−"}</span>
            </p>
          )}
        </div>
      )}
      {v.form === "lags" ? (
        <div className="grid gap-4 sm:grid-cols-2">
          {field("value", "cv", <code className="font-mono">{MIN_LAG}</code>, shown.unit)}
          {field("other", "co", <code className="font-mono">{v.other?.name ?? MAX_LAG}</code>, shown.other_unit)}
        </div>
      ) : (
        field("value", "cv", v.form === "single" ? <>New value for <code className="font-mono">{topic}</code></> : shown.label, shown.unit)
      )}
      <FieldError>{error}</FieldError>
      <Teaching defaultOpen label="What Kafka says about it">
        <SpanLines
          lines={[
            ["What it is", v.teach.what],
            ["Now", v.teach.now],
            ["Allowed", v.teach.allowed],
            ["Takes effect", v.teach.takes_effect],
            ["Here", v.teach.here],
          ]}
        />
      </Teaching>
      {checked?.view.checked && (
        <div role="status" className="flex gap-3 rounded-xl border border-good/30 bg-good/5 p-4 text-[13px]" data-testid="config-checked">
          <CheckIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-good" />
          <div className="min-w-0">
            <b className="font-semibold text-bright">Kafka would take this.</b> The same call with validate_only answered 0, and nothing was changed.
            <dl className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1">
              {checked.view.checked.after.map(([key, now, after]) => (
                <div key={key} className="contents">
                  <dt className="font-mono text-muted-foreground">{key}</dt>
                  <dd className="font-mono break-all">
                    {now} <span className="font-sans text-muted-foreground">to</span> {after}
                  </dd>
                </div>
              ))}
            </dl>
          </div>
        </div>
      )}
      <CallBlock lines={v.call} />
      <StickyFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button type="button" variant="outline" disabled={send.isPending} onClick={form.handleSubmit((x) => send.mutate({ ...x, check: true }))} data-testid="config-check">
          Check
        </Button>
        <Button type="submit" disabled={send.isPending} data-testid="config-save">
          Save
        </Button>
      </StickyFooter>
    </form>
  );
}

function ResetBody({ topic, k, open, onDone }: { topic: string; k: string; open: boolean; onDone: () => void }) {
  const qc = useQueryClient();
  const preview = useConfigResetForm(topic, queryOf({ key: k }), open);
  const send = useMutation({
    mutationFn: () => post<ConfigResetDone>(topicPath(topic, "/config/reset"), { key: k }),
    onSuccess: () => void afterWrite(qc),
  });
  const failed = detailOf<ConfigResetForm>(send.error);
  const shown = failed ?? preview.data;
  if (send.isSuccess) {
    return (
      <div className="flex flex-col gap-4">
        <Done testId="config-reset-done">
          <b className="font-semibold text-bright">Reset {k}.</b> The value set on {topic} is removed, so the topic follows the broker again.
        </Done>
        <DoneFooter onDone={onDone} />
      </div>
    );
  }
  if (!shown) return <Skeleton className="h-40 w-full" />;
  const v = shown.view;
  return (
    <div className="flex flex-col gap-4" data-testid="config-reset-form">
      <p className="text-[14px]">
        The value set on <code className="font-mono">{topic}</code> is removed, so the topic follows the broker again.
      </p>
      <dl className="grid grid-cols-[5rem_minmax(0,1fr)] gap-x-4 gap-y-2 text-[13.5px]">
        <dt className="text-muted-foreground">Now</dt>
        <dd data-testid="reset-now">
          <code className="font-mono break-all">{v.row.value}</code> {shown.now_gloss && <span className="text-muted-foreground">{shown.now_gloss}</span>} <span className="text-[12px] text-muted-foreground">· {typeof v.row.source === "string" ? SOURCE[v.row.source] : "Other"}</span>
        </dd>
        <dt className="text-muted-foreground">After</dt>
        <dd data-testid="reset-after">
          {v.after ? (
            <>
              <code className="font-mono break-all">{v.after.value}</code> {shown.after_gloss && <span className="text-muted-foreground">{shown.after_gloss}</span>} <span className="text-[12px] text-muted-foreground">· {typeof v.after.source === "string" ? SOURCE[v.after.source] : "Other"}</span>
            </>
          ) : (
            <span className="text-muted-foreground">none: the broker lists no value for it</span>
          )}
        </dd>
      </dl>
      {shown.takes_effect && (
        <p className="border-l-2 pl-3 text-[12.5px] text-muted-foreground">
          <b className="font-semibold text-foreground">Takes effect</b> {shown.takes_effect}
        </p>
      )}
      {v.refusal && <RefusalCallout r={v.refusal} />}
      {send.error && !failed && <FieldError>{send.error.message}</FieldError>}
      <CallBlock lines={v.call} />
      <StickyFooter>
        <DialogClose asChild>
          <Button variant="outline">Cancel</Button>
        </DialogClose>
        <Button variant="destructive" disabled={send.isPending || !v.row.source || v.row.source !== "dynamic_topic"} onClick={() => send.mutate()} data-testid="reset-submit">
          Reset to default
        </Button>
      </StickyFooter>
    </div>
  );
}

export default function ConfigDialogs({ topic, edit, reset, onClose }: { topic: string; edit?: string; reset?: string; onClose: () => void }) {
  // The dialog on screen, kept while it animates out; a new opening gets a fresh body.
  const [shown, setShown] = useState<{ kind: "edit" | "reset"; key: string; n: number } | null>(null);
  const want = edit ? { kind: "edit" as const, key: edit } : reset ? { kind: "reset" as const, key: reset } : null;
  const [was, setWas] = useState<string | null>(null);
  const id = want ? `${want.kind}:${want.key}` : null;
  if (id !== was) {
    setWas(id);
    if (want) setShown({ ...want, n: (shown?.n ?? 0) + 1 });
  }
  if (!shown) return null;
  const open = !!want && want.kind === shown.kind && want.key === shown.key;
  const lags = shown.key === MIN_LAG || shown.key === MAX_LAG;
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="flex max-h-[min(92vh,60rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl" data-testid={`dialog-config-${shown.kind}`} aria-describedby="config-dialog-d">
        <DialogHeader className="border-b px-6 pt-5 pb-4">
          <DialogTitle className="text-[19px] font-semibold break-all text-bright">
            {shown.kind === "edit" ? (lags ? "Edit compaction lag" : <>Edit <code className="font-mono">{shown.key}</code></>) : <>Reset <code className="font-mono">{shown.key}</code> to default?</>}
          </DialogTitle>
          <DialogDescription id="config-dialog-d">
            {shown.kind === "edit" ? "Check asks the broker with validate_only and changes nothing; Save sends IncrementalAlterConfigs." : "IncrementalAlterConfigs DELETE of the value the topic sets."}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-6 py-5 [--dp-y:1.25rem]" data-testid="dialog-body">
          {shown.kind === "edit" ? <EditBody key={shown.n} topic={topic} k={shown.key} open={open} /> : <ResetBody key={shown.n} topic={topic} k={shown.key} open={open} onDone={onClose} />}
        </div>
      </DialogContent>
    </Dialog>
  );
}

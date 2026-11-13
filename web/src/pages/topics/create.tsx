// Create topic (boards topic-create, -config, -review, -refused, topic-created): a page with three steps (shadcn Tabs as a
// stepper, the step in the address), react-hook-form with a zod schema for what can be told at once, and, as the person
// types, the server's own checks: the form view (the name taken or colliding, the limits, the call and the terminal
// command), then the same CreateTopics with `validate_only` once nothing is wrong, whose answer is Kafka's.
import { zodResolver } from "@hookform/resolvers/zod";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, Navigate, useNavigate, useSearch } from "@tanstack/react-router";
import { CheckIcon, CircleCheckIcon, LoaderCircleIcon, SendIcon } from "lucide-react";
import { useEffect, useMemo } from "react";
import { Controller, useForm, useWatch } from "react-hook-form";
import { z } from "zod";
import { ApiFailure } from "@/api/client";
import type { CheckedAnswer } from "@/api/types/CheckedAnswer";
import type { CreatedAnswer } from "@/api/types/CreatedAnswer";
import type { CreateRefused } from "@/api/types/CreateRefused";
import type { CreateStep } from "@/api/types/CreateStep";
import type { CreateTemplate } from "@/api/types/CreateTemplate";
import type { CreateView } from "@/api/types/CreateView";
import { afterWrite, detailOf, FORM, post, queryOf, useCreateForm, useDebounced } from "@/api/topic-writes";
import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { number } from "@/lib/format";
import type { CreateSearch } from "@/lib/search";
import { cn } from "@/lib/utils";
import { CallBlock, FieldError, RefusalCallout, TeachLines, Terminal } from "./writes/parts";

z.config({ jitless: true });

// What can be said at once, before the server is asked (the server says it again, in the same words where it can).
const schema = z.object({
  name: z
    .string()
    .min(1, "A topic needs a name.")
    .max(249, "At most 249 characters.")
    .refine((n) => n !== "." && n !== "..", "'.' and '..' cannot be topic names.")
    .regex(/^[a-zA-Z0-9._-]*$/, "ASCII letters and digits, '.', '_' and '-' only."),
  partitions: z.string().regex(/^-?\d+$/, "A whole number."),
  template: z.enum(["event", "compact", "short"]),
  cleanup: z.enum(["delete", "compact", "delete,compact"]),
  retention: z.string().regex(/^-?\d+$/, "A whole number of milliseconds, or -1 for no limit."),
});
type Values = z.infer<typeof schema>;

const STEPS: { id: CreateStep; label: string }[] = [
  { id: "name", label: "Name and partitions" },
  { id: "config", label: "Configuration" },
  { id: "review", label: "Review and create" },
];
// The sentence under the title for each step (the classic page's words).
const SUBS: Record<CreateStep, string> = {
  name: "Three steps. The call on the right is the one that will be sent; it changes as you type.",
  config: "What the topic does with old records. Kafka's own sentence, the default and when it takes effect sit under each field.",
  review: "Check what will be sent, then create. Nothing exists until you do.",
};
const order = (s: CreateStep) => STEPS.findIndex((x) => x.id === s);

const TEMPLATES: { id: CreateTemplate; name: string; sub: string; cleanup: Values["cleanup"]; retention: string }[] = [
  { id: "event", name: "Event log", sub: "delete, keeps 7 d", cleanup: "delete", retention: "604800000" },
  { id: "compact", name: "Compacted state", sub: "compact: the latest value for each key", cleanup: "compact", retention: "604800000" },
  { id: "short", name: "Short lived", sub: "delete, keeps 1 h", cleanup: "delete", retention: "3600000" },
];

const POLICIES: { id: Values["cleanup"]; sub: string }[] = [
  { id: "delete", sub: "old segments go when they pass the retention time or size" },
  { id: "compact", sub: "keep the latest record for every key; a record with no key is refused" },
  { id: "delete,compact", sub: "both: compact what is kept, and drop what is too old" },
];

function fieldsOf(v: Values, step?: CreateStep): Record<string, string> {
  return { ...(step ? { step } : {}), name: v.name, partitions: v.partitions, template: v.template, "cleanup.policy": v.cleanup, "retention.ms": v.retention };
}

/** The live check: the same CreateTopics with validate_only, asked only when the form has nothing to fix. */
function useLiveCheck(v: Values, ready: boolean) {
  const q = useDebounced(queryOf(fieldsOf(v)), 400);
  return useQuery({
    queryKey: [FORM, "create-check", q],
    queryFn: () => post<CheckedAnswer>("/api/v1/create-topic", { ...Object.fromEntries(new URLSearchParams(q)), mode: "check" }),
    enabled: ready && q !== "",
    staleTime: 10_000,
    retry: false,
  });
}

function Stepper({ step, reached }: { step: CreateStep; reached: number }) {
  return (
    <TabsList className="mb-8 h-auto flex-wrap justify-start gap-1 bg-transparent p-0" aria-label="Steps">
      {STEPS.map((s, i) => {
        const done = i < order(step);
        return (
          <TabsTrigger
            key={s.id}
            value={s.id}
            disabled={i > reached}
            className="h-9 flex-none gap-2.5 rounded-full border-0 px-3 text-[14px] text-muted-foreground shadow-none data-[state=active]:bg-primary/10 data-[state=active]:text-bright data-[state=active]:shadow-none"
            data-testid={`step-${s.id}`}
          >
            <span className={cn("grid size-6 place-items-center rounded-full border text-[12px] tabular-nums", (done || s.id === step) && "border-primary bg-primary text-primary-foreground")}>{done ? <CheckIcon className="size-3.5" /> : i + 1}</span>
            {s.label}
          </TabsTrigger>
        );
      })}
    </TabsList>
  );
}

const CREATED = "doze:created:";
function keepCreated(a: CreatedAnswer) {
  try {
    sessionStorage.setItem(CREATED + a.created.name, JSON.stringify(a));
  } catch {
    // private mode: a reload shows the topic instead
  }
}
function keptCreated(name: string): CreatedAnswer | null {
  try {
    const raw = sessionStorage.getItem(CREATED + name);
    return raw ? (JSON.parse(raw) as CreatedAnswer) : null;
  } catch {
    return null;
  }
}

function Created({ a }: { a: CreatedAnswer }) {
  const c = a.created;
  return (
    <div data-testid="created">
      <PageHeader crumbs={[{ label: "Topics", to: "/topics" }]} title={c.name} mono sub={c.empty ? `Empty: offsets 0 to 0 on all ${c.partitions} partitions. Nothing has written to it yet.` : `${c.partitions} partitions.`} actions={
        <>
          <Button asChild>
            <Link to="/topics/$topic" params={{ topic: c.name }} search={{ dialog: "produce" }} data-testid="created-produce">
              <SendIcon /> Produce a record
            </Link>
          </Button>
          <Button asChild variant="outline">
            <Link to="/topics/$topic" params={{ topic: c.name }}>
              Open topic
            </Link>
          </Button>
        </>
      } />
      <div role="status" className="mb-8 flex gap-3 rounded-xl border border-good/30 bg-good/5 p-4 text-[14px]">
        <CircleCheckIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-good" />
        <p>
          <b className="font-semibold text-bright">Created {c.name}.</b> {c.partitions} partitions, cleanup.policy {c.cleanup}, {a.retention}. It is on disk now: it will be there after a power cut.
        </p>
      </div>
      <h2 className="text-[17px] font-semibold text-bright">Next steps</h2>
      <p className="mt-1 mb-4 text-[14px] text-muted-foreground">Write a first record, then read it back with a consumer in a group.</p>
      <div className="grid gap-5 lg:grid-cols-2">
        <Terminal title="1 · Produce, from a terminal" lines={a.produce} testId="kcat-produce" />
        <Terminal title="2 · Add a consumer (a group id makes it a consumer group)" lines={a.consume} testId="kcat-consume" />
      </div>
    </div>
  );
}

function Refused({ r, onName }: { r: CreateRefused; onName: () => void }) {
  const most = r.most !== null ? ` (${number(r.most)})` : "";
  const rules = [
    ["INVALID_TOPIC_EXCEPTION (17)", "the name has a character Kafka forbids, or collides: \"Topic 'orders_dlq' collides with existing topic: orders.dlq\"", "name"],
    ["INVALID_PARTITIONS (37)", `zero or fewer partitions, or more than --max-partitions${most}`, "partitions"],
    ["INVALID_CONFIG (40)", "a value Kafka would not accept, naming the first bad key", "config"],
  ];
  return (
    <div className="flex flex-col gap-5" data-testid="create-refused">
      <p className="text-[14px] text-foreground">The topic was not created.</p>
      <RefusalCallout r={r.refusal} />
      <Card>
        <CardContent className="flex flex-col gap-3">
          <h3 className="text-[15px] font-semibold text-bright">What you can do</h3>
          <div className="flex flex-wrap gap-2">
            {r.existing && (
              <Button asChild>
                <Link to="/topics/$topic" params={{ topic: r.existing }}>
                  Open the existing topic
                </Link>
              </Button>
            )}
            <Button variant="outline" onClick={onName} data-testid="another-name">
              Choose another name
            </Button>
            <Button asChild variant="outline">
              <Link to="/topics">Back to topics</Link>
            </Button>
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardContent className="flex flex-col gap-2">
          <h3 className="text-[15px] font-semibold text-bright">The other refusals of this call</h3>
          <p className="text-[13px] text-muted-foreground">Each topic in a CreateTopics is answered on its own, with Kafka's code.</p>
          <ul className="flex flex-col gap-2 text-[13px]">
            {rules.map(([code, why, tag]) => (
              <li key={code} className="grid grid-cols-[1fr_auto] gap-3 rounded-lg bg-well px-3 py-2">
                <span>
                  <code className="font-mono text-foreground">{code}</code>
                  <span className="block text-muted-foreground">{why}</span>
                </span>
                <span className="text-muted-foreground">{tag}</span>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
      <CallBlock title="The answer" lines={r.answer} testId="the-answer" note="A refused topic answers -1 partitions and -1 replication factor." />
    </div>
  );
}

function Kv({ rows }: { rows: [string, React.ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[minmax(8rem,auto)_1fr] gap-x-6 gap-y-2.5 text-[14px]" data-testid="review">
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="min-w-0 break-words">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

export function CreateTopicPage() {
  const search = useSearch({ from: "/topics/new" });
  const navigate = useNavigate({ from: "/topics/new" });
  const step: CreateStep = search.step ?? "name";
  const qc = useQueryClient();
  const form = useForm<Values>({
    resolver: zodResolver(schema),
    mode: "onChange",
    defaultValues: { name: search.name ?? "", partitions: "", template: "event", cleanup: "delete", retention: "604800000" },
  });
  const v = useWatch({ control: form.control }) as Values;
  const q = useDebounced(queryOf(fieldsOf({ ...form.getValues(), ...v }, "review")), 250);
  const preview = useCreateForm(q);
  const view: CreateView | undefined = preview.data?.view;
  // The broker's default count, the first time the form is answered.
  const brokerDefault = view?.draft.partitions;
  useEffect(() => {
    if (brokerDefault !== undefined && form.getValues("partitions") === "" && !form.getFieldState("partitions").isDirty) form.setValue("partitions", brokerDefault, { shouldValidate: true });
  }, [brokerDefault, form]);
  const local = form.formState.errors;
  // Server errors for every step (the form view was asked at step review, so every check ran); the step it lands on is the
  // first with something to fix.
  const server = view ? { name: view.errors.name ?? undefined, partitions: view.errors.partitions ?? undefined, retention: view.errors.retention ?? undefined } : {};
  const nameError = local.name?.message ?? (v.name ? server.name : undefined);
  const partitionsError = local.partitions?.message ?? server.partitions;
  const retentionError = local.retention?.message ?? server.retention;
  const blocked = !view || !!(nameError || partitionsError || retentionError) || !v.name || q !== queryOf(fieldsOf({ ...form.getValues(), ...v }, "review"));
  const check = useLiveCheck(v, !blocked && step !== "review");
  const go = (s: CreateStep) => void navigate({ search: (p: CreateSearch) => ({ ...p, step: s === "name" ? undefined : s }), resetScroll: false });
  const reachable = (s: CreateStep) => {
    const i = order(s);
    if (i >= 1 && (nameError || partitionsError || !v.name)) return false;
    if (i >= 2 && retentionError) return false;
    return true;
  };
  const reached = reachable("review") ? 2 : reachable("config") ? 1 : 0;
  const create = useMutation({
    mutationFn: (mode: "check" | "create") => post<CreatedAnswer | CheckedAnswer>("/api/v1/create-topic", { ...fieldsOf(form.getValues()), mode }),
    onSuccess: (a) => {
      if ("created" in a) {
        void afterWrite(qc);
        // The done state has its own address (`?created=<name>`), so a reload shows it and never the form again; what the
        // server answered is kept for the session to draw it.
        keepCreated(a);
        void navigate({ search: { created: a.created.name }, replace: true, resetScroll: true });
      }
    },
  });
  const refused = create.error instanceof ApiFailure && create.error.status === 422 ? detailOf<CreateRefused>(create.error) : undefined;
  const answered = create.data;
  const liveRefusal = check.error instanceof ApiFailure && check.error.status === 422 ? detailOf<CreateRefused>(check.error) : undefined;
  const templateOf = useMemo(() => Object.fromEntries(TEMPLATES.map((t) => [t.id, t])), []);

  if (search.created) {
    const done = answered && "created" in answered && answered.created.name === search.created ? answered : keptCreated(search.created);
    // Kept no more (another browser session): the topic itself is the done state.
    return done ? <Created a={done} /> : <Navigate to="/topics/$topic" params={{ topic: search.created }} replace />;
  }

  const live = (
    <p className="min-h-5 text-[12.5px]" data-testid="live-check" aria-live="polite">
      {check.isFetching ? (
        <span className="inline-flex items-center gap-1.5 text-muted-foreground">
          <LoaderCircleIcon className="size-3.5 animate-spin" /> Asking the broker…
        </span>
      ) : liveRefusal ? (
        <span className="text-destructive">
          Kafka says <code className="font-mono">{liveRefusal.refusal.name}</code>: {liveRefusal.refusal.message}
        </span>
      ) : check.data ? (
        <span className="inline-flex items-center gap-1.5 text-good">
          <CheckIcon className="size-3.5" /> Kafka would take this: CreateTopics with validate_only passed.
        </span>
      ) : null}
    </p>
  );

  return (
    <div data-testid="create-page">
      <PageHeader crumbs={[{ label: "Topics", to: "/topics" }]} title="Create topic" sub={SUBS[step]} />
      <Tabs value={step} onValueChange={(s) => go(s as CreateStep)} className="gap-0">
        <Stepper step={step} reached={reached} />
        <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(22rem,30rem)]">
          <div className="min-w-0">
            {refused ? (
              <Refused
                r={refused}
                onName={() => {
                  create.reset();
                  go("name");
                  setTimeout(() => form.setFocus("name"), 0);
                }}
              />
            ) : (
              <Card>
                <CardContent>
                  <form noValidate onSubmit={(e) => e.preventDefault()} className="flex flex-col gap-6" data-testid="create-form">
                    <TabsContent value="name" className="flex flex-col gap-6">
                      <div className="flex flex-col gap-2">
                        <Label htmlFor="cn">Name</Label>
                        <Input id="cn" className="h-10 font-mono" autoComplete="off" spellCheck={false} autoFocus aria-invalid={!!nameError} aria-describedby="cn-e" {...form.register("name")} data-testid="name" />
                        <FieldError id="cn-e">{nameError}</FieldError>
                        {live}
                        {view && <TeachLines t={view.teach_name} />}
                      </div>
                      <div className="flex flex-col gap-2">
                        <Label htmlFor="cp">Partitions</Label>
                        <div className="relative">
                          <Input id="cp" inputMode="numeric" className="h-10 pr-32 font-mono" autoComplete="off" aria-invalid={!!partitionsError} {...form.register("partitions")} data-testid="partitions" />
                          <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center font-mono text-[12px] text-muted-foreground">{view?.most ? `of 1 to ${number(view.most)}` : ""}</span>
                        </div>
                        <FieldError>{partitionsError}</FieldError>
                        {view && <TeachLines t={view.teach_partitions} />}
                      </div>
                      <div className="flex flex-col gap-2">
                        <span className="text-sm font-medium" id="tpl-l">
                          Start from
                        </span>
                        <Controller
                          control={form.control}
                          name="template"
                          render={({ field }) => (
                            <RadioGroup
                              aria-labelledby="tpl-l"
                              value={field.value}
                              onValueChange={(t) => {
                                field.onChange(t);
                                const tpl = templateOf[t]!;
                                form.setValue("cleanup", tpl.cleanup);
                                form.setValue("retention", tpl.retention, { shouldValidate: true });
                              }}
                              className="grid gap-3 md:grid-cols-3"
                              data-testid="template"
                            >
                              {TEMPLATES.map((t) => (
                                <Label key={t.id} htmlFor={`tpl-${t.id}`} className={cn("flex cursor-pointer items-start gap-3 rounded-xl border px-4 py-3.5 font-normal transition-colors hover:bg-well/60", field.value === t.id && "border-primary bg-primary/5")}>
                                  <RadioGroupItem id={`tpl-${t.id}`} value={t.id} className="mt-0.5" />
                                  {/* The Label's own leading is none: the card's lines get theirs back, so a sub that wraps reads. */}
                                  <span className="flex min-w-0 flex-col gap-1.5">
                                    <span className="text-[14px] leading-5 font-medium text-bright">{t.name}</span>
                                    <span className="text-[12.5px] leading-[18px] text-muted-foreground">{t.sub}</span>
                                  </span>
                                </Label>
                              ))}
                            </RadioGroup>
                          )}
                        />
                        <p className="text-[12.5px] text-muted-foreground">A starting point only: the next step shows every value it sets.</p>
                      </div>
                      <div className="flex justify-end gap-2 border-t pt-5">
                        <Button asChild variant="outline">
                          <Link to="/topics">Cancel</Link>
                        </Button>
                        <Button type="button" disabled={!reachable("config")} onClick={() => go("config")} data-testid="next-config">
                          Next: configuration
                        </Button>
                      </div>
                    </TabsContent>
                    <TabsContent value="config" className="flex flex-col gap-6">
                      <div className="flex flex-col gap-2">
                        <span className="font-mono text-sm font-medium" id="cl-l">
                          cleanup.policy
                        </span>
                        <Controller
                          control={form.control}
                          name="cleanup"
                          render={({ field }) => (
                            <RadioGroup aria-labelledby="cl-l" value={field.value} onValueChange={field.onChange} className="gap-2" data-testid="cleanup">
                              {POLICIES.map((p) => (
                                <Label key={p.id} htmlFor={`cl-${p.id}`} className={cn("flex cursor-pointer items-start gap-3 rounded-lg border px-4 py-3 font-normal hover:bg-well/60", field.value === p.id && "border-primary bg-primary/5")}>
                                  <RadioGroupItem id={`cl-${p.id}`} value={p.id} className="mt-0.5" />
                                  <span className="flex min-w-0 flex-col gap-1">
                                    <span className="font-mono text-[13.5px] leading-5 text-bright">{p.id}</span>
                                    <span className="text-[12.5px] leading-[18px] text-muted-foreground">{p.sub}</span>
                                  </span>
                                </Label>
                              ))}
                            </RadioGroup>
                          )}
                        />
                        {view?.teach_cleanup && <TeachLines t={view.teach_cleanup} />}
                      </div>
                      <div className="flex flex-col gap-2">
                        <Label htmlFor="cr" className="font-mono">
                          retention.ms
                        </Label>
                        <div className="relative">
                          <Input id="cr" inputMode="numeric" className="h-10 pr-32 font-mono" autoComplete="off" aria-invalid={!!retentionError} {...form.register("retention")} data-testid="retention" />
                          <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center font-mono text-[12px] text-muted-foreground">{preview.data?.retention_words ?? ""}</span>
                        </div>
                        <FieldError>{retentionError}</FieldError>
                        {live}
                        {view?.teach_retention && <TeachLines t={view.teach_retention} />}
                      </div>
                      <div className="flex justify-end gap-2 border-t pt-5">
                        <Button type="button" variant="outline" onClick={() => go("name")}>
                          Back
                        </Button>
                        <Button type="button" disabled={!reachable("review")} onClick={() => go("review")} data-testid="next-review">
                          Next: review
                        </Button>
                      </div>
                    </TabsContent>
                    <TabsContent value="review" className="flex flex-col gap-6">
                      <h2 className="text-[17px] font-semibold text-bright">What you will get</h2>
                      <Kv
                        rows={[
                          ["Name", <code className="font-mono">{v.name}</code>],
                          ["Partitions", <>{v.partitions} <span className="text-muted-foreground">directories made and flushed before the topic is announced</span></>],
                          ["Replication factor", <>1 <span className="text-muted-foreground">one broker, said once</span></>],
                          ["cleanup.policy", <code className="font-mono">{v.cleanup}</code>],
                          ["retention.ms", <><code className="font-mono">{v.retention}</code> <span className="text-muted-foreground">{preview.data?.retention_words ? `(${preview.data.retention_words.replace(/^= /, "")})` : ""}</span></>],
                          ["Everything else", "the broker's defaults, none set on the topic"],
                        ]}
                      />
                      {answered && "checked" in answered && (
                        <div role="status" className="flex gap-3 rounded-xl border border-good/30 bg-good/5 p-4 text-[13.5px]" data-testid="checked">
                          <CheckIcon aria-hidden className="mt-0.5 size-5 shrink-0 text-good" />
                          <p>
                            <b className="font-semibold text-bright">Checked.</b> Every check passed for {answered.partitions} partitions, and nothing was created.
                          </p>
                        </div>
                      )}
                      {create.error && !refused && <FieldError>{create.error.message}</FieldError>}
                      <div className="rounded-xl border bg-well/40 p-4 text-[13px] leading-[20px] text-muted-foreground">
                        <b className="font-semibold text-foreground">What can refuse this call.</b> A name that already exists, a name that collides with one by <code className="font-mono">.</code> and <code className="font-mono">_</code>, a partition count outside {view?.most ? `1 to ${number(view.most)}` : "what the broker allows"}, a config value Kafka would not accept. Check only runs every one of those checks and creates nothing. If a client makes <code className="font-mono">{v.name}</code> while you read this, the call is refused.
                      </div>
                      <div className="flex flex-wrap justify-end gap-2 border-t pt-5">
                        <Button type="button" variant="outline" onClick={() => go("config")}>
                          Back
                        </Button>
                        <Button type="button" variant="outline" disabled={create.isPending} onClick={() => create.mutate("check")} data-testid="check-only">
                          Check only
                        </Button>
                        <Button type="button" disabled={create.isPending || !reachable("review")} onClick={() => create.mutate("create")} data-testid="create-submit">
                          Create topic
                        </Button>
                      </div>
                    </TabsContent>
                  </form>
                </CardContent>
              </Card>
            )}
          </div>
          <div className="flex min-w-0 flex-col gap-5 xl:sticky xl:top-20">
            {view && <CallBlock lines={view.call} note={step === "name" ? "Replication factor is always 1: this broker is the only broker." : undefined} />}
            {view && <Terminal title="Or from a terminal" lines={view.terminal} testId="terminal" />}
          </div>
        </div>
      </Tabs>
    </div>
  );
}


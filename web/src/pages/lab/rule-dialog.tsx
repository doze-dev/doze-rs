// The New rehearsal dialog (`/lab?new=<tab>`): Tabs for the six things the Lab can do. Slow and Fail make a rule (APIs, topics,
// partitions, client, group, share, lifetime; the server checks the fields and says what the rule will do before it is made);
// Drop, Expire and Rebalance happen once; Compact runs the cleaner now.
import { useQueryClient } from "@tanstack/react-query";
import { ActivityIcon, BookMarkedIcon, FlaskConicalIcon, PlayIcon, ScrollTextIcon } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { useForm, useWatch } from "react-hook-form";
import { Done, DoneFooter } from "@/components/app/write-done";
import { ApiFailure } from "@/api/client";
import { post, qs, useLabForm, useLabOnce } from "@/api/r4";
import type { LabFields } from "@/api/types/LabFields";
import type { LabGuide } from "@/api/types/LabGuide";
import type { LabRecipe } from "@/api/types/LabRecipe";
import type { LabTeach } from "@/api/types/LabTeach";
import type { LabForm } from "@/api/types/LabForm";
import type { LabMadeOutcome } from "@/api/types/LabMadeOutcome";
import type { LabOnceOutcome } from "@/api/types/LabOnceOutcome";
import { Combobox, type Option } from "@/components/app/combobox";
import { MultiSelect } from "@/components/app/multi-select";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { LabTab } from "@/lib/search";
import { CompactNowBody } from "./compact-now";
import { Lesson, TAB_LOOK, teachOf } from "./guide";

/** The fields of a recipe as the form holds them. */
function recipeFields(r: LabRecipe | undefined): LabFields | undefined {
  return r?.fields ?? undefined;
}

const list = (s: string) =>
  s
    .split(",")
    .map((x) => x.trim())
    .filter(Boolean);

/** The sentence of the server for a field, if it has one. */
function errorOf(form: LabForm | undefined, field: string): string | undefined {
  return form?.errors.find(([f]) => f === field)?.[1];
}

function FieldError({ text, id }: { text?: string; id: string }) {
  return text ? (
    <p role="alert" className="text-[12.5px] text-bad" data-testid={`err-${id}`}>
      {text}
    </p>
  ) : null;
}

/** The value after a pause in typing. */
function useSettled<T>(value: T, ms = 300): T {
  const [v, setV] = useState(value);
  const key = JSON.stringify(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, ms]);
  return v;
}

function RuleForm({ kind, start, recipe, teach, onMade }: { kind: "slow" | "fail"; start: LabForm; recipe?: LabRecipe; teach?: LabTeach; onMade: (id: number) => void }) {
  // The form's API words (`produce`) are the options' values (`Produce`), so the MultiSelect shows them chosen. A recipe's
  // fields replace the defaults whole.
  const apiValue = (w: string) => start.apis.find((a) => a.value.toLowerCase() === w.toLowerCase())?.value ?? w;
  const from = recipeFields(recipe) ?? start.fields;
  const form = useForm<LabFields>({ defaultValues: { ...from, api: list(from.api).map(apiValue).join(",") } });
  const values = useWatch({ control: form.control }) as LabFields;
  const settled = useSettled(values);
  // With a recipe the defaults' preview would say another rule: none until the server has read the recipe's fields.
  const [answer, setAnswer] = useState<LabForm>(() => (recipe ? { ...start, preview: null } : start));
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  // The server checks the fields and works out what the rule would do, after each pause in typing.
  useEffect(() => {
    let live = true;
    post<LabMadeOutcome>("/api/v1/lab/rules", { kind, fields: settled, check: true })
      .then((out) => {
        if (live && out.outcome !== "made") setAnswer(out.form);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [kind, settled]);

  const submit = async (v: LabFields) => {
    setBusy(true);
    setFailure(null);
    setChecked(true);
    try {
      const out = await post<LabMadeOutcome>("/api/v1/lab/rules", { kind, fields: v, check: false });
      if (out.outcome === "made") onMade(out.id);
      else setAnswer(out.form);
    } catch (e) {
      setFailure(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  // Field sentences show once the person tried to make the rule (or the preview cannot be made and a field has text).
  const shown = checked ? answer : undefined;
  const apis: Option[] = answer.apis.map((a) => ({ value: a.value, label: a.label }));
  const topics: Option[] = answer.topics.map((t) => ({ value: t.value, label: t.label, hint: t.desc?.replace(/\.$/, "") ?? undefined }));
  const chosenTopics = list(values.topic);
  const most = Math.max(0, ...answer.topics.filter((t) => chosenTopics.includes(t.value)).map((t) => Number(/^(\d+) partition/.exec(t.desc ?? "")?.[1] ?? 0)));
  const partitions: Option[] = Array.from({ length: Math.min(most, 64) }, (_, i) => ({ value: String(i), label: `Partition ${i}` }));
  const clients: Option[] = [...new Set([...answer.clients, ...(values.client ? [values.client] : [])])].map((c) => ({ value: c, label: c }));
  const groups: Option[] = [...new Set([...answer.groups, ...(values.group ? [values.group] : [])])].map((g) => ({ value: g, label: g }));
  const shareGroups = [
    { label: "All of them", items: answer.shares.filter((s) => s.value === "every") },
    { label: "Every Nth", items: answer.shares.filter((s) => /^\d+$/.test(s.value)) },
    { label: "A random share", items: answer.shares.filter((s) => s.value.startsWith("p")) },
  ];
  const preview = answer.preview;

  return (
    <Form {...form}>
      <form noValidate onSubmit={form.handleSubmit((v) => void submit(v))} className="grid min-w-0 gap-6 lg:grid-cols-[minmax(0,1fr)_18rem]" data-testid={`lab-form-${kind}`}>
        <div className="flex min-w-0 flex-col gap-5">
          <FormField
            control={form.control}
            name="api"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Which requests</FormLabel>
                <FormControl>
                  <MultiSelect options={apis} value={list(field.value)} onChange={(v) => field.onChange(v.join(","))} noun="APIs" aria-label="APIs" triggerClassName="w-full" className="min-w-0" />
                </FormControl>
                <FormDescription>{kind === "fail" ? "The APIs whose answer the Lab can make up: one error code in the shape of the API." : "Produce delays the ack a producer waits for; Fetch delays what a consumer reads."}</FormDescription>
                <FieldError id="api" text={errorOf(shown, "api")} />
              </FormItem>
            )}
          />
          <div className="grid items-start gap-5 sm:grid-cols-2">
            <FormField
              control={form.control}
              name="topic"
              render={({ field }) => (
                <FormItem className="min-w-0">
                  <FormLabel>Topics</FormLabel>
                  <FormControl>
                    <MultiSelect options={topics} value={list(field.value)} onChange={(v) => field.onChange(v.join(","))} noun="topics" aria-label="Topics" triggerClassName="w-full" className="min-w-0" chips={false} pattern />
                  </FormControl>
                  <FormDescription>{answer.topic_note ?? "None chosen: every topic. Type a pattern to match several: * is any run of characters, ? one (orders.*)."}</FormDescription>
                  <FieldError id="topic" text={errorOf(shown, "topic")} />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="partitions"
              render={({ field }) => (
                <FormItem className="min-w-0">
                  <FormLabel>Partitions</FormLabel>
                  <FormControl>
                    <MultiSelect options={partitions} value={list(field.value)} onChange={(v) => field.onChange(v.sort((a, b) => Number(a) - Number(b)).join(","))} noun="partitions" aria-label="Partitions" triggerClassName="w-full" className="min-w-0" chips={false} />
                  </FormControl>
                  <FormDescription>{chosenTopics.length ? "None chosen: every partition of the topics." : "Choose a topic first."}</FormDescription>
                  <FieldError id="partitions" text={errorOf(shown, "partitions")} />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="client"
              render={({ field }) => (
                <FormItem className="min-w-0">
                  <FormLabel>Client</FormLabel>
                  <FormControl>
                    <Combobox options={clients} value={field.value || null} onChange={(v) => field.onChange(v ?? "")} placeholder="Every client" search="Search apps…" empty="No app has a connection open." aria-label="Client" className="w-full" pattern />
                  </FormControl>
                  <FormDescription>An app connected now, by client.id, or a pattern such as payments-* for every instance. None: every app.</FormDescription>
                  <FieldError id="client" text={errorOf(shown, "client")} />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="group"
              render={({ field }) => (
                <FormItem className="min-w-0">
                  <FormLabel>Group</FormLabel>
                  <FormControl>
                    <Combobox options={groups} value={field.value || null} onChange={(v) => field.onChange(v ?? "")} placeholder="Any group" search="Search groups…" empty="No group yet." aria-label="Group" className="w-full" pattern />
                  </FormControl>
                  <FormDescription>Only group requests (JoinGroup, Heartbeat, OffsetCommit…) that name it; a pattern works too.</FormDescription>
                  <FieldError id="group" text={errorOf(shown, "group")} />
                </FormItem>
              )}
            />
          </div>
          {kind === "slow" ? (
            <div className="grid items-start gap-5 sm:grid-cols-2">
              <FormField
                control={form.control}
                name="delay"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Delay</FormLabel>
                    <div className="relative">
                      <FormControl>
                        <Input {...field} inputMode="numeric" autoComplete="off" className="bg-card pr-10 font-mono" data-testid="lab-delay" />
                      </FormControl>
                      <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center font-mono text-[12.5px] text-muted-foreground">ms</span>
                    </div>
                    <FormDescription>Added to the normal time; the wire shows the total.</FormDescription>
                    <FieldError id="delay" text={errorOf(shown, "delay")} />
                  </FormItem>
                )}
              />
              <FormField
                control={form.control}
                name="jitter"
                render={({ field }) => (
                  <FormItem>
                    <FormLabel>Jitter</FormLabel>
                    <div className="relative">
                      <FormControl>
                        <Input {...field} inputMode="numeric" autoComplete="off" placeholder="0" className="bg-card pr-10 font-mono" data-testid="lab-jitter" />
                      </FormControl>
                      <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center font-mono text-[12.5px] text-muted-foreground">ms</span>
                    </div>
                    <FormDescription>Up to this much more, at random.</FormDescription>
                    <FieldError id="jitter" text={errorOf(shown, "jitter")} />
                  </FormItem>
                )}
              />
            </div>
          ) : (
            <FormField
              control={form.control}
              name="code"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Answer with</FormLabel>
                  <Select value={field.value} onValueChange={field.onChange}>
                    <FormControl>
                      <SelectTrigger className="w-full bg-card font-mono" data-testid="lab-code">
                        <SelectValue />
                      </SelectTrigger>
                    </FormControl>
                    <SelectContent position="popper" className="max-h-80">
                      {answer.codes.map((c) => (
                        <SelectItem key={c.value} value={c.value} className="font-mono">
                          {c.label} ({c.value})
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <FormDescription>{answer.codes.find((c) => c.value === field.value)?.desc}</FormDescription>
                  <FieldError id="code" text={errorOf(shown, "code")} />
                </FormItem>
              )}
            />
          )}
          <div className="grid items-start gap-5 sm:grid-cols-2">
            <FormField
              control={form.control}
              name="share"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Share of requests</FormLabel>
                  <Select value={field.value} onValueChange={field.onChange}>
                    <FormControl>
                      <SelectTrigger className="w-full bg-card" data-testid="lab-share">
                        <SelectValue />
                      </SelectTrigger>
                    </FormControl>
                    <SelectContent position="popper" className="max-h-80">
                      {shareGroups.map((g) => (
                        <SelectGroup key={g.label}>
                          <SelectLabel>{g.label}</SelectLabel>
                          {g.items.map((s) => (
                            <SelectItem key={s.value} value={s.value}>
                              {s.label}
                            </SelectItem>
                          ))}
                        </SelectGroup>
                      ))}
                    </SelectContent>
                  </Select>
                  <FormDescription>1 in N: every Nth request the rule matches. A random share: each request by chance.</FormDescription>
                  <FieldError id="share" text={errorOf(shown, "share")} />
                </FormItem>
              )}
            />
            <FormField
              control={form.control}
              name="hits"
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Stop after</FormLabel>
                  <div className="relative">
                    <FormControl>
                      <Input {...field} inputMode="numeric" autoComplete="off" placeholder="no limit" className="bg-card pr-12 font-mono" data-testid="lab-hits" />
                    </FormControl>
                    <span className="pointer-events-none absolute inset-y-0 right-3 flex items-center text-[12.5px] text-muted-foreground">hits</span>
                  </div>
                  <FormDescription>It ends after this many hits, even with time left. Empty: no limit.</FormDescription>
                  <FieldError id="hits" text={errorOf(shown, "hits")} />
                </FormItem>
              )}
            />
          </div>
          <FormField
            control={form.control}
            name="ends"
            render={({ field }) => (
              <FormItem>
                <FormLabel>Ends after</FormLabel>
                <FormControl>
                  <ToggleGroup type="single" variant="outline" value={field.value} onValueChange={(v) => v && field.onChange(v)} className="self-start bg-card" data-testid="lab-ends">
                    {answer.ends.map((e) => (
                      <ToggleGroupItem key={e.value} value={e.value} className="px-3">
                        {e.label}
                      </ToggleGroupItem>
                    ))}
                  </ToggleGroup>
                </FormControl>
                <FormDescription>Every rule ends by itself. You can remove it sooner.</FormDescription>
                <FieldError id="ends" text={errorOf(shown, "ends")} />
              </FormItem>
            )}
          />
        </div>
        <aside className="flex min-w-0 flex-col gap-4" aria-live="polite" data-testid="lab-preview">
          <div className="rounded-xl border bg-card p-4">
            <h3 className="mb-2 text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">What this will do</h3>
            {preview ? (
              <>
                <p className="text-[13.5px] leading-5 text-foreground" data-testid="lab-preview-sentence">
                  {preview.plain}
                </p>
                <pre className="mt-3 overflow-x-auto rounded-lg bg-well p-3 font-mono text-[12px] leading-5">
                  {preview.rule.map(([k, v], i) => (
                    <div key={i}>{i === 0 ? `${k}:${v}` : `  ${`${k}:`.padEnd(7)} ${v}`}</div>
                  ))}
                </pre>
                <p className="mt-3 text-[12.5px] text-muted-foreground">
                  <b className="font-semibold text-foreground">The wire will show: </b>
                  {preview.wire}.
                </p>
              </>
            ) : (
              <p className="text-[13px] text-muted-foreground" data-testid="lab-preview-none">
                {answer.errors.length ? answer.errors.map(([, w]) => w).join(" ") : recipe ? "Reading the recipe…" : "Fill in the rule to see what it does."}
              </p>
            )}
          </div>
          {(answer.refused || failure) && (
            <p role="alert" className="rounded-lg border border-bad/30 bg-bad/5 p-3 text-[13px] text-bad" data-testid="lab-refused">
              {answer.refused ?? failure}
            </p>
          )}
          <Expect teach={teach} />
        </aside>
        <DialogFooter className="lg:col-span-2">
          <DialogClose asChild>
            <Button type="button" variant="outline">
              Cancel
            </Button>
          </DialogClose>
          <Button type="submit" disabled={busy} data-testid="lab-start">
            <PlayIcon /> {busy ? "Starting…" : "Start rehearsal"}
          </Button>
        </DialogFooter>
      </form>
    </Form>
  );
}

/** What follows once it starts, from the lesson. */
function Expect({ teach }: { teach?: LabTeach }) {
  return (
    <div className="rounded-xl border bg-card p-4" data-testid="lab-expect">
      <h3 className="mb-2 text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">What to expect</h3>
      {teach ? <p className="text-[13px] leading-5 text-muted-foreground">{teach.expect}</p> : <Skeleton className="h-12 w-full" />}
    </div>
  );
}

function RuleTab({ kind, recipe, teach, onMade }: { kind: "slow" | "fail"; recipe?: LabRecipe; teach?: LabTeach; onMade: (id: number) => void }) {
  const q = useLabForm(kind, true);
  if (q.error) return <p role="alert" className="text-[13.5px] text-bad">{q.error.message}</p>;
  if (!q.data) return <Skeleton className="h-96 w-full" />;
  return <RuleForm key={`${kind}-${recipe?.id ?? ""}`} kind={kind} start={q.data} recipe={recipe} teach={teach} onMade={onMade} />;
}

const HOW: [string, string][] = [
  ["once", "Once, now"],
  ["1", "For 1 min"],
  ["5", "For 5 min"],
];

/** The group of an Expire choice (`group/member`, each part encoded). */
function groupOfMember(value: string): string {
  const g = value.split("/")[0] ?? "";
  try {
    return decodeURIComponent(g);
  } catch {
    return g;
  }
}

/** Where to see what a rehearsal that happens once did. */
function SeeIt({ kind, target }: { kind: "drop" | "expire" | "rebalance"; target: string }) {
  if (kind === "drop") {
    return (
      <Button variant="outline" size="sm" className="bg-card" asChild>
        <Link to="/live" search={{ lab: "all", client: [target] }} data-testid="once-see">
          <ActivityIcon /> See it on Live
        </Link>
      </Button>
    );
  }
  const group = kind === "expire" ? groupOfMember(target) : target;
  return (
    <Button variant="outline" size="sm" className="bg-card" asChild>
      <Link to="/groups/$group/story" params={{ group }} data-testid="once-see">
        <ScrollTextIcon /> Read it in {group}'s Story
      </Link>
    </Button>
  );
}

function OnceTab({ kind, recipe, teach, onDone }: { kind: "drop" | "expire" | "rebalance"; recipe?: LabRecipe; teach?: LabTeach; onDone: () => void }) {
  const qc = useQueryClient();
  const [target, setTarget] = useState(recipe?.target ?? "");
  const [how, setHow] = useState(recipe?.how ?? "once");
  const q = useLabOnce(kind, qs({ target, how: kind === "drop" ? how : undefined }), true);
  const v = q.data;
  const [refused, setRefused] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [did, setDid] = useState<string | null>(null);
  const options = useMemo<Option[]>(() => (v?.choices ?? []).map((c) => ({ value: c.value, label: c.label, hint: undefined })), [v?.choices]);
  const desc = v?.choices.find((c) => c.value === target)?.desc;
  const title = teach?.title ?? TAB_LOOK[kind].label;
  const go = async () => {
    setBusy(true);
    setRefused(null);
    setError(null);
    try {
      const out = await post<LabOnceOutcome>(`/api/v1/lab/once/${kind}`, { target, how });
      if (out.outcome === "done") {
        setDid(kind === "drop" && how !== "once" ? `${target}: its connections close for ${how} min.` : `Done: ${target}.`);
        void qc.invalidateQueries({ queryKey: ["lab"] });
      } else {
        setRefused(out.form.refused);
        setError(out.form.error);
      }
    } catch (e) {
      setRefused(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const noun = kind === "drop" ? "app" : kind === "expire" ? "member" : "group";
  if (did) {
    return (
      <div className="flex min-w-0 flex-col gap-5" data-testid={`lab-once-${kind}`}>
        <Done testId="once-done">
          <b className="font-semibold text-bright">{title}.</b> {did} It leaves a line in the Lab's history.
        </Done>
        <div className="flex flex-wrap items-center gap-3">
          <SeeIt kind={kind} target={target} />
          {teach && <p className="min-w-0 flex-1 basis-[20rem] text-[13px] text-muted-foreground">{teach.watch}</p>}
        </div>
        <DoneFooter onDone={onDone} />
      </div>
    );
  }
  return (
    <div className="flex min-w-0 flex-col gap-5" data-testid={`lab-once-${kind}`}>
      {!v ? (
        <Skeleton className="h-40 w-full" />
      ) : v.nothing ? (
        <p role="status" className="rounded-lg border bg-well p-4 text-[13.5px]" data-testid="lab-nothing">
          {v.nothing}
        </p>
      ) : (
        <>
          <div className="flex flex-col gap-2">
            <Label htmlFor={`once-${kind}`}>{kind === "drop" ? "App" : kind === "expire" ? "Member" : "Group"}</Label>
            <Combobox id={`once-${kind}`} options={options} value={target || null} onChange={(x) => setTarget(x ?? "")} placeholder={`Choose the ${noun}…`} search={`Search ${noun}s…`} aria-label={`The ${noun}`} className="w-full" />
            {desc && <p className="text-[12.5px] text-muted-foreground">{desc}</p>}
            {error && (
              <p role="alert" className="text-[12.5px] text-bad">
                {error}
              </p>
            )}
          </div>
          {kind === "drop" && (
            <div className="flex flex-col gap-2">
              <Label id="drop-how">How</Label>
              <ToggleGroup type="single" variant="outline" value={how} onValueChange={(x) => x && setHow(x)} aria-labelledby="drop-how" className="self-start bg-card">
                {HOW.map(([value, label]) => (
                  <ToggleGroupItem key={value} value={value} className="px-3">
                    {label}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
              <p className="text-[12.5px] text-muted-foreground">Once closes the connections it has open now; for a while is a rule that closes each new one too, and ends by itself.</p>
            </div>
          )}
          {v.note && <p className="text-[13px] text-muted-foreground">{v.note}</p>}
          {v.call && (
            <pre className="overflow-x-auto rounded-lg border bg-well p-3 font-mono text-[12.5px]" data-testid="once-call">
              {v.call}
            </pre>
          )}
          {v.follows && <p className="text-[13px] text-muted-foreground">{v.follows}</p>}
          <Expect teach={teach} />
        </>
      )}
      {refused && (
        <p role="alert" className="rounded-lg border border-bad/30 bg-bad/5 p-3 text-[13px] text-bad" data-testid="lab-refused">
          {refused}
        </p>
      )}
      <DialogFooter>
        <DialogClose asChild>
          <Button type="button" variant="outline">
            Cancel
          </Button>
        </DialogClose>
        <Button disabled={busy || !target || !!v?.nothing} onClick={() => void go()} data-testid="lab-once-go">
          <PlayIcon /> {busy ? "Working…" : title}
        </Button>
      </DialogFooter>
    </div>
  );
}

function CompactTab({ topics, recipe, teach, onDone }: { topics: string[]; recipe?: LabRecipe; teach?: LabTeach; onDone: () => void }) {
  const [topic, setTopic] = useState(recipe?.target ?? topics[0] ?? "");
  const [partition, setPartition] = useState("0");
  if (topics.length === 0) {
    return (
      <p role="status" className="rounded-lg border bg-well p-4 text-[13.5px]" data-testid="lab-nothing">
        No topic compacts (cleanup.policy=compact), so there is no cleaner round to run.
      </p>
    );
  }
  return (
    <div className="flex min-w-0 flex-col gap-5" data-testid="lab-once-compact">
      <div className="grid gap-4 sm:grid-cols-[1fr_10rem]">
        <div className="flex min-w-0 flex-col gap-2">
          <Label htmlFor="compact-topic">Compacted topic</Label>
          <Combobox id="compact-topic" options={topics.map((t) => ({ value: t, label: t }))} value={topic || null} onChange={(t) => setTopic(t ?? "")} aria-label="Compacted topic" className="w-full" />
        </div>
        <div className="flex flex-col gap-2">
          <Label htmlFor="compact-partition">Partition</Label>
          <Input id="compact-partition" value={partition} onChange={(e) => setPartition(e.target.value.replace(/\D/g, "").slice(0, 4))} inputMode="numeric" className="bg-card font-mono" />
        </div>
      </div>
      <Expect teach={teach} />
      {topic && partition !== "" && <CompactNowBody key={`${topic}-${partition}`} topic={topic} partition={Number(partition)} onDone={onDone} footer={false} />}
    </div>
  );
}

/** The recipe the dialog was opened from, as a band over the form: the question it answers. */
function RecipeBand({ recipe }: { recipe: LabRecipe }) {
  return (
    <div className="flex items-start gap-3 rounded-lg border border-primary/40 bg-primary/10 p-3 text-[13px] leading-5" data-testid="lab-recipe-band">
      <BookMarkedIcon className="mt-0.5 size-4 flex-none text-link" aria-hidden />
      <p className="min-w-0">
        <b className="font-semibold text-bright">Recipe: {recipe.title}.</b> {recipe.question} Review the rehearsal, then start it.
      </p>
    </div>
  );
}

/** The dialog; `tab` is the `new` search param, `recipe` the recipe it was filled from (only on that recipe's tab). */
export function NewRehearsalDialog({ tab, recipe: recipeId, guide, compactTopics, onTab, onClose, onMade }: { tab: LabTab | undefined; recipe?: string; guide: LabGuide | undefined; compactTopics: string[]; onTab: (t: LabTab) => void; onClose: () => void; onMade: (id: number) => void }) {
  const teach = tab ? teachOf(guide, tab) : undefined;
  const recipe = guide?.recipes.find((r) => r.id === recipeId && r.tab === tab && !r.needs);
  // A recipe named in the address waits for the guide, so the form opens filled, never with the defaults first.
  const waiting = !!recipeId && !guide;
  const look = (t: LabTab) => ({ recipe: recipe?.tab === t ? recipe : undefined, teach: teachOf(guide, t) });
  return (
    <Dialog open={!!tab} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="top-6 max-h-[calc(100vh-3rem)] translate-y-0 overflow-y-auto [scrollbar-gutter:stable] sm:max-w-[56rem]" data-testid="lab-dialog">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-[20px] font-medium">
            <FlaskConicalIcon className="size-5 text-warn" aria-hidden /> New rehearsal
          </DialogTitle>
          <DialogDescription>{teach ? `${teach.title}. ${teach.lede}` : "Rehearse a failure on purpose and watch what your app does."}</DialogDescription>
        </DialogHeader>
        {tab && (
          <Tabs value={tab} onValueChange={(t) => onTab(t as LabTab)} className="min-w-0">
            <TabsList className="flex h-auto w-full flex-wrap justify-start" data-testid="lab-tabs">
              {(Object.keys(TAB_LOOK) as LabTab[]).map((t) => {
                const Icon = TAB_LOOK[t].icon;
                return (
                  <TabsTrigger key={t} value={t} className="flex-none px-3">
                    <Icon aria-hidden /> {TAB_LOOK[t].label}
                    {TAB_LOOK[t].once && <span className="ml-1 text-[11px] text-muted-foreground">once</span>}
                  </TabsTrigger>
                );
              })}
            </TabsList>
            <div className="flex flex-col gap-3 pt-4">
              {recipe && <RecipeBand recipe={recipe} />}
              <Lesson key={tab} teach={teach} />
            </div>
            {waiting ? (
              <Skeleton className="mt-4 h-96 w-full" />
            ) : (
              <>
                <TabsContent value="slow" className="pt-4">
                  <RuleTab kind="slow" {...look("slow")} onMade={onMade} />
                </TabsContent>
                <TabsContent value="fail" className="pt-4">
                  <RuleTab kind="fail" {...look("fail")} onMade={onMade} />
                </TabsContent>
                <TabsContent value="drop" className="pt-4">
                  <OnceTab key={recipe?.id} kind="drop" {...look("drop")} onDone={onClose} />
                </TabsContent>
                <TabsContent value="expire" className="pt-4">
                  <OnceTab key={recipe?.id} kind="expire" {...look("expire")} onDone={onClose} />
                </TabsContent>
                <TabsContent value="rebalance" className="pt-4">
                  <OnceTab key={recipe?.id} kind="rebalance" {...look("rebalance")} onDone={onClose} />
                </TabsContent>
                <TabsContent value="compact" className="pt-4">
                  <CompactTab key={recipe?.id} topics={compactTopics} {...look("compact")} onDone={onClose} />
                </TabsContent>
              </>
            )}
          </Tabs>
        )}
      </DialogContent>
    </Dialog>
  );
}

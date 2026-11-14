// Register a schema (a new subject, or a new version of one): the draft is checked as it is typed, against the versions the
// subject's level calls for, and every reason it would be refused is said in the registry's (Confluent's) words with Go's one
// fix (a default for a field old data lacks). Registering is the same call a serializer makes; a refusal stays on the page.
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { CircleCheckIcon, PlusIcon, SendIcon, WandSparklesIcon, XIcon } from "lucide-react";
import { useEffect, useMemo, useRef } from "react";
import { Controller, useFieldArray, useForm, useWatch } from "react-hook-form";
import { toast } from "sonner";
import { useCheck, useRegister, useSubject } from "@/api/schemas";
import { useDebounced } from "@/api/topic-writes";
import type { CheckAnswer } from "@/api/types/CheckAnswer";
import type { FindingView } from "@/api/types/FindingView";
import type { SchemaPost } from "@/api/types/SchemaPost";
import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Callout } from "../groups/parts";
import { Refusal } from "./dialogs";
import { KIND_WORD, KINDS, type Kind } from "./search";

const route = getRouteApi("/schemas/new");

type Values = { subject: string; kind: Kind; schema: string; refs: { name: string; subject: string; version: string }[]; normalize: boolean };

const STARTER: Record<Kind, string> = {
  AVRO: '{\n  "type": "record",\n  "name": "Order",\n  "namespace": "shop",\n  "fields": [\n    {"name": "id", "type": "long"}\n  ]\n}',
  JSON: '{\n  "type": "object",\n  "properties": {\n    "id": {"type": "integer"}\n  },\n  "required": ["id"]\n}',
  PROTOBUF: 'syntax = "proto3";\npackage shop;\n\nmessage Order {\n  int64 id = 1;\n}\n',
};

const SAYS: Record<Kind, string> = {
  AVRO: "In Avro, a field added with a default can be read from older messages.",
  PROTOBUF: "In Protobuf, adding a field with a new number is wire-compatible.",
  JSON: "In JSON Schema, a property added to a closed object (additionalProperties false) is allowed.",
};

function asPost(v: Values): SchemaPost {
  return {
    subject: v.subject.trim(),
    schema_type: v.kind,
    schema: v.schema,
    references: v.refs.filter((r) => r.name || r.subject).map((r) => ({ name: r.name.trim(), subject: r.subject.trim(), version: Number(r.version) || 0 })),
    normalize: v.normalize,
    level: null,
  };
}

function Findings({ findings, onFix }: { findings: FindingView[]; onFix: (schema: string) => void }) {
  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      <table className="w-full table-fixed text-[13px]" data-testid="findings" aria-label="Why it is not compatible">
        <colgroup>
          <col style={{ width: "3rem" }} />
          <col style={{ width: "30%" }} />
          <col />
          <col style={{ width: "11rem" }} />
        </colgroup>
        <thead className="bg-well text-[11.5px] tracking-[0.04em] text-muted-foreground uppercase">
          <tr>
            <th className="h-9 px-3 text-left font-medium">#</th>
            <th className="h-9 px-3 text-left font-medium">Incompatibility</th>
            <th className="h-9 px-3 text-left font-medium">Where and why</th>
            <th className="h-9 px-3 text-left font-medium">Fix</th>
          </tr>
        </thead>
        <tbody>
          {findings.flatMap((f) =>
            f.reasons.map((r, i) => (
              <tr key={`${f.version}-${i}`} className="border-t align-top" data-testid="finding">
                <td className="px-3 py-2.5 font-mono text-muted-foreground">v{f.version}</td>
                <td className="px-3 py-2.5 font-mono text-[12px] break-words text-bright">{r.kind}</td>
                <td className="px-3 py-2.5 break-words">
                  {r.path && <span className="mr-1 font-mono text-[12px] text-muted-foreground">{r.path}:</span>}
                  {r.description}
                </td>
                <td className="px-3 py-2.5">
                  {r.fix ? (
                    <Button size="sm" variant="outline" onClick={() => onFix(r.fix!.schema)} title={r.fix.label} data-testid="apply-fix" className="h-auto max-w-full py-1 text-left whitespace-normal">
                      <WandSparklesIcon /> {r.fix.label}
                    </Button>
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </td>
              </tr>
            )),
          )}
        </tbody>
      </table>
    </div>
  );
}

function Verdict({ a, kind, onFix }: { a: CheckAnswer; kind: Kind; onFix: (schema: string) => void }) {
  switch (a.outcome) {
    case "first":
      return (
        <Callout tone="ok" data-testid="check-first">
          <b className="font-semibold text-bright">Parses.</b> A new subject: this would be its first version, v1, compared with nothing.
        </Callout>
      );
    case "compatible":
      return (
        <Callout tone="ok" data-testid="check-compatible">
          <b className="font-semibold text-bright">Compatible</b> with {a.compared.map((v) => `v${v}`).join(", ") || "every version"} at {a.level}. {SAYS[kind]}
        </Callout>
      );
    case "incompatible":
      return (
        <div className="flex flex-col gap-3" data-testid="check-incompatible">
          <Callout tone="refused">
            <b className="font-semibold text-bright">Not compatible at {a.level}.</b> Registering it would be refused (409); nothing would be stored. Compared with {a.compared.map((v) => `v${v}`).join(", ")}.
          </Callout>
          <Findings findings={a.findings} onFix={onFix} />
        </div>
      );
    case "invalid":
      return (
        <Callout tone="refused" data-testid="check-invalid">
          <b className="font-semibold text-bright">Not a schema the registry takes (error_code {a.error_code}).</b> <span className="break-words">{a.message}</span>
        </Callout>
      );
  }
}

export function NewSchemaPage() {
  const search = route.useSearch();
  const navigate = useNavigate();
  const existing = useSubject(search.subject ?? "", undefined);
  const known = !!search.subject && !!existing.data;
  const form = useForm<Values>({ defaultValues: { subject: search.subject ?? "", kind: search.type ?? "AVRO", schema: STARTER[search.type ?? "AVRO"], refs: [], normalize: false } });
  const refs = useFieldArray({ control: form.control, name: "refs" });
  // An existing subject starts from its latest version, in its own language.
  const seeded = useRef(false);
  useEffect(() => {
    if (seeded.current || !existing.data) return;
    const s = existing.data.shown;
    form.reset({ subject: search.subject ?? "", kind: s.kind as Kind, schema: s.schema, refs: s.references.map((r) => ({ name: r.name, subject: r.subject, version: String(r.version) })), normalize: false });
    seeded.current = true;
  }, [existing.data, form, search.subject]);
  const watched = useWatch({ control: form.control }) as Values;
  const post = useMemo(() => asPost(watched), [watched]);
  const asked = useDebounced(post, 400);
  const check = useCheck(asked);
  const register = useRegister();
  const refused = register.data?.outcome === "refused" ? register.data : null;
  const kind = watched.kind ?? "AVRO";
  const lines = (watched.schema ?? "").split("\n").length;
  const onFix = (schema: string) => {
    form.setValue("schema", schema, { shouldDirty: true });
    toast.message("The fix is in the draft. Nothing was registered.");
  };
  const submit = form.handleSubmit((v) =>
    register.mutate(asPost(v), {
      onSuccess: (w) => {
        if (w.outcome !== "registered") return;
        const subject = v.subject.trim();
        toast.success(w.existed ? `Already registered as v${w.version}, id ${w.id}: nothing was created.` : w.shared.length ? `Registered v${w.version} with id ${w.id}, the id this schema already has under ${w.shared.join(" and ")}.` : `Registered v${w.version}, id ${w.id}.`);
        void navigate({ to: "/schemas/subjects/$subject", params: { subject } });
      },
    }),
  );
  const title = known ? `New version of ${search.subject}` : "Register a schema";
  return (
    <>
      <PageHeader title={title} crumbs={known ? [{ label: "Schemas", to: "/schemas" }, { label: search.subject!, to: `/schemas/subjects/${encodeURIComponent(search.subject!)}` }] : [{ label: "Schemas", to: "/schemas" }]} sub="Checked as you type against the subject's versions at its level, as registering would check it. Nothing is stored until you register." />
      <form onSubmit={submit} className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]" data-testid="register-form" noValidate>
        <div className="flex min-w-0 flex-col gap-5">
          <div className="flex flex-col gap-2">
            <Label htmlFor="subject">Subject</Label>
            <Input id="subject" className="font-mono" placeholder="orders-value" autoComplete="off" spellCheck={false} readOnly={known} {...form.register("subject", { required: "give the subject a name, such as orders-value" })} data-testid="subject" />
            <p className="text-[12.5px] text-muted-foreground">{form.formState.errors.subject?.message ?? "A topic's values are <topic>-value and its keys <topic>-key (TopicNameStrategy), which is what serializers use unless told otherwise."}</p>
          </div>
          <div className="flex flex-col gap-2">
            <span className="text-sm font-medium" id="kind-l">
              Type
            </span>
            <Controller
              control={form.control}
              name="kind"
              render={({ field }) => (
                <ToggleGroup
                  type="single"
                  variant="outline"
                  aria-labelledby="kind-l"
                  value={field.value}
                  disabled={known}
                  onValueChange={(v) => {
                    if (!v) return;
                    const was = form.getValues("schema");
                    field.onChange(v);
                    if (was === STARTER[field.value]) form.setValue("schema", STARTER[v as Kind]);
                  }}
                  className="self-start"
                  data-testid="kind"
                >
                  {KINDS.map((k) => (
                    <ToggleGroupItem key={k} value={k} className="px-3">
                      {KIND_WORD[k]}
                    </ToggleGroupItem>
                  ))}
                </ToggleGroup>
              )}
            />
            {known && <p className="text-[12.5px] text-muted-foreground">A subject keeps one language: a new version of {search.subject} is {KIND_WORD[kind]}.</p>}
          </div>
          <div className="flex flex-col gap-2">
            <div className="flex items-baseline justify-between">
              <Label htmlFor="schema">Schema</Label>
              <span className="text-[12px] text-muted-foreground tabular-nums">
                {KIND_WORD[kind]} · {lines} {lines === 1 ? "line" : "lines"}
              </span>
            </div>
            <Textarea id="schema" rows={18} spellCheck={false} className="max-h-[32rem] min-h-72 font-mono text-[12.5px] leading-5" {...form.register("schema", { required: "paste the schema" })} data-testid="schema" />
          </div>
          <fieldset className="flex flex-col gap-2">
            <legend className="mb-2 text-sm font-medium">
              References <span className="font-normal text-muted-foreground">{refs.fields.length ? "" : "none"}</span>
            </legend>
            {refs.fields.map((r, i) => (
              <div key={r.id} className="grid grid-cols-[1fr_1fr_5rem_auto] gap-2" data-testid="ref-row">
                <Input className="font-mono" placeholder="name used in the schema" aria-label={`Reference name ${i + 1}`} {...form.register(`refs.${i}.name`)} />
                <Input className="font-mono" placeholder="subject" aria-label={`Reference subject ${i + 1}`} {...form.register(`refs.${i}.subject`)} />
                <Input className="font-mono" placeholder="1" inputMode="numeric" aria-label={`Reference version ${i + 1}`} {...form.register(`refs.${i}.version`)} />
                <Button type="button" variant="ghost" size="icon" aria-label={`Remove reference ${i + 1}`} onClick={() => refs.remove(i)}>
                  <XIcon />
                </Button>
              </div>
            ))}
            <div>
              <Button type="button" variant="outline" size="sm" onClick={() => refs.append({ name: "", subject: "", version: "1" })} data-testid="add-ref">
                <PlusIcon /> Add reference
              </Button>
            </div>
          </fieldset>
          <div className="flex items-center gap-2">
            <Controller control={form.control} name="normalize" render={({ field }) => <Checkbox id="normalize" checked={field.value} onCheckedChange={(c) => field.onChange(c === true)} />} />
            <Label htmlFor="normalize" className="font-normal">
              Normalize before storing (<code className="font-mono">?normalize=true</code>)
            </Label>
          </div>
        </div>
        <div className="flex min-w-0 flex-col gap-4">
          <h2 className="text-[15px] font-semibold text-bright">The check</h2>
          <div className="min-h-24" aria-live="polite" data-testid="check">
            {check.data ? <Verdict a={check.data} kind={kind} onFix={onFix} /> : <p className="text-[13px] text-muted-foreground">Type a subject and a schema: the registry checks it as you go.</p>}
            {check.isFetching && <p className="mt-2 text-[12px] text-muted-foreground">Checking…</p>}
          </div>
          {refused && (
            <div className="flex flex-col gap-3">
              <Refusal r={refused} />
              {refused.error_code === 409 && <p className="text-[13px] text-muted-foreground">Confluent Schema Registry refuses the same call with the same status, error code and reasons.</p>}
              {refused.error_code === 42205 && (
                <p className="text-[13px] text-muted-foreground">
                  The subject is READONLY. Change its mode on{" "}
                  <Link to="/schemas/subjects/$subject" params={{ subject: post.subject }} search={{ dialog: "compatibility" }} className="text-link hover:underline">
                    its page
                  </Link>
                  .
                </p>
              )}
            </div>
          )}
          <div className="flex flex-wrap items-center gap-3 border-t pt-4">
            <Button type="submit" disabled={register.isPending} data-testid="register">
              <SendIcon /> Register
            </Button>
            <Button type="button" variant="outline" asChild>
              <Link to={known ? "/schemas/subjects/$subject" : "/schemas"} params={{ subject: search.subject ?? "" }}>
                Cancel
              </Link>
            </Button>
            {check.data?.outcome === "compatible" && (
              <span className="flex items-center gap-1.5 text-[13px] text-good">
                <CircleCheckIcon className="size-4" /> Ready to register
              </span>
            )}
          </div>
        </div>
      </form>
    </>
  );
}

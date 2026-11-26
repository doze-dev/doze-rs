import { SendIcon, UndoIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Button } from "@doze/ui/components/ui/button";
import { Label } from "@doze/ui/components/ui/label";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { clockUtc, number } from "@doze/ui/lib/format";
import { useLab, useLabOperation, useLabSend } from "../../api/spaces";
import { CopyCalls, callJson, usePreview } from "../../kit";
import { Card, CardSkeleton } from "../card";
import { Answer } from "./answer";
import { BodyBox, bytesOf, splitBody, withBody } from "./body-box";
import { type Ask, addSent, newId, parseSent, sentFrom, type Sent } from "./history";
import { SENT_KEY, readKept, writeKept } from "./kept";
import { draftFor, inputText, setDraft, useDraft } from "./draft";
import { MemberList } from "./members";
import { SavedList, SaveInput } from "./saved-list";
import { SentList } from "./sent-list";
import { type LabSearch, readInput } from "./search";
import type { Saved } from "./saved";

/** Send a call: any operation doze-aws has built, an input started from AWS's model, sent as this console; the answer here and in Live. */
export function SendTab({ api, op, pick }: { api: string; op: string; pick: (next: LabSearch) => void }) {
  const lab = useLab();
  const operation = useLabOperation(api, op);
  const draft = draftFor(useDraft(), api, op);
  const targets = useMemo(() => lab.data?.targets ?? [], [lab.data]);
  const target = targets.find((t) => t.target === api);
  const record = (sent: { target: string; op: string; input: unknown }, answer: Parameters<typeof sentFrom>[1]) => {
    const t = targets.find((x) => x.target === sent.target);
    if (!t) return;
    const now = Date.now();
    const entry = sentFrom({ target: t.target, service: t.service, title: t.title, op: sent.op, input: sent.input as Record<string, unknown> }, answer, newId(now), now);
    writeKept(SENT_KEY, addSent(readKept(SENT_KEY, parseSent), entry));
  };
  const send = useLabSend(record);
  // The body of a put, by the operation it is for: kept while the page is open, as the draft is.
  const [bodies, setBodies] = useState<Record<string, string>>({});
  const text = draft?.text ?? operation.data?.example ?? "";
  const edited = draft !== null && operation.data !== undefined && draft.text !== operation.data.example;
  const input = readInput(text);
  const bodyMax = operation.data?.body_max ?? null;
  const body = bodies[`${api}/${op}`] ?? "";
  const overBody = bodyMax !== null && bytesOf(body) > bodyMax;
  const apis = useMemo(() => targets.map((t) => ({ value: t.target, label: `${t.title}${t.sdk_id !== t.title ? ` · ${t.sdk_id}` : ""}`, hint: `${number(t.operations.length)} operations${t.served ? "" : " · not served"}` })), [targets]);
  const ops = useMemo(() => (target?.operations ?? []).map((o) => ({ value: o, label: o })), [target]);
  const preview = usePreview(target && op && input.ok ? { service: target.service, operation: op, input: input.value } : null);
  const ask: Ask | null = target && op && input.ok ? { target: target.target, service: target.service, title: target.title, op, input: bodyMax !== null ? withBody(input.value, body) : input.value } : null;
  const setText = (t: string) => setDraft({ target: api, op, text: t, note: draft?.note ?? null });
  const choose = (next: LabSearch) => (send.reset(), pick(next));
  const go = (e: React.FormEvent) => {
    e.preventDefault();
    if (ask) send.mutate({ target: ask.target, op: ask.op, input: ask.input });
  };
  const replay = (s: Sent | Saved, note: string) => {
    if (!s.input) return;
    // A put's body goes back into its box, so the editor holds the rest.
    const parts = s.op === "PutObject" ? splitBody(s.input) : { input: s.input, body: "" };
    setBodies((b) => ({ ...b, [`${s.target}/${s.op}`]: parts.body }));
    setDraft({ target: s.target, op: s.op, text: inputText(parts.input), note });
    send.reset();
    pick({ tab: "send", api: s.target, op: s.op });
  };
  const failed = send.error ? errorWords(send.error) : null;
  const cannot = operation.data?.cannot ?? null;
  if (lab.isError && !lab.data) return <ErrorBlock what="The operations could not be read" error={lab.error} hint="Check that doze-aws is still running." onRetry={() => void lab.refetch()} retrying={lab.isFetching} className="mb-10" testId="lab-error" />;
  if (!lab.data) return <CardSkeleton rows={4} />;
  return (
    <>
      <div className="mb-10 grid gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <Card testId="lab-compose" title="Send a call" lede="Calls go through the service's own model and IAM, as the caller console (the account root: allowed everything). They are the console's own calls: nothing made in the Lab bends them.">
          <form onSubmit={go} className="grid gap-5">
            <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
              <div className="grid gap-1.5">
                <Label htmlFor="lab-api">Service</Label>
                <Combobox id="lab-api" aria-label="Service" options={apis} value={api || null} onChange={(v) => choose({ tab: "send", api: v ?? undefined, op: undefined })} placeholder="Choose a service" search="Search services" className="w-full min-w-0" />
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="lab-op">Operation</Label>
                <Combobox id="lab-op" aria-label="Operation" options={ops} value={op || null} onChange={(v) => choose({ tab: "send", api, op: v ?? undefined })} placeholder={api ? "Choose an operation" : "Choose a service first"} search="Search operations" className="w-full min-w-0" />
              </div>
            </div>
            {target && !target.served && (
              <Alert tone="warn" title={`${target.title} is not served in this run`}>
                Started without it in --services: a call is answered with how to turn it on.
              </Alert>
            )}
            {operation.isError && <ErrorBlock what="The operation could not be read" error={operation.error} testId="lab-op-error" />}
            {operation.data && (
              <>
                {draft?.note && <p className="text-[13px] text-muted-foreground" data-testid="lab-note">{draft.note}</p>}
                <div className="grid gap-1.5">
                  <div className="flex items-center justify-between gap-3">
                    <Label htmlFor="lab-input">Input</Label>
                    {edited && (
                      <Button type="button" variant="ghost" size="xs" onClick={() => setDraft(null)} data-testid="lab-reset">
                        <UndoIcon /> Start from the example again
                      </Button>
                    )}
                  </div>
                  <Textarea id="lab-input" value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} rows={12} className="bg-card font-mono text-[12.5px] leading-5" data-testid="lab-input" aria-invalid={!input.ok} />
                  {!input.ok && <p className="text-[12.5px] text-destructive" data-testid="lab-input-why">{input.why}</p>}
                </div>
                {cannot && <Alert tone="warn" title="This call cannot be sent from the Lab" testId="lab-cannot">{cannot}</Alert>}
                {bodyMax !== null && <BodyBox value={body} onChange={(v) => setBodies((b) => ({ ...b, [`${api}/${op}`]: v }))} max={bodyMax} />}
                <MemberList members={operation.data.members} />
                <div className="flex flex-wrap items-center gap-3">
                  <Button type="submit" disabled={!ask || send.isPending || overBody || cannot !== null} data-testid="lab-send">
                    <SendIcon /> {send.isPending ? "Sending…" : `Send ${op}`}
                  </Button>
                  <SaveInput ask={ask} />
                </div>
                <p className="-mt-2 text-[12.5px] text-muted-foreground">A change is real: it is made in this doze-aws&apos;s data.</p>
                {preview.data && <CopyCalls cli={preview.data.cli} boto3={preview.data.boto3} json={callJson(preview.data, input.ok ? input.value : undefined)} format="json" waiting={preview.settling} />}
              </>
            )}
          </form>
        </Card>
        <div className="grid min-w-0 content-start gap-6 [&>*]:min-w-0">
          {failed && (
            <Alert tone="bad" title={failed.code ? `Not sent: ${failed.code}` : "Not sent"} testId="lab-send-error">
              {failed.message}
            </Alert>
          )}
          {send.data ? <Answer answer={send.data} op={send.variables?.op ?? op} /> : <div className="rounded-[var(--radius-card)] border border-dashed bg-card px-6 py-10 text-center text-[13.5px] text-muted-foreground">The answer shows here, with a link to the call in Live.</div>}
        </div>
      </div>
      <div className="grid gap-x-6 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <SentList busy={send.isPending} onReplay={(s) => replay(s, `Replayed: the ${s.op} sent at ${clockUtc(s.at)} UTC.`)} onSend={(s) => s.input && send.mutate({ target: s.target, op: s.op, input: s.input })} />
        <SavedList onOpen={(s) => replay(s, `Opened from the saved input ${s.name}.`)} />
      </div>
    </>
  );
}

import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { HistoryIcon, RadioIcon, SendIcon, ZapIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Alert, ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { Combobox } from "@doze/ui/components/app/combobox";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Label } from "@doze/ui/components/ui/label";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { micros, number } from "@doze/ui/lib/format";
import { useLab, useLabOperation, useLabSend } from "../../api/spaces";
import type { LabAnswer } from "../../api/types/LabAnswer";
import { Card, CardSkeleton, CodeBlock, Section } from "../card";
import { type LabSearch, readInput } from "./search";

function Answer({ answer }: { answer: LabAnswer }) {
  return (
    <Card
      testId="lab-answer"
      title={answer.error ? <StatusDot tone="bad">{answer.error.code}</StatusDot> : <StatusDot tone="ok">Answered</StatusDot>}
      lede={`In ${micros(Number(answer.took_us))}${answer.error ? `, status ${answer.error.status}` : ""}.`}
      actions={
        answer.seq !== null ? (
          <Button asChild variant="outline" size="sm" className="bg-card">
            <Link to="/live" search={{ call: Number(answer.seq), console: "shown" }} data-testid="lab-in-live">
              <RadioIcon /> Open in Live
            </Link>
          </Button>
        ) : undefined
      }
    >
      {answer.error ? <p className="text-[13.5px] text-foreground">{answer.error.message}</p> : <CodeBlock code={answer.output ?? ""} what="Output copied" testId="lab-output" />}
    </Card>
  );
}

/** The Lab: send any operation doze-aws has built, through the services' bus as this console, with an input started from the
 * model; see the answer and its entry in Live. Fault rules and checkpoints, the rest of the design's Lab, are not built. */
export function LabPage() {
  const search = useSearch({ from: "/lab" });
  const navigate = useNavigate({ from: "/lab" });
  const lab = useLab();
  const api = search.api ?? "";
  const op = search.op ?? "";
  const operation = useLabOperation(api, op);
  const send = useLabSend();
  // What the person typed, for the operation it was typed for; another operation starts from its own example.
  const opened = `${api}/${op}`;
  const [edit, setEdit] = useState<{ at: string; text: string } | null>(null);
  const text = edit?.at === opened ? edit.text : (operation.data?.example ?? "");
  const setText = (t: string) => setEdit({ at: opened, text: t });
  const pick = (next: LabSearch) => {
    send.reset();
    void navigate({ search: next });
  };
  const targets = useMemo(() => lab.data?.targets ?? [], [lab.data]);
  const target = targets.find((t) => t.target === api);
  const apis = useMemo(() => targets.map((t) => ({ value: t.target, label: `${t.title}${t.sdk_id !== t.title ? ` · ${t.sdk_id}` : ""}`, hint: `${number(t.operations.length)} operations${t.served ? "" : " · not served"}` })), [targets]);
  const ops = useMemo(() => (target?.operations ?? []).map((o) => ({ value: o, label: o })), [target]);
  const input = readInput(text);
  const go = (e: React.FormEvent) => {
    e.preventDefault();
    if (!input.ok || !api || !op) return;
    send.mutate({ target: api, op, input: input.value });
  };
  const failed = send.error ? errorWords(send.error) : null;
  return (
    <>
      <PageHeader title="Lab" sub="Try any operation doze-aws has built: an input started from AWS's model, sent as this console, the answer here and in Live." />
      {lab.isError && !lab.data ? (
        <ErrorBlock what="The operations could not be read" error={lab.error} hint="Check that doze-aws is still running." onRetry={() => void lab.refetch()} retrying={lab.isFetching} className="mb-10" testId="lab-error" />
      ) : !lab.data ? (
        <CardSkeleton rows={4} />
      ) : (
        <div className="mb-10 grid gap-6 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
          <Card testId="lab-compose" title="Send a call" lede="Calls go through the service's own model and IAM, as the caller console (the account root: allowed everything).">
            <form onSubmit={go} className="grid gap-5">
              <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
                <div className="grid gap-1.5">
                  <Label htmlFor="lab-api">Service</Label>
                  <Combobox id="lab-api" aria-label="Service" options={apis} value={api || null} onChange={(v) => pick({ api: v ?? undefined, op: undefined })} placeholder="Choose a service" search="Search services" className="w-full min-w-0" />
                </div>
                <div className="grid gap-1.5">
                  <Label htmlFor="lab-op">Operation</Label>
                  <Combobox id="lab-op" aria-label="Operation" options={ops} value={op || null} onChange={(v) => pick({ api, op: v ?? undefined })} placeholder={api ? "Choose an operation" : "Choose a service first"} search="Search operations" className="w-full min-w-0" />
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
                  <div className="grid gap-1.5">
                    <Label htmlFor="lab-input">Input</Label>
                    <Textarea id="lab-input" value={text} onChange={(e) => setText(e.target.value)} spellCheck={false} rows={12} className="bg-card font-mono text-[12.5px] leading-5" data-testid="lab-input" aria-invalid={!input.ok} />
                    {!input.ok && <p className="text-[12.5px] text-destructive">{input.why}</p>}
                  </div>
                  <details className="text-[13px]" data-testid="lab-members">
                    <summary className="cursor-pointer text-muted-foreground">{number(operation.data.members.length)} input members in the model</summary>
                    <ul className="mt-2 grid gap-1">
                      {operation.data.members.map((m) => (
                        <li key={m.name} className="font-mono text-[12.5px]">
                          {m.name} <span className="text-muted-foreground">{m.kind}</span>
                          {m.required && <span className="ml-1 text-bright">required</span>}
                          {m.values.length > 0 && <span className="text-muted-foreground"> · {m.values.slice(0, 8).join(", ")}{m.values.length > 8 ? ", …" : ""}</span>}
                        </li>
                      ))}
                    </ul>
                  </details>
                  <div className="flex items-center gap-3">
                    <Button type="submit" disabled={!input.ok || send.isPending} data-testid="lab-send">
                      <SendIcon /> {send.isPending ? "Sending…" : `Send ${op}`}
                    </Button>
                    <span className="text-[12.5px] text-muted-foreground">A change is real: it is made in this doze-aws&apos;s data.</span>
                  </div>
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
            {send.data ? <Answer answer={send.data} /> : <div className="rounded-[14px] border border-dashed bg-card px-6 py-10 text-center text-[13.5px] text-muted-foreground">The answer shows here, with a link to the call in Live.</div>}
          </div>
        </div>
      )}
      <Section title="Not built yet" testId="lab-not-built" lede="The design's Lab rehearses failure. These need parts doze-aws does not have yet, so they are not here.">
        <div className="grid gap-4 md:grid-cols-2">
          <div className="rounded-xl border bg-card px-5 py-4">
            <div className="flex items-center gap-2 font-semibold text-bright">
              <ZapIcon className="size-4 text-muted-foreground" aria-hidden /> Fault rules
            </div>
            <p className="mt-1.5 text-[13px] text-foreground">Latency, errors and throttling on chosen calls, with an expiry. Needs a hook in the gateway&apos;s request path that doze-aws does not have.</p>
          </div>
          <div className="rounded-xl border bg-card px-5 py-4">
            <div className="flex items-center gap-2 font-semibold text-bright">
              <HistoryIcon className="size-4 text-muted-foreground" aria-hidden /> Checkpoints
            </div>
            <p className="mt-1.5 text-[13px] text-foreground">Save the account&apos;s state and go back to it. Needs every service to pause its writes for a consistent copy, which none can yet. doze-aws data reset (in Settings) is the one rewind there is.</p>
          </div>
        </div>
      </Section>
    </>
  );
}


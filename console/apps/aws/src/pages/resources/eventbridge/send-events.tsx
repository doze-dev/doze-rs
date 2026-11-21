// Send events (eventbridge__send-events): one PutEvents entry on a bus, which of its rules the event matches, and what the call answered.
import { useQueries } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { BusPicker, Field, FormPage, FormSection, callKindOp, useKindOp, useServiceCrumbs } from "../../../kit";
import { useDebounced } from "../../../kit/debounce";
import { BUSES, RULES, useBuses, useRulesOf } from "./api";
import { JsonBox } from "./json-box";
import { useWhere } from "./parts";
import { type SendForm, emptySend, matchHint, putEventsInput, sendProblems, sentWords, wholeEvent } from "./send-model";
import type { PutEventsAnswer, RuleItem, TestAnswer } from "./types";

export function SendEvents({ bus }: { bus: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const { region, account } = useWhere();
  const [f, setF] = useState<SendForm>(emptySend(bus));
  const [tried, setTried] = useState(false);
  const send = useKindOp<PutEventsAnswer>(BUSES, "PutEvents", { keep: true });
  const problems = sendProblems(f);
  const bad = Object.values(problems).find(Boolean) ?? null;
  const rules = useRulesOf(f.bus);
  const buses = useBuses();
  const others = (buses.data?.EventBuses ?? []).map((b) => b.Name).filter((n) => n !== f.bus);
  const otherRules = useQueries({ queries: others.map((n) => ({ queryKey: ["resources", "kindop", "eventbridge", "rules", "ListRules", n, "page"], queryFn: () => callKindOp<{ Rules?: RuleItem[] }>(RULES, "ListRules", { EventBusName: n, Limit: 100 }), retry: false })) });
  const elsewhere = others.flatMap((n, i) => (otherRules[i]?.data?.Rules ?? []).filter((r) => !r.ScheduleExpression).map((r) => ({ bus: n, rule: r.Name })));
  const onBus = (rules.data?.Rules ?? []).filter((r) => !r.ScheduleExpression && r.EventPattern);
  const event = useDebounced(wholeEvent(f, account, region), 300).value;
  const tests = useQueries({ queries: onBus.map((r) => ({ queryKey: ["eventbridge", "test", r.EventPattern, event], queryFn: () => callKindOp<TestAnswer>(RULES, "TestEventPattern", { EventPattern: r.EventPattern, Event: event }), retry: false, enabled: bad === null })) });
  const settled = bad === null && rules.data !== undefined && tests.every((t) => t.data !== undefined);
  const matched = settled ? onBus.filter((_, i) => tests[i]?.data?.Result).map((r) => r.Name) : null;
  const input = useMemo(() => putEventsInput(f), [f]);
  const answer = send.data ? sentWords(send.data) : null;
  const refused = send.error instanceof ApiFailure ? { code: send.error.code, message: send.error.message } : send.error ? { code: "Error", message: String((send.error as Error).message) } : null;
  const hint = matchHint(f.bus, onBus, matched, elsewhere);
  return (
    <FormPage
      title="Send events"
      sub={<>Put one test event on the <b className="font-mono font-semibold text-foreground">{f.bus}</b> bus. It is a PutEvents call with a single entry.</>}
      crumbs={[...crumbs, { label: "Event buses", to: "/resources/eventbridge/buses" }]}
      request={{ service: "events", operation: "PutEvents", input }}
      refusal={refused}
      callNote={<>The reply holds one <b className="font-semibold text-foreground">EventId</b>, or an <b className="font-semibold text-foreground">ErrorCode</b> and <b className="font-semibold text-foreground">ErrorMessage</b> for the entry. To reach rules on another bus, name it in <span className="font-mono">EventBusName</span>.</>}
      banner={answer ? <ResultBanner tone={answer.ok ? "good" : "bad"} title={answer.ok ? "Event sent." : "The event was not accepted."} returned={answer.text} left={answer.ok && matched !== null ? (matched.length > 0 ? `It matched ${matched.join(", ")}, which deliver${matched.length === 1 ? "s" : ""} it to ${matched.length === 1 ? "its" : "their"} targets.` : `It matched no rule on ${f.bus}, so nothing was delivered.`) : undefined} onDismiss={() => send.reset()} testId="sent-banner" /> : undefined}
      submitLabel="Send"
      pending={send.isPending}
      disabled={tried && bad !== null}
      disabledWhy={bad ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "buses" }, search: {} })}
      onSubmit={() => {
        setTried(true);
        if (!bad) send.mutate(input as never);
      }}
      testId="send-events"
    >
      <FormSection title="Event" testId="section-event">
        <Field label="Event bus" testId="field-bus">{(id) => <BusPicker id={id} value={f.bus} onChange={(b) => b && setF({ ...f, bus: b })} />}</Field>
        <Field label="Event source" error={tried ? problems.source : undefined} help={<>Required, along with detail type and detail.</>}>{(id) => <Input id={id} value={f.source} onChange={(e) => setF({ ...f, source: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="send-source" />}</Field>
        <Field label="Detail type" error={tried ? problems.detailType : undefined} help={<>Free-form text, at most <b>128</b> characters. An entry missing a source, a detail type or a detail fails.</>}>{(id) => <Input id={id} value={f.detailType} onChange={(e) => setF({ ...f, detailType: e.target.value })} autoComplete="off" spellCheck={false} className="font-mono" data-testid="send-detail-type" />}</Field>
        <div className="grid gap-1.5">
          <JsonBox value={f.detail} onChange={(detail) => setF({ ...f, detail })} label="Event detail" rows={7} testId="send-detail" />
          {tried && problems.detail && <p className="text-[13px] text-bad" role="alert">{problems.detail}</p>}
          <p className="text-[13px] leading-5 text-muted-foreground" data-testid="match-hint">A valid JSON object. A request, with all its entries, must stay under <b className="font-semibold text-foreground">1 MB</b> (1,048,576 bytes). {hint}</p>
        </div>
      </FormSection>
    </FormPage>
  );
}

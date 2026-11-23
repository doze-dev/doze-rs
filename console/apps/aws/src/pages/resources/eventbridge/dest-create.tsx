// Create API destination (eventbridge__api-destination-create): the endpoint, the method, a rate limit and the connection that signs in to it, an existing one or
// a new one (a page of its own, which comes back here with the new connection chosen). What was typed is kept while it is away.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { Combobox } from "@doze/ui/components/app/combobox";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, LocalStatus, RadioCards, RuleHelp, useServiceCrumbs } from "../../../kit";
import { DESTS, useConnections, useDests, useEbOp } from "./api";
import { type DestForm, METHODS, createDestInput, destProblem, emptyDest, endpointProblem, nameProblem, rateProblem } from "./dest-model";
import { list } from "./words";

/** What was typed, kept when the person goes to Create connection and comes back. */
let kept: DestForm | null = null;

export function DestCreate({ connection }: { connection?: string }) {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("eventbridge");
  const [f, setF] = useState<DestForm>(() => ({ ...(kept ?? emptyDest()), ...(connection ? { connection } : {}) }));
  const [tried, setTried] = useState(false);
  const make = useEbOp(DESTS, "CreateApiDestination");
  const conns = useConnections().data?.Connections ?? [];
  const existing = (useDests().data?.ApiDestinations ?? []).map((d) => d.Name);
  const chosen = conns.find((c) => c.Name === f.connection);
  const problem = destProblem(f, existing);
  const set = (p: Partial<DestForm>) => { setF((x) => { kept = { ...x, ...p }; return { ...x, ...p }; }); make.reset(); };
  const input = useMemo(() => createDestInput(f, chosen?.ConnectionArn ?? "<connection>"), [f, chosen]);
  const refused = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  const others = conns.filter((c) => c.Name !== f.connection).map((c) => c.Name);
  const done = () => { kept = null; };
  return (
    <FormPage
      title="Create API destination"
      sub="An HTTPS endpoint and the connection that signs in to it. Once it exists, choose it as a rule's target."
      crumbs={[...crumbs, { label: "API destinations", to: "/resources/eventbridge/api-destinations" }]}
      request={{ service: "events", operation: "CreateApiDestination", input }}
      refusal={refused}
      callNote="The reply holds the destination's ARN and state. A rule target then names that ARN."
      banner={refused ? <Alert tone="bad" title="The API destination wasn't created." testId="create-refused">CreateApiDestination answered <span className="font-mono font-semibold text-bright">{refused.code}</span>: “{refused.message}”</Alert> : undefined}
      submitLabel="Create"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => { done(); void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "api-destinations" }, search: {} }); }}
      onSubmit={() => {
        setTried(true);
        if (!problem && chosen) make.mutate(input as never, { onSuccess: () => { done(); void navigate({ to: "/resources/$service/$kind/$name", params: { service: "eventbridge", kind: "api-destinations", name: f.name }, search: {} }); } });
      }}
      testId="dest-create"
    >
      <FormSection title="API destination detail" testId="section-detail">
        <Field label="Name" error={tried ? (nameProblem(f.name, "API destination") ?? (existing.includes(f.name) ? `${f.name} already exists.` : undefined) ?? undefined) : undefined} help={<>Up to <b>64</b> characters: letters, digits, dot, hyphen and underscore. Unique in the account and Region.</>} hint={existing.length > 0 ? <><b className="font-mono font-semibold text-foreground">{list(existing.slice(0, 3))}</b> {existing.length === 1 ? "exists" : "exist"}.</> : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="status-page" autoComplete="off" spellCheck={false} className="font-mono" data-testid="dest-name" />}
        </Field>
        <Field label="Description" optional help={<>Up to <b>512</b> characters.</>}>{(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="dest-description" />}</Field>
        <Field label="API destination endpoint" error={tried ? (endpointProblem(f.endpoint) ?? undefined) : undefined} help={<>AWS requires <span className="font-mono">https</span>; doze-aws also accepts <span className="font-mono">http</span> so a server on localhost can be a destination. A <span className="font-mono">*</span> is a path wildcard that each target fills in from its HTTP parameters.</>}>
          {(id) => <Input id={id} value={f.endpoint} onChange={(e) => set({ endpoint: e.target.value })} placeholder="https://status.larder.example/v1/incidents/*" autoComplete="off" spellCheck={false} className="font-mono" data-testid="dest-endpoint" />}
        </Field>
        <div className="grid items-start gap-4 md:grid-cols-2">
        <Field label="HTTP method" help={<>Any method except CONNECT and TRACE. <b>POST</b> and <b>PUT</b> are the common ones.</>}>{(id) => <Combobox id={id} options={METHODS.map((m) => ({ value: m, label: m }))} value={f.method} onChange={(m) => m && set({ method: m })} aria-label="HTTP method" className="w-full max-w-xs" />}</Field>
        <Field label="Invocation rate limit per second" optional error={tried ? (rateProblem(f.rate) ?? undefined) : undefined} help={<>Greater than <b>0</b>. The default is <b>300</b>.</>}>
          {(id) => <Input id={id} value={f.rate} onChange={(e) => set({ rate: e.target.value })} placeholder="300" inputMode="numeric" className="max-w-[12rem]" data-testid="dest-rate" />}
        </Field>
        </div>
        <LocalStatus state="stored" reason={<>doze-aws keeps the limit and does not enforce it.</>} />
      </FormSection>
      <FormSection title="Connection" description="The authorization type and credentials EventBridge uses for the endpoint." testId="section-connection">
        <RadioCards<"existing" | "new"> name="conn" label="Connection" value="existing" onChange={(v) => { if (v === "new") void navigate({ to: "/resources/$service/$second", params: { service: "eventbridge", second: "api-destinations" }, search: { do: "connection" } }); }} options={[{ value: "existing", title: "Use an existing connection", body: "Pick one from this account." }, { value: "new", title: "Create a new connection", body: "Enter its name, destination type and authorization." }]} />
        <Field label="Connection" error={tried && !f.connection ? "Choose the connection." : undefined} testId="field-connection">
          {(id) => <Combobox id={id} options={conns.map((c) => ({ value: c.Name, label: c.Name, hint: c.AuthorizationType }))} value={f.connection} onChange={(connection) => set({ connection })} placeholder={conns.length === 0 ? "There are no connections yet: create one" : "Choose a connection"} search="Search connections…" empty="No connection matches." aria-label="Connection" className="w-full" />}
        </Field>
        {chosen && <Callout tone="warn" testId="connection-state">If the connection is new, wait until it reaches the <b className="font-semibold text-bright">Authorized</b> state before you create the destination. <span className="font-mono font-semibold text-bright">{chosen.Name}</span> is <b className="font-semibold text-bright">{chosen.ConnectionState === "AUTHORIZED" ? "Authorized" : (chosen.ConnectionState ?? "unknown")}</b>.</Callout>}
        <RuleHelp>The endpoint must accept the connection's authorization type.{others.length > 0 ? ` ${list(others)} ${others.length === 1 ? "is" : "are"} the other connection${others.length === 1 ? "" : "s"}.` : ""}</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

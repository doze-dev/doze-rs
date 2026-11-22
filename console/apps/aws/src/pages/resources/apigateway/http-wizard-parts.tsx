// What the HTTP API wizard's steps are made of (apigateway__http-create, http-create-routes): one backend's integration (the card Add integration
// adds), and the route that sends requests to it.
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { XIcon } from "lucide-react";
import { Field, FunctionPicker } from "../../../kit";
import { useFunctionSentence } from "./api";
import { type Backend, type IntegrationForm, ROUTE_METHODS, pathOf } from "./http-model";

/** `integration-kind`, then `integration-kind-2` for the second: the first keeps the names it has always had. */
const tid = (base: string, i: number): string => (i === 0 ? base : `${base}-${i + 1}`);

/** One integration of the wizard: its type and what it calls. `problem` is shown on the field at fault once a step was tried. */
export function IntegrationFields({ i, count, form, region, set, remove, tried, problem }: { i: number; count: number; form: IntegrationForm; region: string; set: (patch: Partial<IntegrationForm>) => void; remove: () => void; tried: boolean; problem: string | null }) {
  const lambda = form.kind === "lambda";
  const sentence = useFunctionSentence(lambda ? form.fn : null);
  return (
    <div className="grid gap-5 rounded-[12px] border p-5" data-testid={tid("integration", i)}>
      {count > 1 && (
        <div className="flex items-center justify-between gap-3">
          <h3 className="text-[15px] font-semibold text-bright">Integration {i + 1}</h3>
          <Button type="button" variant="ghost" size="sm" onClick={remove} data-testid={tid("remove-integration", i)}><XIcon /> Remove</Button>
        </div>
      )}
      <Field label="Integration type">
        {(id) => (
          <Select value={form.kind} onValueChange={(kind) => set({ kind: kind as "lambda" | "http" })}>
            <SelectTrigger id={id} className="w-full bg-card" data-testid={tid("integration-kind", i)}><SelectValue /></SelectTrigger>
            <SelectContent><SelectItem value="lambda">Lambda</SelectItem><SelectItem value="http">HTTP URI</SelectItem></SelectContent>
          </Select>
        )}
      </Field>
      {lambda ? (
        <>
          <Field label="Lambda function" error={tried && !form.fn ? "Choose the Lambda function." : undefined} hint={<>API Gateway needs permission to invoke the function, through a resource-based policy or an IAM role.{sentence && <> <Emphasis text={sentence} names={[form.fn ?? ""]} /></>}</>}>
            {(id) => (
              <div className="grid gap-3 sm:grid-cols-[11rem_minmax(0,1fr)]">
                {/* doze-aws serves one Region, so the Region of the function is the one choice. */}
                <Select value={region}><SelectTrigger className="w-full bg-card font-mono" aria-label="Region of the function" data-testid={tid("function-region", i)}><SelectValue /></SelectTrigger><SelectContent><SelectItem value={region} className="font-mono">{region}</SelectItem></SelectContent></Select>
                <FunctionPicker id={id} by="name" value={form.fn} onChange={(fn) => set({ fn })} />
              </div>
            )}
          </Field>
          <Field label="Payload format version" hint="The shape of the event the function receives and of the response it must return. The console uses the latest version, 2.0, when you do not choose; 1.0 is the other option.">
            {(id) => (
              <Select value={form.version} onValueChange={(version) => set({ version: version as "2.0" | "1.0" })}>
                <SelectTrigger id={id} className="w-64 bg-card" data-testid={tid("payload-version", i)}><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="2.0">2.0</SelectItem><SelectItem value="1.0">1.0</SelectItem></SelectContent>
              </Select>
            )}
          </Field>
        </>
      ) : (
        <>
          <Field label="Endpoint URL" error={tried ? (problem ?? undefined) : undefined} help={<>A <span className="font-mono">{"{param}"}</span> placeholder is filled from the request path.</>}>{(id) => <Input id={id} value={form.url} onChange={(e) => set({ url: e.target.value })} placeholder="https://orders.larder.example.com/{proxy}" className="font-mono" autoComplete="off" spellCheck={false} data-testid={tid("integration-url", i)} />}</Field>
          <Field label="Method">{(id) => <Select value={form.method} onValueChange={(method) => set({ method })}><SelectTrigger id={id} className="w-full bg-card font-mono" data-testid={tid("integration-method", i)}><SelectValue /></SelectTrigger><SelectContent>{ROUTE_METHODS.map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent></Select>}</Field>
        </>
      )}
    </div>
  );
}

/** The route of one backend: its method and path, and the integration it sends to. */
export function RouteFields({ i, b, set, tried, problem }: { i: number; b: Backend; set: (patch: Partial<Backend>) => void; tried: boolean; problem: string | null }) {
  const lambda = b.integration.kind === "lambda";
  return (
    <div className="grid items-start gap-4 px-1 sm:grid-cols-[10rem_minmax(0,1fr)_minmax(0,16rem)]" data-testid={tid("route", i)}>
      <Select value={b.method} onValueChange={(method) => set({ method })}><SelectTrigger className="w-full bg-card font-mono" aria-label="Method" data-testid={tid("route-method", i)}><SelectValue /></SelectTrigger><SelectContent>{ROUTE_METHODS.map((m) => <SelectItem key={m} value={m} className="font-mono">{m}</SelectItem>)}</SelectContent></Select>
      <Field label={<span className="sr-only">Path</span>} error={tried ? (problem ?? undefined) : undefined}>{(id) => <Input id={id} value={pathOf(b)} onChange={(e) => set({ path: e.target.value })} className="font-mono" autoComplete="off" spellCheck={false} data-testid={tid("route-path", i)} />}</Field>
      <span className="flex h-9 items-center font-mono text-[13px] text-bright">{lambda ? <>{b.integration.fn ?? "…"} <span className="ml-1.5 text-muted-foreground">Lambda</span></> : "HTTP endpoint"}</span>
    </div>
  );
}

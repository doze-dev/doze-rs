// An HTTP API's CORS (apigateway__http-cors): the origins, headers, methods and exposed headers a browser may use, the preflight's max age and
// whether credentials are allowed, saved with UpdateApi. The configuration is a part of the API itself, not of a route or a stage.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Switch } from "@doze/ui/components/ui/switch";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { CallPanel, Field, RuleHelp } from "../../../kit";
import { HTTP, useRoutes, useWrite } from "./api";
import { type CorsForm, corsInput, corsOf, corsProblem } from "./http-model";
import { ListEditor } from "./list-editor";
import { ApiChips } from "./parts";
import { httpSections } from "./sections";
import type { HttpApi } from "./types";
import { list } from "./words";

export function HttpCors({ id, api }: { id: string; api: HttpApi }) {
  const navigate = useNavigate();
  const routes = useRoutes(id).data?.items ?? [];
  const [f, setF] = useState<CorsForm>(() => corsOf(api));
  const save = useWrite<HttpApi>(HTTP, "UpdateApi");
  const [tried, setTried] = useState(false);
  const problem = corsProblem(f);
  const input = useMemo(() => corsInput(id, f), [id, f]);
  const failure = save.error instanceof ApiFailure ? { code: save.error.code, message: save.error.message } : null;
  const set = (patch: Partial<CorsForm>) => { setF((x) => ({ ...x, ...patch })); save.reset(); };
  const keys = routes.map((r) => r.routeKey);
  const reset = () => { setF(corsOf(api)); save.reset(); setTried(false); };
  return (
    <>
      <ApiChips kind="apis" id={id} name={api.name} sections={httpSections(api.protocolType === "WEBSOCKET")} current="cors" />
      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_22.5rem]">
        <form className="grid min-w-0 gap-6" noValidate onSubmit={(e) => { e.preventDefault(); setTried(true); if (!problem) save.mutate(input as never, { onSuccess: () => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "apis", name: id }, search: { tab: "cors", done: "cors" } }) }); }} data-testid="cors-form">
          <section className="grid gap-6 rounded-[14px] border bg-card p-6 lg:p-7" aria-label="CORS configuration">
            <h2 className="text-[19px] leading-7 font-semibold text-bright">CORS configuration</h2>
            <Field label="Access-Control-Allow-Origin" help={<>An origin, <span className="font-mono">*</span> for any origin, or a prefix such as <span className="font-mono">https://*</span>.</>}>
              {() => <ListEditor values={f.origins} onChange={(origins) => set({ origins })} placeholder="https://admin.larder.example.com" label="Allowed origin" testId="cors-origins" />}
            </Field>
            <Field label="Access-Control-Allow-Headers">{() => <ListEditor values={f.headers} onChange={(headers) => set({ headers })} placeholder="authorization" label="Allowed header" testId="cors-headers" />}</Field>
            <Field label="Access-Control-Allow-Methods" help={<>Methods the browser may use. {api.name} has {keys.length === 0 ? "no route yet" : <span className="font-mono">{list(keys)}</span>}.</>}>
              {() => <ListEditor values={f.methods} onChange={(methods) => set({ methods })} placeholder="GET" label="Allowed method" testId="cors-methods" />}
            </Field>
            <Field label="Access-Control-Expose-Headers">{() => <ListEditor values={f.expose} onChange={(expose) => set({ expose })} placeholder="x-api-id" label="Exposed header" testId="cors-expose" />}</Field>
            <Field label="Access-Control-Max-Age" error={tried && problem?.startsWith("Max age") ? problem : undefined} help="How long the browser may cache a preflight answer. Range -1 to 86,400 seconds.">
              {(fid) => <div className="flex items-center gap-3"><Input id={fid} value={f.maxAge} onChange={(e) => set({ maxAge: e.target.value })} inputMode="numeric" className="w-32 font-mono" data-testid="cors-max-age" /><span className="text-[13.5px] text-muted-foreground">seconds</span></div>}
            </Field>
            <div className="grid gap-1.5">
              <div className="flex items-center gap-3"><Switch id="cors-credentials" checked={f.credentials} onCheckedChange={(credentials) => set({ credentials })} data-testid="cors-credentials" /><Label htmlFor="cors-credentials">Access-Control-Allow-Credentials</Label></div>
              <p className="text-[13px] text-muted-foreground" data-testid="cors-credentials-note">{f.credentials ? "On. The credentials header is sent, and a browser then needs each origin named." : "Off. The credentials header is not sent."}</p>
            </div>
            {tried && problem && !problem.startsWith("Max age") && <p className="text-[13px] text-bad" role="alert" data-testid="cors-problem">{problem}</p>}
            {failure && <Alert tone="bad" title="The CORS configuration wasn't saved." testId="cors-refused">UpdateApi answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert>}
            <div className="flex flex-wrap items-center justify-end gap-3 border-t pt-5">
              <Button type="button" variant="outline" className="bg-card" onClick={reset} data-testid="cors-cancel">Cancel</Button>
              <Button type="submit" disabled={save.isPending} data-testid="cors-save">{save.isPending ? "Saving…" : "Save"}</Button>
            </div>
          </section>
        </form>
        <div className="grid gap-6 xl:sticky xl:top-6"><CallPanel request={{ service: "apigatewayv2", operation: "UpdateApi", input }} showInput note="The CORS block is part of the API itself, not of a route or a stage." />
          <section className="grid gap-3 rounded-[14px] border bg-card p-5" aria-label="When CORS is on">
            <h2 className="text-[17px] leading-6 font-semibold text-bright">When CORS is on</h2>
            <RuleHelp>API Gateway replies to preflight <span className="font-mono">OPTIONS</span> requests on its own, even with no OPTIONS route, and adds the CORS headers to integration responses.</RuleHelp>
            <RuleHelp>A request must carry an <span className="font-mono">origin</span> header to get CORS headers back; a preflight also needs Access-Control-Request-Method. doze-aws answers a preflight with 204.</RuleHelp>
          </section>
        </div>
      </div>
    </>
  );
}

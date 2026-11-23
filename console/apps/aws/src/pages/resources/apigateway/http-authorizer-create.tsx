// Create authorizer for an HTTP API (apigateway__http-authorizer-create): a Lambda function that decides (a request authorizer), or a JWT
// authorizer that checks a token against its issuer. Attaching it to a route is the Authorization page's second step.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, FunctionPicker, RadioCards, RuleHelp } from "../../../kit";
import { HTTP, useHttpAuthorizers, useWrite } from "./api";
import { type HttpAuthorizerForm, emptyHttp, httpAuthorizerInput, httpAuthorizerProblem } from "./authorizer-model";
import { ListEditor } from "./list-editor";
import { useWhere } from "./parts";
import type { HttpAuthorizer } from "./types";
import { list } from "./words";
import { sdkShown } from "./calls";

export function HttpAuthorizerCreate({ api, apiName, crumbs, onClose }: { api: string; apiName: string; crumbs: { label: string; to: string }[]; onClose: (created?: string) => void }) {
  const where = useWhere();
  const [f, setF] = useState<HttpAuthorizerForm>(emptyHttp);
  const [tried, setTried] = useState(false);
  const make = useWrite<HttpAuthorizer>(HTTP, "CreateAuthorizer");
  const others = useHttpAuthorizers(api).data?.items ?? [];
  const set = (patch: Partial<HttpAuthorizerForm>) => { setF((x) => ({ ...x, ...patch })); make.reset(); };
  const problem = httpAuthorizerProblem(f);
  const input = useMemo(() => httpAuthorizerInput(f, { api, region: where.region, account: where.account }), [f, api, where.region, where.account]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  const lambda = f.kind === "lambda";
  return (
    <FormPage
      shown={sdkShown}
      title="Create authorizer"
      sub={lambda ? "Have API Gateway ask a Lambda function whether a request may pass. You attach the authorizer to a route afterwards." : "Have API Gateway check a token against its issuer before a request may pass. You attach the authorizer to a route afterwards."}
      titleExtra={<span className="font-mono text-[13px] text-muted-foreground">{apiName}</span>}
      crumbs={[...crumbs, { label: "Authorization", to: `/resources/apigateway/apis/${api}?tab=authorization` }, { label: "Manage authorizers", to: `/resources/apigateway/apis/${api}?tab=authorization&mode=manage` }, { label: "Create authorizer", to: `/resources/apigateway/apis/${api}?do=create-authorizer` }]}
      request={{ service: "apigatewayv2", operation: "CreateAuthorizer", input }}
      showInput
      refusal={failure}
      callNote={<>Attaching it to a route is a second step, on the Authorization page: pick the route, pick the authorizer, attach. The route&apos;s authorization type becomes <span className="font-mono">{lambda ? "CUSTOM" : "JWT"}</span>.</>}
      banner={failure ? <Alert tone="bad" title="The authorizer wasn't created." testId="create-refused">CreateAuthorizer answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => onClose()}
      onSubmit={() => { setTried(true); if (problem) return; make.mutate(input as never, { onSuccess: () => onClose(f.name) }); }}
      testId="http-authorizer-create"
    >
      <FormSection title="Authorizer settings" testId="section-settings">
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Authorizer type</span>
          <RadioCards<"jwt" | "lambda"> name="kind" label="Authorizer type" value={f.kind} onChange={(kind) => set({ kind, sources: f.sources.length > 0 ? f.sources : ["$request.header.Authorization"] })} options={[
            { value: "jwt", title: "JWT", body: "A token is checked against its issuer's keys: signature (RSA), issuer, expiry and audience." },
            { value: "lambda", title: "Lambda", body: "A Lambda function decides." },
          ]} />
        </div>
        <Field label="Name" error={tried && f.name.trim() === "" ? "Enter an authorizer name." : undefined} hint={others.length > 0 ? <>{apiName} has {list(others.map((a) => a.name))} already.</> : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="orders-request-auth" autoComplete="off" spellCheck={false} className="font-mono" data-testid="authorizer-name-input" />}
        </Field>
        {lambda ? (
          <>
            <Field label="Lambda function" error={tried && !f.fn ? "Choose the Lambda function." : undefined} hint={<>API Gateway must be allowed to invoke the function, through its resource policy or an IAM role.</>}>
              {(id) => <FunctionPicker id={id} by="name" value={f.fn} onChange={(fn) => set({ fn })} />}
            </Field>
            <div className="grid gap-2"><span className="text-[14px] leading-none font-medium text-bright">Payload format version</span><Select value={f.version} onValueChange={(version) => set({ version: version as "2.0" | "1.0", simple: version === "2.0" ? f.simple : false })}><SelectTrigger className="w-64 bg-card" aria-label="Payload format version" data-testid="payload-version"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="2.0">2.0</SelectItem><SelectItem value="1.0">1.0</SelectItem></SelectContent></Select><p className="text-[13px] leading-5 text-muted-foreground">The shape of the event the function receives. The console uses the latest, 2.0, when you do not choose; use 1.0 for compatibility with REST APIs.</p></div>
            {f.version === "2.0" && (
              <div className="flex items-start gap-3"><Switch id="simple" checked={f.simple} onCheckedChange={(simple) => set({ simple })} className="mt-0.5" data-testid="authorizer-simple" /><div className="grid gap-0.5"><Label htmlFor="simple">Enable simple responses</Label><p className="text-[13px] text-muted-foreground">With 2.0, the function may answer <span className="font-mono">{"{\"isAuthorized\": true}"}</span> instead of an IAM policy. Turn it off for per-route permissions in a policy.</p></div></div>
            )}
          </>
        ) : (
          <>
            <Field label="Issuer URL" error={tried && !/^https?:\/\//.test(f.issuer.trim()) ? "The issuer is a URL, such as https://idp.example.com/." : undefined} help="Where the token's keys are found: doze-aws fetches the issuer's JWKS and keeps it two hours.">{(id) => <Input id={id} value={f.issuer} onChange={(e) => set({ issuer: e.target.value })} placeholder="https://idp.example.com/" className="font-mono" autoComplete="off" spellCheck={false} data-testid="authorizer-issuer" />}</Field>
            <Field label="Audience" error={tried && f.audience.trim() === "" ? "Enter at least one audience." : undefined} help="The token's aud (or client_id) must be one of these. Separate several with commas.">{(id) => <Input id={id} value={f.audience} onChange={(e) => set({ audience: e.target.value })} placeholder="orders-api" className="font-mono" autoComplete="off" data-testid="authorizer-audience" />}</Field>
          </>
        )}
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Identity sources</span>
          <ListEditor values={f.sources} onChange={(sources) => set({ sources })} placeholder="$request.header.Authorization" label="Identity source" testId="authorizer-sources" />
          <RuleHelp>{lambda ? <>Header, query string, context or stage variable values the request must carry. If one is missing, API Gateway does not invoke the function and the caller gets 401. Header names are case-insensitive, query string names case-sensitive.</> : <>Where the token is: <span className="font-mono">$request.header.Authorization</span>. A request without it gets 401.</>}</RuleHelp>
        </div>
        {lambda && (
          <Field label="Authorizer result TTL" help={<>The identity sources are the cache key. Range <b>0 to 3,600</b> seconds (<span className="font-mono">authorizerResultTtlInSeconds</span>); 0 turns caching off. Caching needs at least one identity source. doze-aws keeps the cache in memory, so a restart clears it.</>}>
            {(id) => <div className="flex items-center gap-3"><Input id={id} value={f.ttl} onChange={(e) => set({ ttl: e.target.value })} inputMode="numeric" className="w-32 font-mono" data-testid="authorizer-ttl" /><span className="text-[13.5px] text-muted-foreground">seconds</span></div>}
          </Field>
        )}
      </FormSection>
    </FormPage>
  );
}

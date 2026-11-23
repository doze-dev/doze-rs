// Create authorizer for a REST API (apigateway__rest-authorizer-create): a Lambda function that returns a policy (a token or a request
// authorizer), or a Cognito user pool whose token claims are checked. Attaching it to a method is the method request's.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Switch } from "@doze/ui/components/ui/switch";
import { Label } from "@doze/ui/components/ui/label";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, FunctionPicker, RadioCards, RuleHelp } from "../../../kit";
import { type RestAuthorizerForm, emptyRest, restAuthorizerInput, restAuthorizerProblem } from "./authorizer-model";
import { REST, useAuthorizers, useWrite } from "./api";
import { useWhere } from "./parts";
import type { Authorizer } from "./types";
import { list } from "./words";
import { sdkShown } from "./calls";

export function AuthorizerCreate({ api, apiName, crumbs, onClose }: { api: string; apiName: string; crumbs: { label: string; to: string }[]; onClose: (created?: string) => void }) {
  const where = useWhere();
  const [f, setF] = useState<RestAuthorizerForm>(emptyRest);
  const [tried, setTried] = useState(false);
  const make = useWrite<Authorizer>(REST, "CreateAuthorizer");
  const others = (useAuthorizers(api).data?.item ?? []).filter((a) => a.name !== f.name);
  const set = (patch: Partial<RestAuthorizerForm>) => { setF((x) => ({ ...x, ...patch })); make.reset(); };
  const problem = restAuthorizerProblem(f);
  const input = useMemo(() => restAuthorizerInput(f, { api, region: where.region, account: where.account }), [f, api, where.region, where.account]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  const lambda = f.kind === "lambda";
  return (
    <FormPage
      shown={sdkShown}
      title="Create authorizer"
      sub={lambda ? "Point API Gateway at a Lambda function that decides who may call. You attach the authorizer to a method afterwards." : "Check the claims of a Cognito user pool token. You attach the authorizer to a method afterwards."}
      titleExtra={<span className="font-mono text-[13px] text-muted-foreground">{apiName}</span>}
      crumbs={[...crumbs, { label: "Authorizers", to: `/resources/apigateway/restapis/${api}?tab=authorizers` }, { label: "Create authorizer", to: `/resources/apigateway/restapis/${api}?do=create-authorizer` }]}
      request={{ service: "apigateway", operation: "CreateAuthorizer", input }}
      showInput
      refusal={failure}
      callNote={<>The type is <span className="font-mono">{lambda ? f.payload : "COGNITO_USER_POOLS"}</span> here{lambda ? <>; choosing {f.payload === "TOKEN" ? "Request" : "Token"} makes it <span className="font-mono">{f.payload === "TOKEN" ? "REQUEST" : "TOKEN"}</span></> : ""}.</>}
      banner={failure ? <Alert tone="bad" title="The authorizer wasn't created." testId="create-refused">CreateAuthorizer answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create authorizer"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => onClose()}
      onSubmit={() => { setTried(true); if (problem) return; make.mutate(input as never, { onSuccess: () => onClose(f.name) }); }}
      testId="authorizer-create"
    >
      <FormSection title="Authorizer details" testId="section-details">
        <Field label="Authorizer name" error={tried && f.name.trim() === "" ? "Enter an authorizer name." : undefined} hint={others.length > 0 ? <>{apiName} has {list(others.map((a) => a.name))} already.</> : undefined}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="partner-token-auth" autoComplete="off" spellCheck={false} className="font-mono" data-testid="authorizer-name-input" />}
        </Field>
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">Authorizer type</span>
          <RadioCards<"lambda" | "cognito"> name="kind" label="Authorizer type" value={f.kind} onChange={(kind) => set({ kind, source: "Authorization" })} options={[
            { value: "lambda", title: "Lambda", body: "A Lambda function returns an IAM policy." },
            { value: "cognito", title: "Cognito", body: "A user pool's token is checked: its claims, not its signature, as no user pool is served here." },
          ]} />
        </div>
        {lambda ? (
          <>
            <Field label="Lambda function" help={<>The function the authorizer calls. doze-aws invokes it directly.</>}>
              {(id) => <FunctionPicker id={id} by="name" value={f.fn} onChange={(fn) => set({ fn })} />}
            </Field>
            <Field label="Lambda invoke role" optional hint={<>A role name or ARN API Gateway would assume to invoke the function. Leave it blank: doze-aws calls the function directly, does not read the function's resource policy and does not assume a role, so a role entered here is stored, not applied locally.</>}>
              {(id) => <Input id={id} value={f.role ?? ""} onChange={(e) => set({ role: e.target.value || null })} placeholder="Leave blank" className="bg-card font-mono" autoComplete="off" spellCheck={false} data-testid="authorizer-role" />}
            </Field>
            <div className="grid gap-2">
              <span className="text-[14px] leading-none font-medium text-bright">Lambda event payload</span>
              <RadioCards<"TOKEN" | "REQUEST"> name="payload" label="Lambda event payload" value={f.payload} onChange={(payload) => set({ payload })} options={[
                { value: "TOKEN", title: "Token", body: "One header carries the token." },
                { value: "REQUEST", title: "Request", body: "Headers, query strings, stage variables, context." },
              ]} />
            </div>
          </>
        ) : (
          <Field label="User pool ARN" error={tried && f.pools.trim() === "" ? "Enter the ARN of a user pool." : undefined} help="Separate several with commas.">
            {(id) => <Input id={id} value={f.pools} onChange={(e) => set({ pools: e.target.value })} placeholder={`arn:aws:cognito-idp:${where.region}:${where.account}:userpool/${where.region}_abc123`} className="font-mono" data-testid="authorizer-pools" />}
          </Field>
        )}
        <Field label={lambda && f.payload === "REQUEST" ? "Identity sources" : "Token source"} help={lambda && f.payload === "REQUEST" ? "Comma-separated: a header name, or method.request.querystring.x, stageVariables.x, context.x. With caching on, these are the cache key." : "The header that carries the token. Callers must send a header of this name. With caching on, this name is the cache key."}>
          {(id) => <Input id={id} value={f.source} onChange={(e) => set({ source: e.target.value })} className="font-mono" data-testid="authorizer-source" />}
        </Field>
        {(f.kind === "cognito" || f.payload === "TOKEN") && (
          <Field label="Token validation" optional help="A regular expression. API Gateway checks the token against it first and invokes the function only when it matches, which saves invocations.">
            {(id) => <Input id={id} value={f.validation} onChange={(e) => set({ validation: e.target.value })} placeholder="^Bearer [A-Za-z0-9-_.]+$" className="font-mono" data-testid="authorizer-validation" />}
          </Field>
        )}
        <div className="grid gap-3">
          <div className="flex items-center gap-3"><Switch id="cache-on" checked={f.cache} onCheckedChange={(cache) => set({ cache })} data-testid="authorizer-cache" /><Label htmlFor="cache-on">Authorization caching</Label></div>
          {f.cache && (
            <Field label="TTL" help={<>Range <b>0 to 3,600</b> seconds, default 300; 0 turns caching off. With caching on, the function must return a policy that fits every method of the API.{others[0] && <> {others[0].name}, the other authorizer here, caches for {others[0].authorizerResultTtlInSeconds ?? 0} seconds.</>}</>}>
              {(id) => <div className="flex items-center gap-3"><Input id={id} value={f.ttl} onChange={(e) => set({ ttl: e.target.value })} inputMode="numeric" className="w-32 font-mono" data-testid="authorizer-ttl" /><span className="text-[13.5px] text-muted-foreground">seconds</span></div>}
            </Field>
          )}
          <RuleHelp>An authorizer&apos;s result is cached in memory by doze-aws, so a restart clears it.</RuleHelp>
        </div>
      </FormSection>
    </FormPage>
  );
}

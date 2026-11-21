// Create REST API (apigateway__rest-create): the name and description, where the API is reachable from, and the IP address type, with
// the call beside them. It lands on the new API's Resources.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, LocalStatus, RadioCards, RuleHelp, useServiceCrumbs } from "../../../kit";
import { REST, useHttpApis, useRestApis, useWrite } from "./api";
import { useWhere } from "./parts";
import type { RestApi } from "./types";
import { apiNameProblem, haveApis } from "./words";

export type RestForm = { name: string; description: string; ip: "ipv4" | "dualstack" };

/** The CreateRestApi input: Regional (the only kind of endpoint with something to serve it here), the IP address type. */
export const createRestInput = (f: RestForm) => ({ name: f.name || "<name>", ...(f.description ? { description: f.description } : {}), endpointConfiguration: { types: ["REGIONAL"], ipAddressType: f.ip } });

export function RestCreate() {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const { region } = useWhere();
  const [f, setF] = useState<RestForm>({ name: "", description: "", ip: "ipv4" });
  const [tried, setTried] = useState(false);
  const make = useWrite<RestApi>(REST, "CreateRestApi");
  const rest = useRestApis().data?.item ?? [];
  const http = useHttpApis().data?.items ?? [];
  const set = (patch: Partial<RestForm>) => { setF((x) => ({ ...x, ...patch })); make.reset(); };
  const problem = apiNameProblem(f.name);
  const input = useMemo(() => createRestInput(f), [f]);
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : make.error ? { code: "Error", message: String((make.error as Error).message) } : null;
  const have = haveApis(rest.map((a) => a.name), http.map((a) => a.name));
  const submit = () => {
    setTried(true);
    if (problem) return;
    make.mutate(input as never, { onSuccess: (made) => void navigate({ to: "/resources/$service/$kind/$name", params: { service: "apigateway", kind: "restapis", name: made.id }, search: { done: "created" } }) });
  };
  return (
    <FormPage
      title="Create REST API"
      sub="Name the API and choose where it is reachable from. The new API starts with a root resource (/) and no methods."
      crumbs={[...crumbs, { label: "APIs", to: "/resources/apigateway/restapis" }, { label: "Create API", to: "/resources/apigateway/restapis?do=create" }]}
      request={{ service: "apigateway", operation: "CreateRestApi", input }}
      showInput
      refusal={failure}
      callNote={<>The answer carries the new API&apos;s ID and its root resource ID. The next page is the API&apos;s <b className="font-semibold text-foreground">Resources</b>.</>}
      banner={failure ? <Alert tone="bad" title="The API wasn't created." testId="create-refused">CreateRestApi answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</Alert> : undefined}
      submitLabel="Create API"
      pending={make.isPending}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "restapis" }, search: { do: "create" } })}
      onSubmit={submit}
      testId="rest-create"
    >
      <FormSection title="API details" testId="section-details">
        <Field label="API name" error={tried ? (problem ?? undefined) : undefined} hint={have !== "" ? <>This Region already has {have}.</> : undefined} help={<><b>Required.</b> Names need not be unique: an API is found by its ID.</>}>
          {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="larder-orders" autoComplete="off" spellCheck={false} className="font-mono" data-testid="api-name" />}
        </Field>
        <Field label="Description" optional>{(id) => <Input id={id} value={f.description} onChange={(e) => set({ description: e.target.value })} autoComplete="off" data-testid="api-description" />}</Field>
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">API endpoint type</span>
          <RadioCards<"regional" | "edge" | "private"> name="endpoint" label="API endpoint type" value="regional" onChange={() => undefined} className="sm:grid-cols-3" options={[
            { value: "regional", title: "Regional", body: "For clients in the same Region." },
            { value: "edge", title: <>Edge-optimized <span className="ml-1 text-[12px] font-normal text-muted-foreground">Not available</span></>, body: "Not available in doze-aws: there is no CloudFront locally.", disabled: true },
            { value: "private", title: <>Private <span className="ml-1 text-[12px] font-normal text-muted-foreground">Not available</span></>, body: "Not available in doze-aws: there is no VPC locally.", disabled: true },
          ]} />
          <RuleHelp>The console form starts on Regional. A Regional API is for clients in the same Region; here that is this machine.</RuleHelp>
        </div>
        <div className="grid gap-2">
          <span className="text-[14px] leading-none font-medium text-bright">IP address type</span>
          <RadioCards<"ipv4" | "dualstack"> name="ip" label="IP address type" value={f.ip} onChange={(ip) => set({ ip })} options={[
            { value: "ipv4", title: "IPv4", body: "Only IPv4 addresses can invoke the API." },
            { value: "dualstack", title: "Dualstack", body: "Both IPv4 and IPv6 addresses can invoke the API." },
          ]} />
          <p className="text-[13px] leading-5 text-muted-foreground">The default for a Regional API is IPv4. You can change it later.</p>
          <LocalStatus state="stored" reason={<>doze-aws serves {region}&apos;s APIs on one local address and reads no IP address type (<span className="font-mono">crates/aws-apigateway/src/v1/apis.rs</span> keeps it).</>} />
        </div>
      </FormSection>
    </FormPage>
  );
}

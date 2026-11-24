// Create API (apigateway__api-create-type): the three kinds of API as cards (HTTP, REST, WebSocket) with what each builds on, and how they
// differ in doze-aws. Build opens the wizard (HTTP), the form (REST) or the WebSocket form.
import { useNavigate } from "@tanstack/react-router";
import { Share2Icon } from "lucide-react";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { useServiceCrumbs } from "../../../kit";
import { Heading } from "./parts";

type Card = { id: "http" | "rest" | "websocket"; title: string; body: string; rows: [string, string][] };

const CARDS: Card[] = [
  { id: "http", title: "HTTP API", body: "Routes, integrations and a stage. You add an integration, review the route and the stage the console creates, then create.", rows: [["Backends", "Lambda function, HTTP endpoint"], ["Steps", "Integrations, Configure routes, Stage, Review and create"]] },
  { id: "rest", title: "REST API", body: "Resources and methods you wire by hand, with authorizers, API keys and usage plans.", rows: [["Backends", "Lambda, HTTP, AWS service, Mock"], ["Form", "API name, endpoint type, IP address type"]] },
  { id: "websocket", title: "WebSocket API", body: "Persistent connections for two-way communication.", rows: [["Backends", "Lambda function, HTTP endpoint"], ["Form", "API name, route selection expression"]] },
];

const EXPECT: [string, string, string][] = [
  ["Authorizers", "Lambda (token or request), Cognito user pool (claims checked)", "Lambda (request), JWT"],
  ["API keys and usage plans", "Yes, enforced here", "No"],
  ["Mock integrations", "Yes", "No"],
  ["CORS", "An OPTIONS method with a mock integration", "One configuration on the API"],
  ["Automatic deployments", "No", "Yes"],
];

export function CreateType() {
  const navigate = useNavigate();
  const crumbs = useServiceCrumbs("apigateway");
  const build = (id: Card["id"]) => void navigate({ to: "/resources/$service/$second", params: { service: "apigateway", second: "restapis" }, search: { do: `create-${id}` } });
  return (
    <div data-testid="create-type">
      <PageHeader title="Create API" crumbs={[...crumbs, { label: "APIs", to: "/resources/apigateway/restapis" }, { label: "Create API", to: "" }]} sub="Choose an API type to build. REST APIs support more features than HTTP APIs; HTTP APIs have minimal features so they can be offered at a lower price." />
      <div className="grid gap-6 lg:grid-cols-3">
        {CARDS.map((c) => (
          <section key={c.id} className="flex min-w-0 flex-col rounded-[var(--radius-card)] border bg-card p-7" aria-label={c.title} data-testid={`type-${c.id}`}>
            <span className="mb-5 grid size-9 place-content-center rounded-lg border bg-pick text-primary"><Share2Icon aria-hidden className="size-[18px]" /></span>
            <h2 className="text-[19px] leading-7 font-semibold text-bright">{c.title}</h2>
            <p className="mt-2 text-[14px] leading-[22px] text-muted-foreground">{c.body}</p>
            <dl className="mt-5 grid grid-cols-[6rem_minmax(0,1fr)] gap-y-0 text-[14px] [&>div:last-child>*]:border-b-0">
              {c.rows.map(([l, v]) => (
                <div key={l} className="contents">
                  <dt className="border-b py-3 text-muted-foreground">{l}</dt>
                  <dd className="border-b py-3 text-bright">{v}</dd>
                </div>
              ))}
            </dl>
            <div className="mt-auto pt-6"><Button variant="outline" className="bg-card" onClick={() => build(c.id)} data-testid={`build-${c.id}`}>Build</Button></div>
          </section>
        ))}
      </div>
      <section className="mt-12" aria-labelledby="expect">
        <Heading title="What to expect from each type here" id="expect">API Gateway&apos;s developer guide page &quot;Choose between REST APIs and HTTP APIs&quot; compares them; this is how they differ in doze-aws.</Heading>
        <div className="overflow-x-auto rounded-[var(--radius-card)] border bg-card" data-testid="expect-table">
          <table className="w-full min-w-[40rem] text-left text-[14px]">
            <thead>
              <tr className="bg-muted/60 text-[12px] tracking-[0.08em] text-muted-foreground uppercase">
                <th className="px-5 py-3.5 font-medium">Feature</th><th className="px-5 py-3.5 font-medium">REST API</th><th className="px-5 py-3.5 font-medium">HTTP API</th>
              </tr>
            </thead>
            <tbody>
              {EXPECT.map(([f, r, h]) => (
                <tr key={f} className="border-t">
                  <td className="px-5 py-4 font-medium text-bright">{f}</td><td className="px-5 py-4">{r}</td><td className="px-5 py-4">{h}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

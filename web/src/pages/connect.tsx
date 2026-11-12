import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { CodeIcon, InfoIcon, LockIcon } from "lucide-react";
import { useConnect } from "@/api/queries";
import type { ClientSnippet } from "@/api/types/ClientSnippet";
import type { ConnectResponse } from "@/api/types/ConnectResponse";
import type { Tok } from "@/api/types/Tok";
import { CodeValue } from "@/components/app/code-value";
import { CopyButton } from "@/components/app/copy-button";
import { PageHeader } from "@/components/app/page-header";
import { Spans } from "@/components/app/spans";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { PageError } from "./page-error";
import { CheckAgain, ProbeRules, VerdictDot } from "./probe";

export type CopyAs = "code" | "properties" | "environment";

const TOK: Record<Tok, string> = { plain: "text-foreground", keyword: "text-primary", str: "text-good", comment: "text-muted-foreground italic" };

const SASL = 'security.protocol=SASL_PLAINTEXT\nsasl.mechanism=SCRAM-SHA-256\nsasl.jaas.config=org.apache.kafka.common.security.scram.ScramLoginModule required \\\n  username="<user>" password="<password>";';

function codeText(s: ClientSnippet): string {
  return s.code.map((line) => line.map(([, t]) => t).join("")).join("\n");
}

function Pre({ children, className }: { children: React.ReactNode; className?: string }) {
  return <pre className={cn("overflow-x-auto rounded-lg border bg-well p-4 font-mono text-[12.5px] leading-[1.65]", className)}>{children}</pre>;
}

function Snippet({ s, as, r }: { s: ClientSnippet; as: CopyAs; r: ConnectResponse }) {
  const text = as === "code" ? codeText(s) : as === "properties" ? r.words.properties : r.words.environment;
  return (
    <Card className="min-w-0" data-testid="snippet">
      <CardHeader className="flex flex-row items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2.5">
          <CodeIcon className="size-4 shrink-0 text-cool" aria-hidden />
          <CardTitle className="truncate text-[16px]">{s.name}</CardTitle>
          <span className="rounded-full border bg-well px-2 font-mono text-[11.5px] leading-[22px]">{s.tag}</span>
        </div>
        <CopyButton text={text} what={`${s.name}: copied as ${as}`} />
      </CardHeader>
      <CardContent>
        <Pre>
          {s.code.map((line, i) => (
            <div key={i}>
              {line.map(([tok, t], j) => (
                <span key={j} className={TOK[tok]}>
                  {t}
                </span>
              ))}
              {line.length === 0 && " "}
            </div>
          ))}
        </Pre>
      </CardContent>
    </Card>
  );
}

function Loaded({ r, as, setAs }: { r: ConnectResponse; as: CopyAs; setAs: (a: CopyAs) => void }) {
  const c = r.connect;
  return (
    <div className="flex flex-col gap-10" data-testid="connect">
      <div className="grid gap-6 xl:grid-cols-2 xl:items-start">
        <Card className="min-w-0">
          <CardHeader>
            <CardTitle className="text-[17px]">Address</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <p className="text-[13.5px] text-muted-foreground">Bootstrap from this address. Clients are then told where to reconnect.</p>
            <CodeValue value={c.listen} label="Address" className="h-11" />
            <p className="text-[12.5px] text-muted-foreground">
              The listener, set by <code className="font-mono">--listen</code>. {c.loopback ? "It is on loopback, so only this machine can reach it." : "It is not on loopback, so other machines can reach it."}
            </p>
            <Separator className="my-3" />
            <h3 className="text-[15px] font-semibold text-bright">From another machine or a container</h3>
            <p className="text-[13px] text-muted-foreground">Listen on every interface and advertise an address the other side can use.</p>
            <div className="relative">
              <Pre className="pr-28">{r.words.recipe}</Pre>
              <CopyButton text={r.words.recipe} className="absolute top-2.5 right-2.5" what="Command copied" />
            </div>
          </CardContent>
        </Card>
        <Card className="min-w-0">
          <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-3">
              <CardTitle className="text-[17px]">Advertised listener</CardTitle>
              <VerdictDot verdict={c.probe.verdict} />
            </div>
            <CheckAgain />
          </CardHeader>
          <CardContent className="flex flex-col gap-4">
            <p className="text-[13.5px] text-muted-foreground">
              <Spans spans={r.words.advertised_lede} />
            </p>
            <ProbeRules probe={c.probe} />
          </CardContent>
        </Card>
      </div>
      <section className="flex flex-col gap-4" aria-labelledby="snippets-h">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h2 id="snippets-h" className="text-[18px] font-semibold text-bright">
              Snippets by client
            </h2>
            <p className="text-[13px] text-muted-foreground">The address is filled in. Pick how Copy copies it.</p>
          </div>
          <div className="flex items-center gap-3">
            <span className="text-[13px] text-muted-foreground" id="copy-as">
              Copy as
            </span>
            <ToggleGroup type="single" variant="outline" value={as} onValueChange={(v) => v && setAs(v as CopyAs)} aria-labelledby="copy-as" data-testid="copy-as" className="bg-card">
              <ToggleGroupItem value="code" className="px-3">
                Code
              </ToggleGroupItem>
              <ToggleGroupItem value="properties" className="px-3">
                Properties
              </ToggleGroupItem>
              <ToggleGroupItem value="environment" className="px-3">
                Environment
              </ToggleGroupItem>
            </ToggleGroup>
          </div>
        </div>
        <div className="grid gap-6 xl:grid-cols-2">
          {c.snippets.map((s) => (
            <Snippet key={s.name} s={s} as={as} r={r} />
          ))}
          <Card className="min-w-0">
            <CardHeader>
              <CardTitle className="text-[16px]">Name your app</CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-4 text-[13.5px] text-muted-foreground">
              <p>
                Set <code className="font-mono text-foreground">client.id</code> to the app's own name. The wire shows every request under that name, so <code className="font-mono text-foreground">payments</code> and <code className="font-mono text-foreground">fulfilment</code> appear as themselves, not as a default like <code className="font-mono text-foreground">rdkafka</code>.
              </p>
              <div className="flex gap-3 rounded-lg bg-accent/60 p-4">
                <InfoIcon className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden />
                <p>
                  {c.access.sasl ? (
                    <>
                      <b className="font-semibold text-bright">Credentials are needed.</b> The top bar says {c.access.label}: every client must log in. Add the lines below.{" "}
                    </>
                  ) : (
                    <>
                      <b className="font-semibold text-bright">No credentials needed.</b> The top bar says Open: any client that can reach the address may connect. If SASL is on, add the lines below.{" "}
                    </>
                  )}
                  <Link to="/access" className="font-medium text-primary hover:underline">
                    Access
                  </Link>{" "}
                  lists the mechanisms the broker offers.
                </p>
              </div>
            </CardContent>
          </Card>
        </div>
        <Card>
          <CardHeader className="flex flex-row items-center justify-between gap-2">
            <div className="flex items-center gap-2.5">
              <LockIcon className="size-4 text-warn" aria-hidden />
              <CardTitle className="text-[16px]">When SASL is on</CardTitle>
            </div>
            <CopyButton text={SASL} what="SASL settings copied" />
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <p className="text-[13.5px] text-muted-foreground">
              These are Kafka's own client settings. Use <code className="font-mono text-foreground">SASL_SSL</code> when the broker serves TLS; a PLAIN password crosses the network as it is without it.
            </p>
            <Pre>{SASL}</Pre>
          </CardContent>
        </Card>
      </section>
    </div>
  );
}

export function ConnectPage() {
  const q = useConnect();
  const search = useSearch({ from: "/connect" });
  const navigate = useNavigate({ from: "/connect" });
  const as: CopyAs = search.as ?? "code";
  const setAs = (a: CopyAs) => void navigate({ search: (s) => ({ ...s, as: a === "code" ? undefined : a }), resetScroll: false });
  return (
    <>
      <PageHeader
        crumbs={[{ label: "Home", to: "/" }]}
        title="Connect an app"
        sub="It is an ordinary Kafka broker, so it is an ordinary Kafka client. Only the address is yours to fill in; here it already is."
        actions={q.data ? <CopyButton text={q.data.words.properties} label="Copy as client.properties" what="client.properties copied" size="default" className="bg-card" /> : undefined}
      />
      {q.isError && !q.data ? (
        <PageError error={q.error} onRetry={() => void q.refetch()} />
      ) : !q.data ? (
        <div className="grid gap-6 xl:grid-cols-2">
          <Skeleton className="h-80 rounded-xl" />
          <Skeleton className="h-80 rounded-xl" />
        </div>
      ) : (
        <Loaded r={q.data} as={as} setAs={setAs} />
      )}
    </>
  );
}

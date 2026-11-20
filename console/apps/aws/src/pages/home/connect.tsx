import { Link } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { InfoIcon, RadioIcon, RefreshCwIcon, TerminalIcon } from "lucide-react";
import { useCallback, useMemo } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { Button } from "@doze/ui/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { plural } from "@doze/ui/lib/format";
import { useAbout } from "../../api/queries";
import { useArrivals, useHome } from "../../api/spaces";
import type { About } from "../../api/types/About";
import { ago, who } from "../../people";
import { Card, CardSkeleton, CodeBlock, CopyRow, useNow } from "../card";
import { credsFor, envBlock, snippets } from "./snippets";

function Overview({ a }: { a: About }) {
  const home = useHome();
  const now = useNow();
  const creds = credsFor(a.auth);
  const h = home.data;
  return (
    <div className="grid gap-8">
      <Card testId="connect-address" title="One address, every service" lede="doze-aws tells services apart by the request, so one endpoint serves them all. This doze-aws knows this one address.">
        <CopyRow label="Endpoint" value={a.endpoint} testId="connect-endpoint" />
      </Card>
      <Card testId="connect-env" title="Environment" lede="Paste into the shell your app runs from. With AWS_ENDPOINT_URL set, an SDK that reads it needs no endpoint in its code.">
        <CodeBlock code={envBlock(a.endpoint, a.region, creds)} what="Environment block copied" testId="env-block" />
        <div className="mt-6 grid gap-4 lg:grid-cols-2">
          <Alert tone="info" title={a.auth === "trust" ? "Any key works" : "Signatures are checked"} icon={InfoIcon} testId="connect-keys">
            {a.auth === "trust"
              ? "doze-aws was started with --auth trust: any non-empty access key and secret are taken. "
              : "doze-aws was started with --auth verify: use the keys it was started with (--access-key, --secret-key); this console does not show the secret. "}
            {a.iam === "off" ? "IAM is off: no policy is checked." : a.iam === "soft" ? "IAM is soft: a key IAM issued is checked against its policies and let through either way (Access shows what would be refused)." : "IAM is enforced: a key IAM issued is refused what its policies do not allow."}
          </Alert>
          <Alert tone="info" title="Or let doze-aws print it" icon={TerminalIcon} testId="connect-env-cmd">
            <span className="font-mono">eval &quot;$(doze-aws env)&quot;</span> sets these, and clears what would send calls elsewhere (a per-service AWS_ENDPOINT_URL_*, a leftover AWS_SESSION_TOKEN).
          </Alert>
        </div>
      </Card>
      <Card
        testId="connect-check"
        title="Check the connection"
        lede="The newest call from each client, from the Live record. This page reads it again when a call arrives."
        actions={
          <div className="flex gap-2">
            <Button variant="outline" size="sm" className="bg-card" onClick={() => void home.refetch()} disabled={home.isFetching} data-testid="check-again">
              <RefreshCwIcon /> Check again
            </Button>
            <Button asChild variant="outline" size="sm" className="bg-card">
              <Link to="/live">
                <RadioIcon /> Open Live
              </Link>
            </Button>
          </div>
        }
      >
        {!h ? (
          <CardSkeleton rows={2} />
        ) : h.callers.length === 0 ? (
          <div data-testid="connect-waiting">
            <StatusDot tone="idle">Waiting for the first call</StatusDot>
            <p className="mt-2 text-[13px] text-muted-foreground">
              Run one of the snippets, or <span className="font-mono">aws sqs list-queues --endpoint-url {a.endpoint}</span>. If your app runs and nothing shows here, it is not reading its endpoint setting.
            </p>
          </div>
        ) : (
          <ul className="grid" data-testid="connect-callers">
            {h.callers.map((c) => {
              const w = who(c.principal);
              return (
                <li key={c.principal.key} className="flex items-center gap-4 border-t py-3 first:border-t-0 first:pt-0">
                  <span className="min-w-0 flex-1 truncate" title={w.title}>
                    <StatusDot tone="ok">{w.label}</StatusDot>
                  </span>
                  <span className="w-56 truncate text-[13px] text-muted-foreground">{c.sdks[0] ?? "no client named"}</span>
                  <span className="w-20 text-right text-[13px] text-muted-foreground">{plural(c.calls, "call")}</span>
                  <span className="w-24 text-right text-[13px] text-muted-foreground">{ago(c.last_ms, now)}</span>
                </li>
              );
            })}
          </ul>
        )}
      </Card>
    </div>
  );
}

function Code({ a }: { a: About }) {
  const all = useMemo(() => snippets(a.endpoint, a.region, credsFor(a.auth)), [a]);
  return (
    <div className="grid gap-6">
      <p className="max-w-[46rem] text-[13.5px] text-muted-foreground">The same call, list the queues, from each client. Credentials come from the code below or from your usual profile and environment.</p>
      {all.map((s) => (
        <Card key={s.id} testId={`snippet-${s.id}`} title={s.title} actions={<span className="font-mono text-[12.5px] text-muted-foreground">{s.install}</span>}>
          <CodeBlock code={s.code} what={`${s.title} snippet copied`} />
          <p className="mt-4 text-[13px] text-muted-foreground">
            For S3: <span className="font-mono text-foreground">{s.s3}</span>
          </p>
        </Card>
      ))}
      <Alert tone="info" title="S3 needs path-style addressing here" icon={InfoIcon} testId="connect-s3">
        doze-aws reads a bucket from the address in path style (<span className="font-mono">{a.endpoint}/bucket/key</span>) on any host, and from the host name only when it is <span className="font-mono">bucket.s3.…</span>. An IP and port has no such name, so turn on path style in the S3 client, as each card above shows.
      </Alert>
    </div>
  );
}

/** Connect your app: the address, the environment, the newest call from each client (it updates when a call arrives), and
 * the same call from six clients. Everything is from `/api/v1/about` and the Live record. */
export function ConnectPage() {
  const about = useAbout();
  const qc = useQueryClient();
  const onCalls = useCallback(() => void qc.invalidateQueries({ queryKey: ["home"], exact: true }), [qc]);
  useArrivals(onCalls);
  const a = about.data;
  return (
    <>
      <PageHeader title="Connect your app" crumbs={[{ label: "Home", to: "/" }]} sub="Point an AWS SDK or the AWS CLI at this doze-aws: one address serves every service." />
      {!a ? (
        <CardSkeleton rows={4} />
      ) : (
        <Tabs defaultValue="overview">
          <TabsList className="mb-8" data-testid="connect-tabs">
            <TabsTrigger value="overview">Overview</TabsTrigger>
            <TabsTrigger value="code" data-testid="tab-code">
              Code
            </TabsTrigger>
          </TabsList>
          <TabsContent value="overview">
            <Overview a={a} />
          </TabsContent>
          <TabsContent value="code">
            <Code a={a} />
          </TabsContent>
        </Tabs>
      )}
    </>
  );
}

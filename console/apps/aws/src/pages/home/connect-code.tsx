import { useMemo } from "react";
import { Callout } from "@doze/ui/components/app/callout";
import type { About } from "../../api/types/About";
import { Card, CodeBlock } from "../card";
import { credsFor, snippets } from "./snippets";

/** The board's order: the clients most often reached for first, the rest after. */
const ORDER = ["boto3", "rust", "js", "cli", "go", "java"];

const link = "rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none";

/** Code: the same call, list the queues, from each client, with what S3 needs under it. */
export function Code({ a, goTo }: { a: About; goTo: (tab: "overview" | "services") => void }) {
  const all = useMemo(() => snippets(a.endpoint, a.region, credsFor(a.auth)).sort((x, y) => ORDER.indexOf(x.id) - ORDER.indexOf(y.id)), [a]);
  return (
    <div className="grid gap-6">
      <p className="max-w-[46rem] text-[13.5px] text-muted-foreground" data-testid="code-lede">
        Credentials come from the environment block on{" "}
        <button type="button" className={link} onClick={() => goTo("overview")} data-testid="code-to-overview">
          Overview
        </button>
        , or from your usual profile: any non-empty values work. With AWS_ENDPOINT_URL set, an SDK that reads it needs no endpoint line at all.
      </p>
      {all.map((s) => (
        <Card key={s.id} testId={`snippet-${s.id}`} title={s.title} actions={<span className="rounded-md border bg-muted px-2 py-0.5 font-mono text-[12px] text-muted-foreground">{s.install}</span>}>
          <CodeBlock code={s.code} what={`${s.title} snippet copied`} />
          <p className="mt-4 text-[13px] text-muted-foreground">
            For S3: <span className="font-mono text-foreground">{s.s3}</span>
          </p>
        </Card>
      ))}
      <Callout tone="note" lead="Using S3 or Step Functions sync calls on an IP and port?" testId="connect-s3">
        Both need one extra client setting there, because the IP form has no hostnames to put a bucket or a host prefix under. See{" "}
        <button type="button" className={link} onClick={() => goTo("services")} data-testid="code-to-services">
          By service
        </button>
        .
      </Callout>
    </div>
  );
}

import { Callout } from "@doze/ui/components/app/callout";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import type { About } from "../../api/types/About";
import { CodeBlock, Section } from "../card";
import { addressLabel, hostOf } from "./address";
import { CheckCard } from "./connect-check";
import { credsFor, envBlock } from "./snippets";
import { urlShapes } from "./url-shapes";

const mono = "font-mono text-[12.5px] text-foreground";
const link = "rounded-sm text-link hover:underline focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:outline-none";
const panel = "min-w-0 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7";
const h3 = "text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright";

/** The two client settings an address with no hostnames needs: what AWS puts in a host name, the client puts elsewhere. */
function Settings() {
  const lines = (rows: [string, string][]) => (
    <div className="mt-4 grid gap-1.5 rounded-xl border bg-well px-5 py-4">
      {rows.map(([what, code]) => (
        <div key={what} className="flex items-baseline gap-5 font-mono text-[12.5px]">
          <span className="w-12 shrink-0 text-muted-foreground">{what}</span>
          <code className="text-foreground">{code}</code>
        </div>
      ))}
    </div>
  );
  return (
    <Section title="Two settings you will need" testId="connect-settings" lede="Both exist because there is no hostname to carry what AWS puts in one.">
      <div className="grid gap-6 md:grid-cols-2">
        <div className={panel} data-testid="setting-s3">
          <h3 className={h3}>S3: path style</h3>
          <p className="mt-1.5 text-[13.5px] text-muted-foreground">Buckets cannot be hostnames on an IP, so the client must put them in the path.</p>
          {lines([["boto3", 'Config(s3={"addressing_style": "path"})'], ["JS v3", "forcePathStyle: true"]])}
        </div>
        <div className={panel} data-testid="setting-sfn">
          <h3 className={h3}>Step Functions: no host prefix</h3>
          <p className="mt-1.5 text-[13.5px] text-muted-foreground">Sync calls send to a sync- host, which does not exist on an IP.</p>
          {lines([["boto3", "Config(inject_host_prefix=False)"], ["JS v3", "disableHostPrefix: true"]])}
        </div>
      </div>
    </Section>
  );
}

function Shapes({ a }: { a: About }) {
  return (
    <Section title="The URL shapes you will see" testId="connect-shapes" lede="The instance mints every URL from the address a request arrived on, so these are what comes back.">
      <div className="overflow-hidden rounded-[var(--radius-card)] border bg-card">
        <table className="w-full text-[13.5px]">
          <thead>
            <tr className="bg-muted text-left text-[11.5px] tracking-wide text-muted-foreground uppercase">
              <th className="w-72 px-4 py-3 font-medium">What</th>
              <th className="px-4 py-3 font-medium">URL</th>
            </tr>
          </thead>
          <tbody>
            {urlShapes(a.endpoint, a.account, a.region).map((s) => (
              <tr key={s.what} className="border-t" data-testid="url-shape">
                <td className="px-4 py-3.5 align-top font-medium text-bright">{s.what}</td>
                <td className="px-4 py-3.5 align-top">
                  {s.urls.map((u) => (
                    <div key={u.url} className="break-all">
                      <code className={mono}>{u.url}</code>
                      {u.note && <span className="ml-2 text-[12.5px] text-muted-foreground">{u.note}</span>}
                    </div>
                  ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  );
}

/** Overview: the one address, the environment, the two settings an IP needs, the URLs that come back, and the check. */
export function Overview({ a, goTo }: { a: About; goTo: (tab: "code" | "services") => void }) {
  const creds = credsFor(a.auth);
  return (
    <div className="grid grid-cols-1">
      <Callout tone="note" lead="One address, every service." testId="connect-one" className="mb-10">
        Names such as <span className="font-mono">aws.larder.doze</span> exist only when the doze app hosts doze-aws as a module. Run on its own, an instance is reached by IP and port, and every service answers there.
      </Callout>
      <Section title="Address" testId="connect-address">
        <div className="grid gap-6 md:grid-cols-2">
          <div className={panel} data-testid="connect-ip">
            <div className="flex items-center justify-between gap-4">
              <h3 className={h3}>{addressLabel(a.endpoint)}</h3>
              <StatusDot tone="ok">Listening</StatusDot>
            </div>
            <div className="mt-5 flex items-center gap-4">
              <div className="min-w-0 flex-1 truncate font-mono text-[18px] text-bright" title={a.endpoint} data-testid="connect-endpoint">
                {a.endpoint}
              </div>
              <CopyButton text={a.endpoint} what="Endpoint copied" className="bg-card" />
            </div>
            <p className="mt-4 text-[13.5px] text-muted-foreground">
              Needs no DNS. Every service arrives on this one address and the instance tells them apart by the request. URLs it hands back use path form, such as <span className="font-mono text-foreground">{hostOf(a.endpoint)}/{a.account}/pick-lists</span>.
            </p>
          </div>
          <div className={panel} data-testid="connect-doze">
            <div className="flex items-center justify-between gap-4">
              <h3 className={h3}>Under the doze app</h3>
              <span className="text-[13px] font-semibold text-link">macOS and Linux</span>
            </div>
            <p className="mt-5 text-[13.5px] text-muted-foreground" data-testid="connect-doze-note">
              When the doze app hosts doze-aws as a module, the same instance also answers on a <span className="font-mono text-foreground">.doze</span> name, and every URL it hands back follows the name you asked with.
            </p>
          </div>
        </div>
      </Section>
      <Section title="Environment" testId="connect-env" lede="Paste into the shell your app runs from.">
        <CodeBlock code={envBlock(a.endpoint, a.region, creds)} what="Environment block copied" testId="env-block" copyLabel="Copy block" />
        {/* The board's two callouts, side by side. */}
        <div className="mt-5 grid gap-5 md:grid-cols-2">
          <Callout tone="note" testId="connect-keys" lead={a.auth === "trust" ? "Credentials are not checked." : "Signatures are checked."}>
            {a.auth === "trust" ? "Any non-empty key and secret work. " : "Use the keys doze-aws was started with (--access-key, --secret-key); this console does not show the secret. "}
            {a.iam === "off" ? "IAM is off: no policy is checked." : a.iam === "soft" ? "IAM runs in soft mode: it evaluates and records every request and never blocks one." : "IAM is enforced: a key IAM issued is refused what its policies do not allow."}
          </Callout>
          <Callout tone="note" testId="connect-env-cmd" lead="Or let the instance print it.">
            <span className="font-mono">eval &quot;$(doze-aws env)&quot;</span> sets these, and clears what would send calls elsewhere (a per-service AWS_ENDPOINT_URL_*, a leftover AWS_SESSION_TOKEN). See the variables{" "}
            <button type="button" className={link} onClick={() => goTo("services")} data-testid="goto-services">
              by service
            </button>
            , or the same call{" "}
            <button type="button" className={link} onClick={() => goTo("code")} data-testid="goto-code">
              in code
            </button>
            .
          </Callout>
        </div>
      </Section>
      <CheckCard a={a} />
      <Settings />
      <Shapes a={a} />
      <Callout tone="note" lead="Every URL follows the address you asked with." testId="connect-follows">
        Ask through <span className="font-mono">{hostOf(a.endpoint)}</span> and a queue URL comes back as <span className="font-mono">{hostOf(a.endpoint)}/…</span>; ask through <span className="font-mono">localhost:{a.endpoint.split(":").pop()}</span> and it comes back as <span className="font-mono">localhost:{a.endpoint.split(":").pop()}/…</span>. Nothing stored changes when you switch.
      </Callout>
    </div>
  );
}

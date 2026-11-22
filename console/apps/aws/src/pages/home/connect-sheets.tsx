import { Button } from "@doze/ui/components/ui/button";
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import type { About } from "../../api/types/About";
import { CodeBlock } from "../card";
import { hostOf } from "./address";

type Props = { open: boolean; onOpenChange: (open: boolean) => void; a: About };

const url = "font-mono text-[12.5px] break-all text-foreground";
const h3 = "text-[14.5px] font-semibold text-bright";

function Frame({ title, lede, testId, children, ...p }: Props & { title: string; lede: string; testId: string; children: React.ReactNode }) {
  return (
    <Sheet open={p.open} onOpenChange={p.onOpenChange}>
      <SheetContent className="w-full gap-0 overflow-y-auto sm:max-w-[560px]" data-testid={testId}>
        <SheetHeader className="gap-2 px-7 pt-7 pb-4">
          <SheetTitle className="text-[20px] font-semibold tracking-[-0.01em] text-bright">{title}</SheetTitle>
          <SheetDescription className="text-[13.5px]">{lede}</SheetDescription>
        </SheetHeader>
        <div className="grid gap-5 px-7 pb-6">{children}</div>
        <SheetFooter className="border-t px-7 py-4">
          <SheetClose asChild>
            <Button variant="outline" className="bg-card">
              Close
            </Button>
          </SheetClose>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}

function Pair({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border bg-card px-4 py-3">
      <div className="mb-1 text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">{label}</div>
      {children}
    </div>
  );
}

/** Invoke URLs and sync calls: three things AWS reaches by hostname rather than by a call, and where they are here. */
export function InvokeSheet(p: Props) {
  const host = hostOf(p.a.endpoint);
  const port = host.split(":")[1];
  const local = `localhost${port ? `:${port}` : ""}`;
  return (
    <Frame {...p} testId="invoke-sheet" title="Invoke URLs and sync calls" lede="Three things AWS reaches by hostname rather than by a call. On an IP and port the same thing lives under a path.">
      <section data-testid="invoke-api">
        <h3 className={h3}>A deployed API</h3>
        <div className="mt-2 grid gap-2">
          <Pair label="By host name"><code className={url}>{`http://<id>.execute-api.${p.a.region}.${local}/<stage>/`}</code></Pair>
          <Pair label="By path on IP and port"><code className={url}>{`http://${host}/_aws/execute-api/<id>/<stage>/`}</code></Pair>
        </div>
      </section>
      <section data-testid="invoke-lambda">
        <h3 className={h3}>A Lambda function URL</h3>
        <div className="mt-2 grid gap-2">
          <Pair label="By host name"><code className={url}>{`http://<id>.lambda-url.${p.a.region}.${local}/`}</code></Pair>
          <Pair label="By path on IP and port"><code className={url}>{`http://${host}/_aws/lambda-url/${p.a.region}/<id>/`}</code></Pair>
        </div>
      </section>
      <section data-testid="invoke-sync">
        <h3 className={h3}>Step Functions sync calls</h3>
        <p className="mt-1.5 mb-2 text-[13.5px]">
          StartSyncExecution and TestState send to a <span className="font-mono">sync-</span> host. On IP and port there is no host to prefix, so turn the prefix off in the client: <span className="font-mono">disableHostPrefix: true</span> in JavaScript, <span className="font-mono">Config(inject_host_prefix=False)</span> in boto3.
        </p>
      </section>
      <p className="text-[13px] text-muted-foreground">Both URL forms work from a browser or curl with no signature, which is how a webhook or a pasted link reaches them. The host names work where *.localhost resolves to this machine, as it does in browsers and curl.</p>
    </Frame>
  );
}

/** S3 addressing: virtual-hosted or path style, and the setting that picks it in each client. */
export function S3Sheet(p: Props) {
  const host = hostOf(p.a.endpoint);
  const port = host.split(":")[1];
  return (
    <Frame {...p} testId="s3-sheet" title="S3 addressing" lede="S3 can address a bucket two ways. One of them needs a hostname for every bucket.">
      <section data-testid="s3-virtual">
        <h3 className={h3}>Virtual-hosted</h3>
        <div className="mt-2">
          <Pair label="A bucket is a host name"><code className={url}>{`http://product-images.s3.${p.a.region}.localhost${port ? `:${port}` : ""}/sku-FV-TOM-V.jpg`}</code></Pair>
        </div>
        <p className="mt-2 text-[13px] text-muted-foreground">The SDK default. An IP has no hostname space to put a bucket under, so a client left on this style fails to find its bucket unless *.localhost resolves to this machine.</p>
      </section>
      <section data-testid="s3-path">
        <h3 className={h3}>Path style</h3>
        <div className="mt-2">
          <Pair label="Works on every address"><code className={url}>{`http://${host}/product-images/sku-FV-TOM-V.jpg`}</code></Pair>
        </div>
        <p className="mt-2 mb-3 text-[13px] text-muted-foreground">Set it on the client when you use an IP and port:</p>
        <div className="grid gap-3">
          {[
            ["Python · boto3", 'Config(s3={"addressing_style": "path"})'],
            ["Rust · on the S3 config builder", ".force_path_style(true)"],
            ["JavaScript · SDK v3 client option", "forcePathStyle: true"],
            ["AWS CLI", "aws configure set default.s3.addressing_style path"],
          ].map(([what, code]) => (
            <div key={what}>
              <div className="mb-1 text-[12.5px] text-muted-foreground">{what}</div>
              <CodeBlock code={code!} what={`${what} setting copied`} />
            </div>
          ))}
        </div>
      </section>
    </Frame>
  );
}

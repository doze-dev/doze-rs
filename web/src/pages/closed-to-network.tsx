import { LockKeyholeIcon, MonitorIcon, RefreshCwIcon, TerminalIcon } from "lucide-react";
import { Logo, Wordmark } from "@/components/app/brand";
import { Button } from "@/components/ui/button";

/** The whole app, once the console has stopped answering this machine (SEC-250): the broker now has ACLs, and a console that
 * has no login and acts as a super user would get round them from another machine. What happened, why, the two ways forward,
 * and Try again (the console answers again once every ACL is deleted). Not a toast: nothing else on the page could load. */
export function ClosedToNetworkPage({ said }: { said: string }) {
  return (
    <main id="main" className="flex min-h-svh items-center justify-center bg-background px-6 py-16" data-testid="closed-to-network">
      <div className="w-full max-w-xl motion-safe:animate-in motion-safe:fade-in motion-safe:duration-500">
        <div className="mb-8 flex items-center gap-2.5">
          <Logo size={28} />
          <Wordmark className="text-[17px]" />
        </div>
        <div className="rounded-xl border bg-card/60 px-8 py-9">
          <div className="mb-5 flex size-11 items-center justify-center rounded-full bg-accent text-link">
            <LockKeyholeIcon className="size-5" aria-hidden />
          </div>
          <h1 className="text-[22px] font-semibold tracking-[-0.01em] text-bright">This console no longer answers other machines</h1>
          <section className="mt-4 space-y-3 text-[14px] leading-relaxed text-muted-foreground">
            <p>
              <span className="font-medium text-foreground">What happened.</span> Someone made an ACL on this broker after it started, over Kafka or from the console.
            </p>
            <p>
              <span className="font-medium text-foreground">Why it stopped.</span> The console has no login and acts as a super user, so from another machine it would get round those ACLs. It is never a wider door than the broker: while the broker has any ACL, it answers only the machine doze-kafka runs on.
            </p>
          </section>
          <h2 className="mt-7 text-[13px] font-semibold tracking-wide text-foreground uppercase">Two ways forward</h2>
          <ol className="mt-3 space-y-3 text-[14px] leading-relaxed text-muted-foreground">
            <li className="flex gap-3">
              <MonitorIcon className="mt-0.5 size-4 shrink-0 text-link" aria-hidden />
              <span>
                <span className="font-medium text-foreground">Use it from that machine</span>, at a loopback address such as <code className="font-mono text-[13px] text-foreground">127.0.0.1</code>.
              </span>
            </li>
            <li className="flex gap-3">
              <TerminalIcon className="mt-0.5 size-4 shrink-0 text-link" aria-hidden />
              <span>
                <span className="font-medium text-foreground">Accept the exposure</span>: restart doze-kafka with <code className="font-mono text-[13px] text-foreground">--console-unprotected</code>, and the console answers every machine again whatever the ACLs say.
              </span>
            </li>
          </ol>
          <p className="mt-7 border-t pt-5 text-[13px] text-muted-foreground" data-testid="closed-said">
            {said}
          </p>
          <div className="mt-6 flex flex-wrap items-center gap-3">
            <Button variant="outline" onClick={() => window.location.reload()}>
              <RefreshCwIcon /> Try again
            </Button>
            <span className="text-[13px] text-muted-foreground">Once every ACL is deleted, it answers here again.</span>
          </div>
        </div>
      </div>
    </main>
  );
}

// The identity menu (sts__role-session): the chip in the top bar that says which role is in effect, and the sheet it opens: the role's
// display name and colour, Switch back, Switch role, the roles used recently, and the credentials the session holds.
import { Link } from "@tanstack/react-router";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Button } from "@doze/ui/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@doze/ui/components/ui/sheet";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../api/queries";
import { expired, setIdentityOpen, type Session, switchBack, useIdentity } from "./identity";

const DOT = { red: "bg-bad", green: "bg-good" } as const;

/** The session's credentials as the lines a shell takes. */
export const exportLines = (s: Session, region: string): string => [`export AWS_ACCESS_KEY_ID=${s.accessKeyId}`, `export AWS_SECRET_ACCESS_KEY=${s.secretAccessKey}`, `export AWS_SESSION_TOKEN=${s.sessionToken}`, `export AWS_DEFAULT_REGION=${region}`].join("\n");

export function IdentitySheet() {
  const { session, recent, open } = useIdentity();
  const region = useAbout().data?.region ?? "us-east-1";
  const live = session && !expired(session) ? session : null;
  return (
    <Sheet open={open} onOpenChange={setIdentityOpen}>
      <SheetContent className="w-full overflow-y-auto sm:max-w-[26rem]" data-testid="identity-sheet">
        <SheetHeader>
          <SheetTitle className="text-[21px] font-semibold">Identity menu</SheetTitle>
          <SheetDescription>The role&apos;s display name replaces your user name in the top bar while it is in effect.</SheetDescription>
        </SheetHeader>
        <div className="grid gap-6 px-4 pb-6">
          <div className="grid gap-4 rounded-[14px] border bg-card p-5" data-testid="identity-current">
            {live ? (
              <>
                <div className="flex items-center justify-between gap-3">
                  <span className="font-mono text-[14px] break-all text-bright" data-testid="identity-name">{live.display}</span>
                  {live.color && <span className="inline-flex items-center gap-2 rounded-full border bg-pick px-3 py-1 text-[13px] text-bright"><span aria-hidden className={cn("size-2 rounded-full", DOT[live.color])} />{live.color === "red" ? "Red" : "Green"}</span>}
                </div>
                <p className="text-[13px] text-muted-foreground">Session of <span className="font-mono">{live.roleArn}</span>, until {new Date(live.expiresAt).toISOString().slice(0, 19).replace("T", " ")} UTC.</p>
                <div className="flex flex-wrap gap-2">
                  <Button type="button" onClick={switchBack} data-testid="switch-back">Switch back</Button>
                  <Button asChild variant="outline" className="bg-card"><Link to="/resources/$service" params={{ service: "sts" }} onClick={() => setIdentityOpen(false)} data-testid="switch-role-link">Switch role</Link></Button>
                </div>
              </>
            ) : (
              <>
                <p className="text-[14px] text-foreground" data-testid="identity-none">{session ? "The role session has expired." : "You are using your own identity: the console makes its calls as the console."}</p>
                <div><Button asChild><Link to="/resources/$service" params={{ service: "sts" }} onClick={() => setIdentityOpen(false)} data-testid="switch-role-link">Switch role</Link></Button></div>
              </>
            )}
          </div>
          {live && (
            <section className="grid gap-2" aria-label="Credentials" data-testid="identity-credentials">
              <h3 className="text-[15px] font-semibold text-bright">Credentials of the session</h3>
              <p className="text-[13px] text-muted-foreground">AssumeRole returned these. They are real to SDKs and the AWS CLI; the console itself keeps making its own calls as the console.</p>
              <pre className="overflow-auto rounded-[10px] border bg-well px-3 py-2.5 font-mono text-[12px] leading-5 break-all whitespace-pre-wrap">{exportLines(live, region)}</pre>
              <div><CopyButton text={exportLines(live, region)} label="Copy as export lines" what="Credentials copied" /></div>
            </section>
          )}
          <section className="grid gap-2" aria-label="Roles you used recently" data-testid="identity-recent">
            <h3 className="text-[15px] font-semibold text-bright">Roles you used recently</h3>
            {recent.length === 0 ? <p className="text-[13px] text-muted-foreground">None yet: the roles you switch to appear here.</p> : (
              <ul className="grid gap-1">
                {recent.map((r) => <li key={`${r.account}/${r.role}`}><Link to="/resources/$service" params={{ service: "sts" }} search={{ account: r.account, roleName: r.role } as never} onClick={() => setIdentityOpen(false)} className="block rounded-md px-2 py-2 font-mono text-[13px] text-bright hover:bg-accent" data-testid="recent-role">{r.display}</Link></li>)}
              </ul>
            )}
          </section>
          <p className="max-w-[24rem] border-l-2 pl-3.5 text-[13px] leading-5 text-muted-foreground">Only one set of permissions is active at a time: the role&apos;s and yours are not added together, and Switch back returns you to yours. The last several roles you used appear on this menu.</p>
        </div>
      </SheetContent>
    </Sheet>
  );
}

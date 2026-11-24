import { Link } from "@tanstack/react-router";
import { KeyRoundIcon, ShieldIcon } from "lucide-react";
import { Section } from "../card";

const card = "flex items-start gap-3.5 rounded-[var(--radius-card)] border bg-card px-[22px] py-5 transition-colors hover:border-input focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none";
const mark = "inline-flex size-9 flex-none items-center justify-center rounded-[var(--radius-field)] border bg-muted";

/** Identity and access: IAM and STS, the lens on everything else. Their pages are in Resources; they are reached from here. */
export function IdentityCards() {
  return (
    <Section title="Identity and access" testId="identity">
      <div className="grid gap-6 md:grid-cols-2 xl:grid-cols-3">
        <Link to="/resources/$service" params={{ service: "iam" }} className={card} data-testid="access-iam">
          <span className={mark}>
            <ShieldIcon aria-hidden className="size-[18px] text-primary" />
          </span>
          <span>
            <span className="block text-[16px] font-semibold text-bright">IAM</span>
            <span className="mt-0.5 block text-[13px] text-muted-foreground">Users, roles, groups, policies and access keys</span>
          </span>
        </Link>
        <Link to="/resources/$service" params={{ service: "sts" }} className={card} data-testid="access-sts">
          <span className={mark}>
            <KeyRoundIcon aria-hidden className="size-[18px] text-primary" />
          </span>
          <span>
            <span className="block text-[16px] font-semibold text-bright">STS</span>
            <span className="mt-0.5 block text-[13px] text-muted-foreground">Who a caller is, and temporary sessions from a role</span>
          </span>
        </Link>
      </div>
    </Section>
  );
}

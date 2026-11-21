import { PageHeader } from "@doze/ui/components/app/page-header";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { useTheme } from "@doze/ui/lib/theme";
import { cn } from "@doze/ui/lib/utils";
import { PRODUCTS, useProduct } from "../product";
import { Section, Specimen } from "../specimen";

const GROUPS: { name: string; roles: [role: string, use: string][] }[] = [
  {
    name: "Grounds",
    roles: [
      ["--background", "The page"],
      ["--card", "Cards, tables"],
      ["--popover", "Menus, popovers"],
      ["--sidebar", "The rail"],
      ["--muted", "Quiet fills"],
      ["--secondary", "Secondary buttons"],
      ["--accent", "Hover, the chosen item"],
      ["--well", "A table's head, code"],
      ["--track", "Tracks, meters"],
      ["--pick", "Selection, a new row"],
    ],
  },
  {
    name: "Text and lines",
    roles: [
      ["--bright", "Headings, figures"],
      ["--foreground", "Body"],
      ["--muted-foreground", "Captions, labels"],
      ["--link", "Text that stands out"],
      ["--border", "Dividers"],
      ["--input", "A field's border"],
    ],
  },
  {
    name: "Brand and focus",
    roles: [
      ["--primary", "The primary action"],
      ["--primary-foreground", "Text on it"],
      ["--ring", "The focus ring"],
      ["--sidebar-primary", "The rail's mark"],
      ["--sidebar-accent", "The rail's chosen item"],
      ["--scrim", "Behind an overlay"],
    ],
  },
  {
    name: "Status",
    roles: [
      ["--good", "Success, on"],
      ["--warn", "Needs a look"],
      ["--bad", "Failure"],
      ["--destructive", "A destructive action"],
      ["--cool", "Information"],
      ["--refused", "The service refused it"],
      ["--tint-good", "Tint"],
      ["--tint-warn", "Tint"],
      ["--tint-brand", "Tint"],
      ["--tint-cool", "Tint"],
    ],
  },
];

const VALUES: [role: string, use: string][] = [
  ["--radius", "A button, a field"],
  ["--body-size", "Body text"],
  ["--control-h", "A control's height"],
  ["--row-h", "A table row"],
  ["--dur-in", "An overlay coming in"],
  ["--dur-out", "An overlay going out"],
  ["--ease", "Their curve"],
  ["--dur-highlight", "A new row's highlight"],
];

/** A role's value as the page has it now. Read while rendering: the theme is already on <html> when this draws. */
const valueOf = (role: string) => getComputedStyle(document.documentElement).getPropertyValue(role).trim();

function Swatch({ role, use }: { role: string; use: string }) {
  return (
    <div className="flex min-w-0 items-center gap-3" data-role={role}>
      <span aria-hidden className="size-10 shrink-0 rounded-lg border" style={{ background: `var(${role})` }} />
      <span className="min-w-0">
        <code className="block truncate text-[12.5px] text-bright">{role}</code>
        <span className="block truncate text-[12px] text-muted-foreground">
          <span className="font-mono">{valueOf(role)}</span> · {use}
        </span>
      </span>
    </div>
  );
}

export function FoundationsPage() {
  // Subscribed so the values shown are read again when either changes.
  const product = useProduct();
  const { theme } = useTheme();
  const label = PRODUCTS.find((p) => p.value === product)!.label;
  return (
    <>
      <PageHeader
        title="Foundations"
        sub={
          <>
            Every shared component reads roles, never a colour. This page shows the roles as <b className="font-medium text-foreground">{label}</b> sets them for {theme}: switch the product or the mode in the top bar and only these values change.
          </>
        }
      />
      <Section id="roles" title="Roles" source="themes/*.css" note="A theme file sets each of these for light and for dark. The library's test fails when a role it reads is missing from either theme.">
        {GROUPS.map((g) => (
          <Specimen key={g.name} label={g.name} stack>
            <div className="grid w-full grid-cols-1 gap-x-6 gap-y-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
              {g.roles.map(([role, use]) => (
                <Swatch key={role} role={role} use={use} />
              ))}
            </div>
          </Specimen>
        ))}
      </Section>

      <Section id="status" title="Status words" source="components/app/status-dot.tsx" note="Colour never carries meaning alone: every status colour sits beside a word.">
        <Specimen label="On the page">
          <StatusDot tone="ok">Running</StatusDot>
          <StatusDot tone="warn">Would be denied</StatusDot>
          <StatusDot tone="bad">Failed</StatusDot>
          <StatusDot tone="info">Replaying</StatusDot>
          <StatusDot tone="idle">Not checked</StatusDot>
          <span className="inline-flex items-center gap-1.5 text-[13px] font-semibold text-refused">
            <span aria-hidden className="size-2 rounded-full bg-current" />
            Refused
          </span>
        </Specimen>
        <Specimen label="On a tint">
          {(
            [
              ["bg-tint-good text-good", "Delivered"],
              ["bg-tint-warn text-warn", "Held"],
              ["bg-tint-cool text-cool", "Note"],
              ["bg-tint-brand text-bright", "New"],
            ] as const
          ).map(([cls, word]) => (
            <span key={word} className={cn("rounded-md px-2.5 py-1 text-[12.5px] font-medium", cls)}>
              {word}
            </span>
          ))}
        </Specimen>
      </Section>

      <Section id="type" title="Type" source="--font-sans · --font-display · --font-mono">
        <Specimen label="Display">
          <span className="font-display text-[34px] leading-[42px] font-semibold tracking-[-0.02em] text-bright">Queues, quietly</span>
        </Specimen>
        <Specimen label="Page title">
          <span className="text-[34px] leading-[42px] font-light tracking-[-0.03em] text-bright">Twelve queues are awake</span>
        </Specimen>
        <Specimen label="Body">
          <p className="max-w-[40rem]">A console says what the service did, in plain words, at the size the theme sets for body text. Numbers stand in tabular figures so a changing value moves nothing beside it.</p>
        </Specimen>
        <Specimen label="Caption">
          <span className="text-[12px] font-medium tracking-[0.06em] text-muted-foreground uppercase">Waiting · in flight · delayed</span>
        </Specimen>
        <Specimen label="Mono">
          <code className="text-[13px] text-bright">arn:aws:sqs:us-east-1:000000000000:orders</code>
          <code className="text-[13px] tabular-nums">14:32:07 UTC · 12,345</code>
        </Specimen>
      </Section>

      <Section id="shape" title="Shape, density and motion" source="--radius · --control-h · --row-h · --dur-in · --dur-out · --ease">
        <Specimen label="Radius">
          {(["rounded-sm", "rounded-md", "rounded-lg", "rounded-xl"] as const).map((r) => (
            <span key={r} className={cn("flex h-14 w-24 items-center justify-center border bg-card font-mono text-[12px] text-muted-foreground", r)}>
              {r.replace("rounded-", "")}
            </span>
          ))}
        </Specimen>
        <Specimen label="Density">
          <span className="flex h-(--control-h) items-center rounded-md border bg-card px-3 text-[13px]">A control is this tall</span>
          <span className="flex items-center border-y bg-card px-3 text-[13px]" style={{ height: "var(--row-h)" }}>
            A table row is this tall
          </span>
        </Specimen>
        <Specimen label="Values" stack>
          <dl className="grid w-full grid-cols-1 gap-x-6 gap-y-3 sm:grid-cols-2 lg:grid-cols-4">
            {VALUES.map(([role, use]) => (
              <div key={role} className="min-w-0" data-role={role}>
                <dt className="truncate text-[12px] text-muted-foreground">{use}</dt>
                <dd className="truncate font-mono text-[13px] text-bright">
                  {role} <span className="text-muted-foreground">{valueOf(role)}</span>
                </dd>
              </div>
            ))}
          </dl>
        </Specimen>
      </Section>
    </>
  );
}

import { ChevronDownIcon, CopyIcon, RadioIcon } from "lucide-react";
import { copyText } from "@doze/ui/components/app/copy";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@doze/ui/components/ui/dropdown-menu";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Tabs, TabsList, TabsTrigger } from "@doze/ui/components/ui/tabs";
import { useUrlParam } from "@doze/ui/hooks/use-url-param";
import { cn } from "@doze/ui/lib/utils";
import { Link } from "@tanstack/react-router";
import type { ResourceDetail, Verb, Wired } from "./contract";
import { DetailBadgeView, DetailFields, Sentence, WiredStrip } from "./resource-parts";

export type ResourceTab = { id: string; label: string; /** Shown after the label (`Graphed metrics 2`). */ count?: number | null; render: () => React.ReactNode };
/** A verb's state on this page: off, and why (the reason is its tooltip and the words under the row). */
export type VerbState = { disabled?: string; /** Off with the reason on hover only: no line of reasons under the header (the board's greyed button). */ quiet?: boolean };

/** Which tab is shown: the one the address names when it is one of the tabs, else the first. */
export function currentTab(tabs: readonly { id: string }[], asked: string | undefined): string {
  return tabs.find((t) => t.id === asked)?.id ?? tabs[0]?.id ?? "";
}

/** A resource's page, one shape for every kind: the header (name in type, its badges, the verbs row, the Actions menu), the live
 * sentence, the Wired to strip, whatever `overview` the kind puts above its tabs, then the tabs (kept in the address as `?tab=`).
 * With no tab of its own a kind gets a Details tab of every field, so no resource is a blank page. Only the tab shown is drawn,
 * so a tab's reads are made when it is opened. */
export function ResourcePage({ detail, meta, banner, overview, tabs = [], tab, onTab, onVerb, verbStates, primary, actions, renderValue, refresh, details = true, sentenceBold, wiredSelf, wiredText, wiredHref, wiredLead, bannerAbove = false, trail, testId = "resource-page" }: { detail: ResourceDetail; /** Under the name: `Created Nov 6, 2026 18:42.` */ meta?: React.ReactNode; banner?: React.ReactNode; overview?: React.ReactNode; tabs?: ResourceTab[]; tab?: string; onTab?: (id: string) => void; onVerb?: (verb: Verb) => void; verbStates?: Record<string, VerbState>; /** The verb drawn as the main button. */ primary?: string; /** More items for the Actions menu. */ actions?: { label: string; onSelect: () => void }[]; renderValue?: React.ComponentProps<typeof DetailFields>["renderValue"]; refresh?: React.ReactNode; /** The fallback Details tab (every field the descriptor read) is added unless `false`: a kind whose tabs say it all leaves it out. */ details?: boolean; /** Figures of the sentence to set in bold (a pattern: `/\\d+(?= messages)/`); the names it says are set apart already. */ sentenceBold?: readonly (string | RegExp)[]; /** Draw the resource itself in the Wired to strip, after this many of what flows in. */ wiredSelf?: number; /** A chip's own words in the strip. */ wiredText?: (w: Wired) => string; /** Where a chip of the strip goes, when it is not the resource it names. */ wiredHref?: (w: Wired) => string | undefined; /** The Wired to strip's first words (`Triggered by`). */ wiredLead?: string; /** Draw the banner above the header (what was just done is the first thing seen). */ bannerAbove?: boolean; /** More crumbs after the kind's (the resource and the tab on screen). */ trail?: { label: string; to: string }[]; testId?: string }) {
  const [asked, setAsked] = useUrlParam("tab");
  const all: ResourceTab[] = overview || !details || tabs.some((t) => t.id === "details") ? tabs : [{ id: "details", label: "Details", render: () => <DetailFields detail={detail} renderValue={renderValue} /> }, ...tabs];
  const shown = currentTab(all, tab ?? asked);
  const choose = (id: string) => (onTab ? onTab(id) : setAsked(id === all[0]?.id ? undefined : id));
  const verbs = detail.verbs ?? [];
  const off = verbs.filter((v) => verbStates?.[v.id]?.disabled && !verbStates[v.id]?.quiet);
  return (
    <div data-testid={testId}>
      {banner && bannerAbove && <div className="mb-6">{banner}</div>}
      <PageHeader
        title={detail.name}
        mono
        crumbs={[{ label: "Resources", to: "/resources" }, { label: detail.group, to: `/resources/${detail.service}` }, { label: detail.service_title, to: `/resources/${detail.service}` }, { label: detail.kind_title, to: `/resources/${detail.service}/${detail.kind}` }, ...(trail ?? [])]}
        sub={meta}
        refresh={refresh}
        titleExtra={detail.badges && detail.badges.length > 0 ? <span className="flex flex-wrap items-center gap-2">{detail.badges.map((b) => <DetailBadgeView key={b.text} badge={b} />)}</span> : undefined}
        actions={
          <>
            {verbs.map((v) => {
              const why = verbStates?.[v.id]?.disabled;
              return (
                <Button key={v.id} type="button" variant={v.id === primary ? "default" : "outline"} className={cn(v.id !== primary && "bg-card", v.danger && "border-bad/40 text-bad hover:text-bad")} disabled={Boolean(why)} title={why} onClick={() => onVerb?.(v)} data-testid={`verb-${v.id}`}>
                  {v.label}
                </Button>
              );
            })}
            <ActionsMenu detail={detail} extra={actions} />
          </>
        }
      />
      {off.length > 0 && (
        <ul className="-mt-4 mb-6 grid gap-0.5 text-[12.5px] text-muted-foreground" data-testid="verb-reasons">
          {off.map((v) => (
            <li key={v.id}>
              <b className="font-medium text-foreground">{v.label}</b> is off: {verbStates![v.id]!.disabled}
            </li>
          ))}
        </ul>
      )}
      {banner && !bannerAbove && <div className="mb-6">{banner}</div>}
      {detail.sentence && <Sentence text={detail.sentence} name={detail.name} also={detail.wired?.map((w) => w.to.name)} bold={sentenceBold} />}
      {detail.wired && <WiredStrip wired={detail.wired} self={wiredSelf === undefined ? undefined : { name: detail.name, at: wiredSelf }} chipText={wiredText} chipHref={wiredHref} lead={wiredLead} />}
      {overview}
      {all.length > 0 && <Tabs value={shown} onValueChange={choose} className={cn("gap-0", overview && "mt-10")}>
        <TabsList variant="line" className="h-11 w-full justify-start gap-2 overflow-x-auto rounded-none border-b p-0" aria-label={`${detail.name}'s pages`}>
          {all.map((t) => (
            <TabsTrigger key={t.id} value={t.id} className="h-11 flex-none px-3 text-[14px] after:bottom-[-1px]" data-testid={`tab-${t.id}`}>
              {t.label}
              {t.count !== undefined && t.count !== null && <span className="rounded-full bg-muted px-1.5 text-[11.5px] text-muted-foreground tabular-nums">{t.count}</span>}
            </TabsTrigger>
          ))}
        </TabsList>
        <div className="pt-6" role="tabpanel" aria-label={all.find((t) => t.id === shown)?.label} data-testid="tab-panel">
          {all.find((t) => t.id === shown)?.render()}
        </div>
      </Tabs>}
    </div>
  );
}

function ActionsMenu({ detail, extra }: { detail: ResourceDetail; extra?: { label: string; onSelect: () => void }[] }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="outline" className="bg-card" data-testid="actions-menu">
          Actions <ChevronDownIcon />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="min-w-52">
        {extra?.map((a) => (
          <DropdownMenuItem key={a.label} onSelect={a.onSelect}>
            {a.label}
          </DropdownMenuItem>
        ))}
        {extra && extra.length > 0 && <DropdownMenuSeparator />}
        <DropdownMenuItem onSelect={() => void copyText(detail.name, "Name copied")}>
          <CopyIcon /> Copy name
        </DropdownMenuItem>
        {detail.id && detail.id !== detail.name && (
          <DropdownMenuItem onSelect={() => void copyText(detail.id, "Copied")}>
            <CopyIcon /> Copy {detail.id.startsWith("arn:") ? "ARN" : detail.id.startsWith("http") ? "URL" : "ID"}
          </DropdownMenuItem>
        )}
        <DropdownMenuItem asChild>
          <Link to={"/live" as never}>
            <RadioIcon /> Open Live
          </Link>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** What a resource's page shows while its detail is being read: the same shape, grey. */
export function ResourcePageSkeleton() {
  return (
    <div className="grid gap-6" data-testid="resource-page-loading">
      <Skeleton className="h-12 w-80 rounded-lg" />
      <Skeleton className="h-6 w-2/3 rounded-lg" />
      <Skeleton className="h-16 rounded-[14px]" />
      <Skeleton className="h-72 rounded-[14px]" />
    </div>
  );
}

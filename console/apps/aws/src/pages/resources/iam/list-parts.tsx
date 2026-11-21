// The pieces of IAM's four lists: the search field, the line under the table ("1 of 1 page, 3 users"), the banner a delete leaves, and
// what a list answers while it reads and when it cannot.
import { Link, useNavigate } from "@tanstack/react-router";
import { SearchIcon } from "lucide-react";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { count } from "./model";

export function SearchBox({ value, onChange, noun }: { value: string; onChange: (v: string) => void; noun: string }) {
  return (
    <div className="relative w-full max-w-[26rem]">
      <SearchIcon aria-hidden className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input value={value} onChange={(e) => onChange(e.target.value)} placeholder={`Search ${noun}`} aria-label={`Search ${noun}`} className="bg-card pl-9" data-testid="list-search" />
    </div>
  );
}

/** `1 of 1 page, 3 users`: one ListX page holds up to 100; more is read on asking. */
export function PageLine({ shown, noun, many, more, loading, onMore }: { shown: number; noun: string; many?: string; more: boolean; loading: boolean; onMore: () => void }) {
  return (
    <div className="mt-4 flex flex-wrap items-center justify-end gap-3 text-[13px] text-muted-foreground" data-testid="page-line">
      <span>{more ? `${count(shown, noun, many)} so far, more to read` : `1 of 1 page, ${count(shown, noun, many)}`}</span>
      {more && (
        <Button type="button" variant="outline" size="sm" className="bg-card" onClick={onMore} disabled={loading} data-testid="load-more">
          Next
        </Button>
      )}
    </div>
  );
}

/** What a delete leaves on the list it lands on: that it is done, and what happened to what was attached. */
export function GoneBanner({ kind, name, back }: { kind: string; name: string; back: () => void }) {
  return <ResultBanner className="mb-6" title={`${kind} ${name} deleted.`} returned="The delete answered, and it is gone from the list." onDismiss={back} testId="deleted-banner" />;
}

export function ListLoading() {
  return (
    <div className="grid gap-3" data-testid="list-loading">
      <Skeleton className="h-9 w-80" />
      <Skeleton className="h-64 rounded-xl" />
    </div>
  );
}

export function ListError({ what, error, retry, retrying }: { what: string; error: unknown; retry: () => void; retrying: boolean }) {
  return <ErrorBlock what={what} error={error} hint="Check that doze-aws is still running, then try again." onRetry={retry} retrying={retrying} testId="list-error" />;
}

/** Goes back to a kind's list, clearing what the address holds. */
export function useToList(kind: string) {
  const navigate = useNavigate();
  return (search: Record<string, string> = {}) => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: kind }, search });
}

/** What a create leaves on the list it lands on: it is made, what was added with it, and the way to its page. */
export function CreatedBanner({ kind, noun, name, kindId, returned, back }: { kind: string; noun: string; name: string; kindId: string; returned?: React.ReactNode; back: () => void }) {
  return (
    <ResultBanner
      className="mb-6"
      title={`${kind} ${name.startsWith("arn:") ? (name.split("/").pop() ?? name) : name} created.`}
      returned={returned}
      actions={<Button asChild size="sm"><Link to="/resources/$service/$kind/$name" params={{ service: "iam", kind: kindId, name }} data-testid="view-created">View {noun}</Link></Button>}
      onDismiss={back}
      testId="created-banner"
    />
  );
}

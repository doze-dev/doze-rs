import { ChevronDownIcon } from "lucide-react";
import { number } from "../../lib/format";
import { Button } from "../ui/button";

/** The line under a list: `1 to 25 of 25`. When the service says there is more (a token) and no total is known, the count is
 * what is loaded and the line says so: `1 to 25, more to load`. Nothing loaded: `0 results`. */
export function pageLine(shown: number, total?: number | null, more = false): string {
  if (shown <= 0) return more ? "No results on this page, more to load" : total ? `0 of ${number(total)}` : "0 results";
  if (total !== undefined && total !== null && !more) return `1 to ${number(shown)} of ${number(Math.max(total, shown))}`;
  if (total !== undefined && total !== null) return `1 to ${number(shown)} of ${number(total)}`;
  return more ? `1 to ${number(shown)}, more to load` : `1 to ${number(shown)} of ${number(shown)}`;
}

/** The paging line of a list and, while the service has more, its Load more button (the next page is read when asked for). */
export function Pagination({ shown, total, more = false, onMore, loading = false, noun = "", className }: { shown: number; total?: number | null; more?: boolean; onMore?: () => void; loading?: boolean; noun?: string; className?: string }) {
  return (
    <div className={`flex flex-wrap items-center justify-between gap-3 text-[13px] text-muted-foreground ${className ?? ""}`} data-slot="pagination">
      <span aria-live="polite" data-testid="page-line">
        {pageLine(shown, total, more)}{noun && !more && shown > 0 ? ` ${noun}` : ""}
      </span>
      {more && onMore && (
        <Button type="button" variant="outline" size="sm" className="bg-card" onClick={onMore} disabled={loading} data-testid="load-more">
          <ChevronDownIcon /> {loading ? "Loading…" : "Load more"}
        </Button>
      )}
    </div>
  );
}

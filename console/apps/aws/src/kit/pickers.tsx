import { useEffect } from "react";
import { Combobox, type Option } from "@doze/ui/components/app/combobox";
import { useResourceList } from "../api/resources";
import type { ResourceRow } from "../api/types/ResourceRow";

/** The picker's choices from a kind's rows: each is its label, valued by what a call takes (`id`: the queue's URL, the topic's
 * ARN) or by its name. Sorted by label, so a long list is findable. */
export function pickerOptions(rows: readonly ResourceRow[], by: "id" | "name" = "id"): Option[] {
  return rows.map((r) => ({ value: by === "id" ? r.id : r.name, label: r.label })).sort((a, b) => a.label.localeCompare(b.label));
}

/** One resource of a kind, chosen from the list the console reads for that kind (the same list the Resources space shows). It
 * reads further pages (up to `PAGES`) so the whole list is there to search. Says so while it reads, when the read fails, and
 * when there is nothing to choose (`noun`: `queue`). */
export function ResourcePicker({ service, kind, noun, value, onChange, by = "id", id, className, hide, "aria-label": ariaLabel }: { service: string; kind: string; noun: string; value: string | null; /** Rows left out of the choices (a queue the call would refuse). */ hide?: (row: ResourceRow) => boolean; onChange: (value: string | null) => void; by?: "id" | "name"; id?: string; className?: string; "aria-label"?: string }) {
  const q = useResourceList(service, kind);
  const { hasNextPage, isFetchingNextPage, fetchNextPage } = q;
  const pages = q.data?.pages.length ?? 0;
  useEffect(() => {
    if (hasNextPage && !isFetchingNextPage && pages < PAGES) void fetchNextPage();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage, pages]);
  const rows = q.data?.pages.flatMap((p) => p.rows) ?? [];
  const options = pickerOptions(hide ? rows.filter((r) => !hide(r)) : rows, by);
  const a = /^[aeiou]/i.test(noun) ? "an" : "a";
  const placeholder = q.isError ? `The ${noun}s could not be read` : q.isPending ? `Reading ${noun}s…` : options.length === 0 ? `There are no ${noun}s yet` : `Choose ${a} ${noun}`;
  return (
    <div className="grid gap-1" data-slot="resource-picker" data-state={q.isError ? "error" : q.isPending ? "loading" : "ready"}>
      <Combobox options={options} value={value} onChange={onChange} placeholder={placeholder} search={`Search ${noun}s…`} empty={`No ${noun} matches.`} id={id} aria-label={ariaLabel ?? `Choose ${a} ${noun}`} className={className ?? "w-full"} />
      {q.isError && (
        <p className="text-[12.5px] text-bad" role="alert" data-testid="picker-error">
          The {noun}s could not be read.{" "}
          <button type="button" className="rounded-sm font-medium text-link hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none" onClick={() => void q.refetch()}>
            Try again
          </button>
        </p>
      )}
    </div>
  );
}

/** The most pages a picker reads on its own. */
export const PAGES = 5;

type P = Omit<React.ComponentProps<typeof ResourcePicker>, "service" | "kind" | "noun">;
export const QueuePicker = (p: P) => <ResourcePicker service="sqs" kind="queues" noun="queue" {...p} />;
export const TopicPicker = (p: P) => <ResourcePicker service="sns" kind="topics" noun="topic" {...p} />;
export const FunctionPicker = (p: P) => <ResourcePicker service="lambda" kind="functions" noun="function" {...p} />;
export const RolePicker = (p: P) => <ResourcePicker service="iam" kind="roles" noun="role" {...p} />;
export const KeyPicker = (p: P) => <ResourcePicker service="kms" kind="keys" noun="key" {...p} />;
export const BusPicker = (p: P) => <ResourcePicker service="eventbridge" kind="buses" noun="event bus" by="name" {...p} />;
export const PolicyPicker = (p: P) => <ResourcePicker service="iam" kind="policies" noun="policy" {...p} />;

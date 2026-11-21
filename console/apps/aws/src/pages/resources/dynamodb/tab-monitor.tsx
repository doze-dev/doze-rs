// A table's Monitor tab (dynamodb__table-monitor): CloudWatch Contributor Insights of the table and of each global secondary index, read
// with DescribeContributorInsights and set with UpdateContributorInsights. doze-aws keeps the setting and answers it as AWS does;
// it computes no most-accessed or most-throttled keys (crates/aws-dynamodb/src/insights.rs).
import { useMutation, useQueries, useQueryClient } from "@tanstack/react-query";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { LocalStatus } from "../../../kit";
import { Card } from "../deep/parts";
import { callOp, deepKey } from "../deep/api";
import { OnOff } from "./tab-parts";
import { tableTarget } from "./parts";

type Answer = { ContributorInsightsStatus?: string };
type Row = { label: string; index: string | null; status: string | null };

export function MonitorTab({ name, indexes }: { name: string; indexes: string[] }) {
  const t = tableTarget(name);
  const qc = useQueryClient();
  const targets = [{ label: name, index: null as string | null }, ...indexes.map((i) => ({ label: i, index: i }))];
  const reads = useQueries({
    queries: targets.map((x) => ({
      queryKey: deepKey(t, "DescribeContributorInsights", x.index ?? ""),
      queryFn: ({ signal }: { signal: AbortSignal }) => callOp<Answer>(t, "DescribeContributorInsights", x.index ? { IndexName: x.index } : {}, signal),
      retry: false,
    })),
  });
  const rows: Row[] = targets.map((x, i) => ({ ...x, status: reads[i]?.data?.ContributorInsightsStatus ?? null }));
  const loading = reads.some((r) => r.isPending);
  const on = rows.every((r) => r.status === "ENABLED");
  const set = useMutation({
    mutationFn: async (enable: boolean) => {
      for (const r of rows.filter((x) => (x.status === "ENABLED") !== enable)) await callOp(t, "UpdateContributorInsights", { ContributorInsightsAction: enable ? "ENABLE" : "DISABLE", ...(r.index ? { IndexName: r.index } : {}) });
    },
    onSuccess: () => qc.invalidateQueries({ queryKey: deepKey(t) }),
  });
  const failed = set.error ? errorWords(set.error) : null;
  return (
    <Card title="CloudWatch Contributor Insights" lede="Shows the most accessed and most throttled items of a table or global secondary index." testId="insights">
      <div className="grid gap-5">
        <div className="flex flex-wrap items-center gap-3">
          <Button disabled={loading || set.isPending} onClick={() => set.mutate(!on)} data-testid="insights-toggle">
            {set.isPending ? "Working…" : on ? "Turn off CloudWatch Contributor Insights" : "Turn on CloudWatch Contributor Insights"}
          </Button>
          {indexes.length > 0 && <span className="text-[13px] text-muted-foreground">Applies to the table and each of its indexes, a call each.</span>}
        </div>
        {failed && (
          <Alert tone="bad" title={`UpdateContributorInsights was refused${failed.code ? `: ${failed.code}` : ""}`} testId="insights-error">
            {failed.message}
          </Alert>
        )}
        {loading ? (
          <Skeleton className="h-24 rounded-xl" />
        ) : (
          <table className="w-full border-separate border-spacing-0 text-[13.5px]" aria-label="Contributor Insights" data-testid="insights-table">
            <thead>
              <tr className="text-left text-[12px] font-medium tracking-[0.04em] text-muted-foreground uppercase">
                <th className="h-10 rounded-l-lg bg-well px-4">Resource</th>
                <th className="h-10 rounded-r-lg bg-well px-4">ContributorInsightsStatus</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.label} data-testid={`insights-${r.label}`}>
                  <td className="h-12 border-b px-4 font-mono text-[13px] text-bright">{r.label}</td>
                  <td className="h-12 border-b px-4"><OnOff on={r.status === "ENABLED"} yes="ENABLED" no={r.status ?? "DISABLED"} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <LocalStatus state="stored" reason="doze-aws has no CloudWatch Contributor Insights rule behind the setting: DescribeContributorInsights reports what was set, and no most-accessed or most-throttled keys are computed." />
      </div>
    </Card>
  );
}

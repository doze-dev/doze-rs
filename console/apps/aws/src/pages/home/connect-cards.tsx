import { Link } from "@tanstack/react-router";
import { ErrorBlock } from "@doze/ui/components/app/alert";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { plural } from "@doze/ui/lib/format";
import type { About } from "../../api/types/About";
import type { HomeView } from "../../api/types/HomeView";
import { ago, clientName, who } from "../../people";
import { Card, CardSkeleton, CopyRow } from "../card";
import { ADDRESS } from "./address";

const HINT = "Check that doze-aws is still running, then try again. Nothing was changed.";

/** Where your app connects: the address, and the same as an environment variable. */
export function WhereCard({ a }: { a: About | undefined }) {
  return (
    <Card testId="card-connect" title="Where your app connects" lede="Every service answers on this one address. Point an AWS SDK or the AWS CLI at it. Under the doze app the same instance also answers on a .doze name.">
      {a ? (
        // grid-cols-1: a track of minmax(0, 1fr), so a long value is cut with an ellipsis instead of pushing Copy out of the card.
        <div className="grid grid-cols-1">
          <CopyRow label={ADDRESS.label} value={a.endpoint} testId="endpoint" />
          <CopyRow label="As an environment variable" value={`AWS_ENDPOINT_URL=${a.endpoint}`} testId="endpoint-env" />
        </div>
      ) : (
        <CardSkeleton rows={2} />
      )}
    </Card>
  );
}

/** Is my app connected: the app that called newest, which client it used, and when. The address it reached is the one this
 * instance has; a name beside it is not guessed. */
export function ConnectedCard({ home, now }: { home: { data: HomeView | undefined; isError: boolean; error: unknown; isFetching: boolean; refetch: () => unknown }; now: number }) {
  const h = home.data;
  const first = h?.callers[0];
  const w = first ? who(first.principal) : null;
  const more = (h?.callers.length ?? 0) - 1;
  return (
    <Card testId="card-connected" title="Is my app connected?" lede="The check ticks when a request from an app reaches this address, and shows which app and client sent it. This console's own calls do not count.">
      {home.isError && !h ? (
        <ErrorBlock what="The Live record could not be read" error={home.error} hint={HINT} onRetry={() => void home.refetch()} retrying={home.isFetching} testId="home-error" />
      ) : !h ? (
        <CardSkeleton rows={2} />
      ) : first && w ? (
        <div data-testid="connected" data-state="yes">
          <div className="grid grid-cols-1">
            <div className="flex items-center gap-4 pb-1" data-testid="connected-caller" data-key={first.principal.key}>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] text-muted-foreground">{ADDRESS.label}</div>
                <div className="mt-1" data-testid="connected-heard">
                  <StatusDot tone="ok">
                    <span title={w.title}>
                      Heard from {w.label}
                      {first.sdks[0] ? ` (${clientName(first.sdks[0])})` : ""}
                    </span>
                  </StatusDot>
                </div>
              </div>
              <span className="shrink-0 text-[13px] text-muted-foreground">{ago(first.last_ms, now)}</span>
            </div>
            {more > 0 && (
              <p className="mt-3 text-[13px] text-muted-foreground" data-testid="connected-more">
                and {plural(more, "other app")} in{" "}
                <Link to="/live" className="text-link hover:underline">
                  Live
                </Link>
              </p>
            )}
          </div>
        </div>
      ) : (
        <div data-testid="connected" data-state="no">
          <div className="text-[13px] text-muted-foreground">{ADDRESS.label}</div>
          <div className="mt-1">
            <StatusDot tone="idle">Waiting for the first call</StatusDot>
          </div>
          <p className="mt-2 text-[13px] text-muted-foreground">Nothing has called since doze-aws started. This turns over by itself when a call arrives.</p>
        </div>
      )}
      <div className="mt-4 border-t pt-4 text-[13.5px]">
        <Link to="/connect" className="text-link hover:underline">
          Snippets, environment variables and every service&apos;s address
        </Link>
      </div>
    </Card>
  );
}

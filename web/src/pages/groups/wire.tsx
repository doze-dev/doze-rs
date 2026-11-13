// The Wire tab of a group (board group-wire): the Live slice's dock of the tape filtered to this group, with its chips
// (All, Join and sync, Commits, Heartbeats, Refused) and its filter box. The same dock is the Members tab's "What just
// happened".
import { getRouteApi } from "@tanstack/react-router";
import { WireDock } from "@/pages/live/wire-dock";

const route = getRouteApi("/groups/$group/wire");

export function GroupWirePage() {
  const { group: id } = route.useParams();
  return (
    <div data-testid="group-wire">
      <WireDock group={id} />
    </div>
  );
}

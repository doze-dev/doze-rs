// What the page of an archive, a replay or an API destination says when it opens from what was just done. Pure, so it is tested without a browser.

/** The banner a page opens with after `done`: what was made and what the call returned. */
export function ownBanner(kind: string, name: string, done: string | undefined, cancelled: boolean): { title: string; returned: string } | null {
  if (cancelled) return { title: `Replay ${name} cancelled.`, returned: "CancelReplay was accepted. The replay is Cancelling, then Cancelled; the events it already sent stay sent." };
  if (done !== "created") return null;
  if (kind === "archives") return { title: `Archive ${name} created.`, returned: "CreateArchive returned its ARN and its state. Replay sends what it holds back to its bus." };
  if (kind === "replays") return { title: `Replay ${name} started.`, returned: "StartReplay returned its ARN and the state Starting. It runs in the background, and can be cancelled while it is Starting or Running." };
  if (kind === "api-destinations") return { title: `API destination ${name} created.`, returned: "CreateApiDestination returned its ARN and its state. A rule sends to it as a target." };
  return null;
}

// The call a form will make, shown as it is filled in: `POST /api/v1/preview`, asked once the person pauses.
import { useQuery } from "@tanstack/react-query";
import { client } from "../api/client";
import type { Preview, PreviewRequest } from "./contract";
import { useDebounced } from "./debounce";

export const PREVIEW_PATH = "/api/v1/preview";
/** How long the form is still before the preview is asked for again (ms). */
export const PREVIEW_PAUSE = 250;

/** The request as one string: what makes two requests the same, so a query is not made again for an input that did not change. */
export const previewKey = (r: PreviewRequest): string => JSON.stringify([r.service, r.operation, r.input]);

export function fetchPreview(r: PreviewRequest, signal?: AbortSignal): Promise<Preview> {
  return client.api<Preview>(PREVIEW_PATH, { method: "POST", body: JSON.stringify(r), headers: { "Content-Type": "application/json" } }, signal);
}

/** Whether two requests (each as `previewKey` wrote it) are for the same operation of the same service: the body of one is a fair stand-in for
 * the other while it is on its way, the body of a different operation is not (it would sit under the new operation's name). */
export function sameOperation(a: string | undefined, b: string | undefined): boolean {
  try {
    const x = JSON.parse(a ?? "null") as unknown[] | null;
    const y = JSON.parse(b ?? "null") as unknown[] | null;
    return Array.isArray(x) && Array.isArray(y) && x[0] === y[0] && x[1] === y[1];
  } catch {
    return false;
  }
}

/** What a preview shows while the next is on its way (`placeholderData` of its query): the last answer when it was for the same operation, so
 * typing does not flash, and nothing when the operation changed, so the panel never draws one operation's body under another's name. */
export const keepSameOperation = (next: string | undefined) => (previous: Preview | undefined, query: { queryKey: readonly unknown[] } | undefined): Preview | undefined => (previous !== undefined && sameOperation(query?.queryKey[1] as string | undefined, next) ? previous : undefined);

/** The preview of `request` (nothing while it is `null`): asked after the input has stopped changing, the last answer kept
 * on screen while the next is on its way (so the panel does not flash), and never retried (a refusal is an answer).
 * `settling` is true from the keystroke until the new answer is here, so the panel can say it is catching up. */
export function usePreview(request: PreviewRequest | null) {
  const key = request ? previewKey(request) : "";
  const pause = useDebounced(key, PREVIEW_PAUSE);
  const asked = pause.value ? (JSON.parse(pause.value) as [string, string, unknown]) : null;
  const q = useQuery<Preview>({
    queryKey: ["preview", pause.value],
    queryFn: ({ signal }) => fetchPreview({ service: asked![0], operation: asked![1], input: asked![2] }, signal),
    enabled: asked !== null,
    placeholderData: keepSameOperation(pause.value),
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
  // The form has moved to another operation and the pause has not ended: the answer in hand is for the one before.
  const elsewhere = asked !== null && request !== null && (asked[0] !== request.service || asked[1] !== request.operation);
  return { ...q, data: elsewhere ? undefined : q.data, settling: !pause.settled || (q.isFetching && q.isPlaceholderData) };
}

// The call a form will make, shown as it is filled in: `POST /api/v1/preview`, asked once the person pauses.
import { keepPreviousData, useQuery } from "@tanstack/react-query";
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

/** The preview of `request` (nothing while it is `null`): asked after the input has stopped changing, the last answer kept
 * on screen while the next is on its way (so the panel does not flash), and never retried (a refusal is an answer).
 * `settling` is true from the keystroke until the new answer is here, so the panel can say it is catching up. */
export function usePreview(request: PreviewRequest | null) {
  const key = request ? previewKey(request) : "";
  const pause = useDebounced(key, PREVIEW_PAUSE);
  const asked = pause.value ? (JSON.parse(pause.value) as [string, string, unknown]) : null;
  const q = useQuery({
    queryKey: ["preview", pause.value],
    queryFn: ({ signal }) => fetchPreview({ service: asked![0], operation: asked![1], input: asked![2] }, signal),
    enabled: asked !== null,
    placeholderData: keepPreviousData,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
  return { ...q, settling: !pause.settled || (q.isFetching && q.isPlaceholderData) };
}

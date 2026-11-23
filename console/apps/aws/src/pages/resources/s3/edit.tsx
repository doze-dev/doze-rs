// The frame of a setting's edit page: a FormPage whose submit is one Put… of the bucket, whose answer sends the person back to the
// tab it was opened from with what was done said there, and whose refusal is shown in place and on the call panel.
import { useState } from "react";
import { Alert, errorWords } from "@doze/ui/components/app/alert";
import { ApiFailure } from "@doze/ui/lib/api";
import { FormPage, type PreviewRequest } from "../../../kit";
import { crumbs, useBucketNav, usePut } from "./api";
import { setFlash } from "./flash";
import { Reading } from "./parts";

const TAB_LABEL: Record<string, string> = { objects: "Objects", properties: "Properties", permissions: "Permissions", management: "Management" };

export type EditProps = { name: string; tab: "properties" | "permissions" | "management"; title: string; sub: React.ReactNode; operation: string; input: object | null; saved: string; returned?: string; children: React.ReactNode; submitLabel?: string; disabled?: boolean; disabledWhy?: React.ReactNode; note?: React.ReactNode; aside?: React.ReactNode; /** The call panel full width under the form (the boards do so for the forms with a long call). */ callBelow?: boolean; /** Saves in place of the Put… (tags are written through the console's tags address). */ save?: () => Promise<unknown> };

/** One setting's edit page. `input` is the whole input of the Put (`null` while it cannot be sent); `saved` is what the tab says after. */
export function SettingForm({ name, tab, title, sub, operation, input, saved, returned, children, submitLabel = "Save changes", disabled, disabledWhy, note, aside, callBelow, save }: EditProps) {
  const go = useBucketNav(name);
  const put = usePut(name, operation);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const back = () => go({ tab: tab === "properties" ? "properties" : tab });
  const failed = error ? errorWords(error) : null;
  const request: PreviewRequest | null = input ? { service: "s3", operation, input: { Bucket: name, ...input } } : null;
  return (
    <FormPage
      title={title}
      sub={sub}
      crumbs={[...crumbs(name), { label: TAB_LABEL[tab]!, to: `/resources/s3/buckets/${encodeURIComponent(name)}` }, { label: title, to: `/resources/s3/buckets/${encodeURIComponent(name)}` }]}
      request={request}
      showInput
      refusal={failed ? { code: error instanceof ApiFailure ? error.code : "Error", message: failed.message } : null}
      callNote={note}
      aside={aside}
      callBelow={callBelow}
      banner={failed ? <Alert tone="bad" title="Not saved." testId="edit-refused">{failed.code?.split(" · ")[0]}: {failed.message}</Alert> : undefined}
      submitLabel={submitLabel}
      pending={put.isPending || busy}
      disabled={disabled || !input}
      disabledWhy={disabledWhy}
      onCancel={back}
      onSubmit={() => {
        if (!input) return;
        setError(null);
        const done = () => (setFlash({ title: saved, returned: returned ?? `${operation} answered.` }), back());
        if (save) {
          setBusy(true);
          void save().then(done, setError).finally(() => setBusy(false));
        } else put.mutate({ ...input }, { onSuccess: done, onError: setError });
      }}
    >
      {children}
    </FormPage>
  );
}

/** Children of a form that needs what the bucket has now: the form is drawn once the read is here, so its first state is that. */
export function Loaded<T>({ q, what, children }: { q: { data: T | undefined; isPending: boolean; failed?: unknown; error?: unknown; refetch: () => unknown; isFetching: boolean; absent?: boolean }; what: string; children: (data: T | undefined) => React.ReactNode }) {
  return (
    <Reading q={q} what={what} height="h-64">
      {children(q.data)}
    </Reading>
  );
}

import type { CallRecord } from "../../api/types/CallRecord";

/** What does the job of an operation doze-aws does not build, for the ones people reach for. The nearest thing is named and not
 * pretended to be the same. */
const INSTEAD: Record<string, string> = {
  SelectObjectContent: "GetObject returns the whole object; the filtering S3 Select does is not built, so read it and filter in your code.",
  GetSdk: "Generate the client from the API's exported definition: GetExport answers with it.",
  CreateDomainNameAccessAssociation: "A custom domain name can be created and mapped; access associations need VPC endpoints, which doze-aws does not have.",
};

export const insteadOf = (action: string | null): string | null => (action ? (INSTEAD[action] ?? null) : null);

/** What happened and what works instead, for a call to an operation that is not built: the call is refused fast, with the nearest
 * thing that works, not answered with an empty result that would look like "no matches". */
export function NotBuilt({ call }: { call: CallRecord }) {
  const instead = insteadOf(call.action);
  return (
    <section className="mt-5 rounded-xl border bg-card p-4" aria-label="What happened" data-testid="not-built">
      <h3 className="text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">What happened</h3>
      <p className="mt-1.5 text-[13.5px]">
        {call.error_code === "OptInRequired" ? (
          <>doze-aws was started without this service, so it answers every call to it with how to turn it on.</>
        ) : (
          <>
            <span className="font-mono">{call.action ?? "This operation"}</span> is not built in doze-aws, so the call was refused with HTTP {call.status} instead of returning an empty answer that would look like a result.
          </>
        )}
      </p>
      <div className="mt-2 flex flex-wrap gap-2 font-mono text-[11.5px]">
        <span className="rounded-md border bg-muted px-2 py-0.5">X-Doze-Not-Built: 1</span>
        <span className="rounded-md border bg-muted px-2 py-0.5">
          HTTP {call.status} {call.error_code ?? ""}
        </span>
      </div>
      {instead && (
        <div className="mt-4" data-testid="not-built-instead">
          <h3 className="text-[11.5px] font-medium tracking-wide text-muted-foreground uppercase">What works instead</h3>
          <p className="mt-1.5 text-[13.5px]">{instead}</p>
        </div>
      )}
    </section>
  );
}

// What a resource's page says when its read failed, for every kind of every service: a name that is not there gets one page (a
// title, the breadcrumb, a way back to the list, the service's own words and code), anything else the error block with Try again.
import { Link } from "@tanstack/react-router";
import { SearchXIcon } from "lucide-react";
import { ErrorBlock, errorWords } from "@doze/ui/components/app/alert";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";
import { ApiFailure } from "@doze/ui/lib/api";
import { useCatalogue } from "../api/resources";
import { withCurrent } from "./crumbs";

/** Said when the console could not reach doze-aws, or it failed: the service did not answer, so "is it running" is the advice. */
export const HINT = "Check that doze-aws is still running, then try again. Nothing was changed.";

/** Whether the service answered that the thing is not there (the codes the services use for it: `ResourceNotFoundException`,
 * `NonExistentQueue`, `NoSuchBucket`, `NotFound`, `does not exist`), as against a read that failed. */
export function isMissing(error: unknown): boolean {
  if (!(error instanceof ApiFailure) || error.status <= 0 || error.status >= 500) return false;
  return error.status === 404 || /NotFound|DoesNotExist|NonExistent|NoSuch|NOT_FOUND/i.test(error.code);
}

const sentence = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** The page of a resource that could not be read. `service` and `kind` are the ids of the address (`dynamodb`, `tables`); the words
 * (`table`, `Tables`, `DynamoDB`) come from the catalogue. `header: false` when the page has drawn its own header already. */
export function DetailError({ service, kind, name, what, error, onRetry, retrying = false, header = true, testId = "detail-error", noun, back }: { /** What the thing is called when the catalogue has no kind for it (`change set`). */ noun?: string; /** Where the way back goes and what it says, when it is not the kind's list (`{ to: "/resources/cloudformation/stacks/x", label: "Back to the stack" }`). */ back?: { to: string; label: string }; service: string; kind: string; name: string; /** `This table could not be read`, for an error that is not a missing name. */ what?: string; error: unknown; onRetry?: () => void; retrying?: boolean; header?: boolean; testId?: string }) {
  const catalogue = useCatalogue().data;
  const s = catalogue?.services.find((x) => x.name === service);
  const k = s?.kinds.find((x) => x.id === kind);
  const one = noun ?? k?.one ?? kind.replace(/s$/, "");
  if (!isMissing(error)) return <ErrorBlock what={what ?? `This ${one} could not be read`} error={error} hint={HINT} onRetry={onRetry} retrying={retrying} testId={testId} />;
  const { message, code } = errorWords(error);
  const list = `/resources/${service}/${kind}`;
  const crumbs = [{ label: "Resources", to: "/resources" }, ...(s ? [{ label: s.group, to: "/resources" }, { label: s.title, to: `/resources/${service}` }] : []), { label: k?.title ?? sentence(kind), to: list }];
  return (
    <>
      {header && <PageHeader title={`${sentence(one)} not found`} crumbs={withCurrent(crumbs, name)} />}
      <div role="alert" data-testid={testId} data-missing="">
        <EmptyState icon={SearchXIcon} title={`There is no ${one} named ${name}`} action={<Button asChild variant="outline" className="bg-card"><Link to={back?.to ?? list}>{back?.label ?? `Back to ${(k?.title ?? sentence(kind)).toLowerCase()}`}</Link></Button>}>
          <p data-slot="error-message">{message}</p>
          <p className="mt-1">There is no {one} of that name in this account and Region. Choose one from the list, or create it.</p>
          {code && <p className="mt-2 font-mono text-[12px]" data-slot="error-code">{code}</p>}
        </EmptyState>
      </div>
    </>
  );
}

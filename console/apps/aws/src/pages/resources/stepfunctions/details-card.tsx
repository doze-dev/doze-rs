// A state machine's Details card (stepfunctions__machine-detail): its ARN and role on a line each, then type, creation date, status and
// revision, and what the logging and tracing settings are.
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { LocalStatus } from "../../../kit";
import { ResourceTo } from "../value";
import { at } from "../deep/parts";

/** The value of the field `label` of a detail, when it has one. */
export const fieldOf = (d: ResourceDetail, label: string): string | null => d.fields.find((f) => f.label === label)?.value ?? null;

const WORDS: Record<string, string> = { STANDARD: "Standard", EXPRESS: "Express", ACTIVE: "Active", DELETING: "Deleting" };

function Row({ label, children, testId }: { label: string; children: React.ReactNode; testId: string }) {
  return (
    <div className="contents">
      <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{label}</dt>
      <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={testId}>{children}</dd>
    </div>
  );
}

export function DetailsCard({ d }: { d: ResourceDetail }) {
  const f = (label: string) => fieldOf(d, label);
  const role = d.fields.find((x) => x.label === "IAM role ARN");
  const level = f("Logging") ?? "OFF";
  const tracing = f("Tracing") === "Enabled";
  const wide = "grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] content-start [&>div:last-child>*]:border-b-0";
  return (
    <section className="mb-10 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7" aria-label="Details" data-testid="details">
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">Details</h2>
      <dl className={`${wide} mt-4 [&>div:last-child>*]:border-b`}>
        <Row label="ARN" testId="field-ARN"><span className="font-mono text-[13px]">{f("ARN") ?? d.id}</span></Row>
        <Row label="IAM role ARN" testId="field-role">
          {role?.value ? (role.link ? <ResourceTo to={role.link}><span className="font-mono text-[13px]">{role.value}</span></ResourceTo> : <span className="font-mono text-[13px]">{role.value}</span>) : "–"}
        </Row>
      </dl>
      <div className="mt-2 grid lg:grid-cols-2 lg:gap-x-12">
        <dl className={wide}>
          <Row label="Type" testId="field-Type">{WORDS[f("Type") ?? ""] ?? f("Type") ?? "–"}</Row>
          <Row label="Status" testId="field-Status">{WORDS[f("Status") ?? ""] ?? f("Status") ?? "–"}</Row>
          <Row label="Logging" testId="field-Logging">
            {level === "OFF" ? "Off" : <>{level}{f("Include execution data") === "Yes" ? ", with execution data" : ""}</>}
            {f("Log group") && <span className="block truncate font-mono text-[12.5px] text-muted-foreground" title={f("Log group") ?? undefined}>{f("Log group")}</span>}
          </Row>
        </dl>
        <dl className={wide}>
          <Row label="Creation date" testId="field-Creation date">{at(f("Creation date"))}</Row>
          <Row label="Revision ID" testId="field-Revision ID"><span className="font-mono text-[13px]">{f("Revision ID") ?? "–"}</span></Row>
          <Row label="X-Ray tracing" testId="field-Tracing">
            {tracing ? "Enabled" : "Disabled"} {tracing && <LocalStatus state="stored" className="ml-1" />}
          </Row>
        </dl>
      </div>
      {f("Description") && <p className="mt-3 text-[13.5px] text-muted-foreground" data-testid="field-Description">{f("Description")}</p>}
    </section>
  );
}

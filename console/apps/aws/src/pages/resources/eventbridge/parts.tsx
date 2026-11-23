// What the EventBridge pages share: a sentence with its names in bold, the details card, a section heading, a state badge and the times.
import { Fragment } from "react";
import { Emphasis } from "@doze/ui/components/app/emphasis";
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { dateTimeUtc } from "@doze/ui/lib/format";
import { cn } from "@doze/ui/lib/utils";
import { useAbout } from "../../../api/queries";

/** The Region and account the console is for (what an ARN is made of). */
export function useWhere(): { region: string; account: string } {
  const about = useAbout().data;
  return { region: about?.region ?? "us-east-1", account: about?.account ?? "000000000000" };
}

/** A time EventBridge answers (seconds since 1970) as the console writes times; a dash when there is none. */
export const when = (seconds: number | undefined): string => (seconds === undefined ? "–" : dateTimeUtc(seconds * 1000));

/** The line under a title: names (monospace, bold) and figures (bold) set apart. */
export function Said({ text, names = [], bold, className, testId = "list-sentence" }: { text: string; names?: readonly string[]; bold?: readonly (string | RegExp)[]; className?: string; testId?: string }) {
  if (text === "") return null;
  return (
    <p className={cn("mb-6 max-w-[62rem] text-[15px] leading-6 text-muted-foreground", className)} data-testid={testId}>
      <Emphasis text={text} names={names} bold={bold} />
    </p>
  );
}

/** The state of an archive, a replay, a rule or a destination, as a dot and a word. */
const TONES: Record<string, "ok" | "info" | "warn" | "bad" | "idle"> = { ENABLED: "ok", ACTIVE: "ok", AUTHORIZED: "ok", COMPLETED: "ok", RUNNING: "info", STARTING: "info", CREATING: "info", UPDATING: "info", CANCELLING: "warn", CANCELLED: "idle", DISABLED: "idle", INACTIVE: "idle", FAILED: "bad", DEAUTHORIZED: "bad" };
export const stateWords = (s: string | undefined): string => (s ? s.charAt(0) + s.slice(1).toLowerCase() : "–");
export function State({ state }: { state: string | undefined }) {
  return state ? <StatusDot tone={TONES[state] ?? "idle"}>{stateWords(state)}</StatusDot> : <span className="text-muted-foreground">–</span>;
}

/** A section's heading and its one line. */
export function Heading({ title, children, id }: { title: string; children?: React.ReactNode; id?: string }) {
  return (
    <div className="mb-4">
      <h2 id={id} className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
      {children && <p className="mt-1 text-[13.5px] text-muted-foreground">{children}</p>}
    </div>
  );
}

/** Rows of label and value, in a card (`columns`: how many side by side). */
export function DetailsGrid({ rows, columns = 1, testId = "details" }: { rows: readonly (readonly [string, React.ReactNode])[]; columns?: 1 | 2; testId?: string }) {
  const per = Math.ceil(rows.length / columns);
  const parts = columns === 2 ? [rows.slice(0, per), rows.slice(per)] : [rows];
  return (
    <section className="rounded-[14px] border bg-card px-6 py-3 lg:px-7" data-testid={testId}>
      <div className={cn("grid gap-x-12", columns === 2 && "lg:grid-cols-2")}>
        {parts.map((p, i) => (
          <dl key={i} className="grid grid-cols-[minmax(7rem,10rem)_minmax(0,1fr)] [&>div:last-child>*]:border-b-0">
            {p.map(([l, v]) => (
              <div key={l} className="contents">
                <dt className="border-b py-3 pr-4 text-[13.5px] text-muted-foreground">{l}</dt>
                <dd className="min-w-0 border-b py-3 text-[13.5px] break-words text-bright" data-testid={`field-${l}`}>{v}</dd>
              </div>
            ))}
          </dl>
        ))}
      </div>
    </section>
  );
}

/** An address or an ARN a cell may break after its `:`, `/` and `.`, so it wraps at a part and is shown whole (it is copied whole: the break is a `<wbr>`). */
export function Breakable({ text }: { text: string }) {
  return (
    <>
      {text.split(/(?<=[:/.])/).map((part, i) => (
        <Fragment key={i}>
          {i > 0 && <wbr />}
          {part}
        </Fragment>
      ))}
    </>
  );
}

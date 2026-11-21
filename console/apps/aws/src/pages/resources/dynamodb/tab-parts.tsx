// Small pieces of a table's tabs: a list of facts (a label and its value, a row each) and a state word with its dot.
import { StatusDot } from "@doze/ui/components/app/status-dot";
import { cn } from "@doze/ui/lib/utils";

/** A label that is one of the service's own member names (`TableStatus`, `LatestStreamArn`): CamelCase in one word. */
const isMember = (label: string): boolean => /^[A-Z][a-z]+(?:[A-Z][A-Za-z]*)+$/.test(label);

/** Labels and values, a row each, ruled apart. A label that is a member name of the service is set in monospace, as the boards do. A
 * value is a node, so a second line (`Stored, not applied locally.`) goes under it. */
export function Facts({ rows, className, testId }: { rows: [string, React.ReactNode][]; className?: string; testId?: string }) {
  return (
    <dl className={cn("grid grid-cols-[minmax(9rem,13rem)_minmax(0,1fr)] content-start text-[14px] [&>div:last-child>*]:border-b-0", className)} data-testid={testId}>
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className={cn("border-b py-3.5 pr-4 text-muted-foreground", isMember(k) && "font-mono text-[13px]")}>{k}</dt>
          <dd className="min-w-0 border-b py-3.5 break-words text-bright" data-testid={`fact-${k}`}>
            {v ?? <span className="text-muted-foreground">–</span>}
          </dd>
        </div>
      ))}
    </dl>
  );
}

/** A setting's state: on is green, off is a hollow dot. */
export const OnOff = ({ on, yes = "On", no = "Off" }: { on: boolean; yes?: string; no?: string }) => <StatusDot tone={on ? "ok" : "idle"}>{on ? yes : no}</StatusDot>;

/** A value with a second, dimmer line under it. */
export const WithSub = ({ children, sub }: { children: React.ReactNode; sub: React.ReactNode }) => (
  <span className="grid gap-0.5">
    <span>{children}</span>
    <span className="text-[12.5px] font-normal text-muted-foreground">{sub}</span>
  </span>
);

export const Dash = <span className="text-muted-foreground">–</span>;

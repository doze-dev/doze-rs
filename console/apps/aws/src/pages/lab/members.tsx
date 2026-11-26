import { number } from "@doze/ui/lib/format";
import type { LabMember } from "../../api/types/LabMember";

/** What a kind of member needs said to write it by hand: a blob is base64 text, a stream is not carried at all. */
export function kindHint(kind: string): string | null {
  if (kind === "blob") return "base64 text";
  if (kind === "stream") return "not carried by the Lab";
  if (kind === "timestamp") return "epoch seconds";
  return null;
}

/** The members of an operation's input, from AWS's model: name, kind, whether it is required, and the values the model lists. */
export function MemberList({ members }: { members: LabMember[] }) {
  const required = members.filter((m) => m.required).length;
  return (
    <details className="text-[13px]" data-testid="lab-members">
      <summary className="cursor-pointer text-muted-foreground">
        {number(members.length)} input members in the model{required > 0 ? `, ${number(required)} required` : ""}
      </summary>
      <ul className="mt-2 grid gap-1.5">
        {members.map((m) => {
          const hint = kindHint(m.kind);
          return (
            <li key={m.name} className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
              <span className="font-mono text-[12.5px] text-foreground">{m.name}</span>
              <span className="text-[12px] text-muted-foreground">{m.kind}{hint ? ` (${hint})` : ""}</span>
              {m.required && <span className="text-[12px] font-medium text-bright">required</span>}
              {m.values.length > 0 && (
                <span className="min-w-0 text-[12px] break-words text-muted-foreground">
                  one of {m.values.slice(0, 8).join(", ")}
                  {m.values.length > 8 ? `, and ${m.values.length - 8} more` : ""}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </details>
  );
}

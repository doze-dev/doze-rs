// Delete a user, group, role or policy (iam__user-delete, iam__role-delete): a typed confirm that says what goes with it, by name, from
// what IAM holds of it, and what stays. The console clears what IAM refuses the delete over (DeleteConflict), then deletes.
import { Alert } from "@doze/ui/components/app/alert";
import { Callout } from "@doze/ui/components/app/callout";
import { TypedConfirm } from "@doze/ui/components/app/typed-confirm";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { ApiFailure } from "@doze/ui/lib/api";
import { type Blocker, useBlockers, useDeleteClearing } from "../../../kit";
import { list } from "./model";

export type DeletedKind = "users" | "groups" | "roles" | "policies";

const OPERATION: Record<DeletedKind, string> = { users: "DeleteUser", groups: "DeleteGroup", roles: "DeleteRole", policies: "DeletePolicy" };
const NOUN: Record<DeletedKind, string> = { users: "user", groups: "group", roles: "role", policies: "policy" };

const n = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;

/** What the delete does with one kind of dependent, in words: how many, which, and what happens to them. */
export function groupWords(name: string, b: Blocker): { lead: string; rest: string } {
  const names = b.items.map((i) => i.name);
  const count = b.items.length;
  const who = list(names) + (b.more ? " and more" : "");
  switch (b.title) {
    case "Attached policies":
      return { lead: `${n(count, "policy", "policies")} will be detached.`, rest: `${who} ${count === 1 ? "is" : "are"} attached to ${name}; the ${count === 1 ? "policy itself stays" : "policies themselves stay"}.` };
    case "Inline policies":
      return { lead: `${n(count, "inline policy", "inline policies")} will be deleted with it.`, rest: `${who} ${count === 1 ? "is" : "are"} embedded in ${name} and go${count === 1 ? "es" : ""} with it.` };
    case "Access keys":
      return { lead: `${n(count, "access key")} will be deleted.`, rest: `${who} stop${count === 1 ? "s" : ""} signing requests as ${name} at once.` };
    case "Groups":
      return { lead: `${name} will be removed from ${n(count, "group")}.`, rest: `${who}: the ${count === 1 ? "group stays" : "groups stay"}.` };
    case "Members":
      return { lead: `${n(count, "member")} will be removed from the group.`, rest: `${who}: the ${count === 1 ? "user stays" : "users stay"}.` };
    case "Instance profiles":
      return { lead: `${name} will be removed from ${n(count, "instance profile")}.`, rest: `${who}.` };
    case "Roles":
    case "Users":
      return { lead: `It will be detached from ${n(count, b.title === "Roles" ? "role" : "user")}.`, rest: `${who}: ${count === 1 ? "it stays" : "they stay"}.` };
    case "Other versions":
      return { lead: `${n(count, "other version")} will be deleted.`, rest: `${who} ${count === 1 ? "is" : "are"} not the default; the default version stays.` };
    default:
      return { lead: `${n(count, b.title.toLowerCase())} ${count === 1 ? "is" : "are"} cleared with it.`, rest: `${who}.` };
  }
}

/** What the console does that the service's own delete does not: the board's note, in what doze-aws really does. */
const HOW: Record<DeletedKind, string> = {
  users: "The console also removes the user's group memberships and inline policies; managed policies attached to the user are detached, not deleted. DeleteUser answers DeleteConflict while the user still has an access key, a group, an inline policy or an attached managed policy, so from the CLI or an API you do each of those steps first.",
  roles: "The console detaches a role's managed policies, deletes its inline policies and removes it from its instance profiles, then deletes the role. From the CLI or an API you must do each of those steps yourself first: DeleteRole answers DeleteConflict while a managed policy is still attached.",
  groups: "The console removes the group's members and detaches its policies, then deletes the group. DeleteGroup answers DeleteConflict while the group has a member or a policy.",
  policies: "The console detaches the policy from everything it is attached to and deletes its other versions, then deletes the policy. DeletePolicy answers DeleteConflict while it is attached or has versions besides its default.",
};

const WARNING: Record<DeletedKind, (name: string) => string> = {
  users: (name) => `This permanently deletes ${name}. Once an IAM user and its access keys are deleted, they cannot be restored or recovered.`,
  roles: (name) => `This deletes ${name}. Sessions already taken of it keep working until they expire.`,
  groups: (name) => `This deletes ${name}. Its members keep their other permissions.`,
  policies: (name) => `This deletes ${name} and all its versions. It cannot be recovered.`,
};

/** The dialog. `onDeleted` is called once the delete is done (the page goes to the list). */
export function DeleteEntity({ kind, name, label, open, onOpenChange, onDeleted }: { kind: DeletedKind; name: string; /** What is shown and typed when it is not the name (a policy's ARN is its name; its label is the name). */ label?: string; open: boolean; onOpenChange: (open: boolean) => void; onDeleted: () => void }) {
  const target = { service: "iam", kind, name };
  const blockers = useBlockers(target, open);
  const del = useDeleteClearing(target);
  const shown = label ?? name;
  const refused = del.error;
  const code = refused instanceof ApiFailure ? refused.code : "";
  const clear = blockers.data?.blockers.some((b) => b.clears) ?? false;
  const none = blockers.data !== undefined && blockers.data.blockers.length === 0;
  return (
    <TypedConfirm
      open={open}
      onOpenChange={(o) => {
        if (!o) del.reset();
        onOpenChange(o);
      }}
      title={`Delete ${NOUN[kind]}`}
      description={<span data-testid="delete-warning">{WARNING[kind](shown)}</span>}
      words={shown}
      noun={`${NOUN[kind]} name`}
      verb="Delete"
      pending={del.isPending}
      onConfirm={() => del.mutate({ clear }, { onSuccess: onDeleted })}
      testId="delete-dialog"
      refusal={refused ? <Alert tone="bad" title={`${OPERATION[kind]} was refused${code ? `: ${code}` : ""}`} testId="delete-refused">{refused instanceof Error ? refused.message : "No reason was given."}</Alert> : undefined}
    >
      <div className="grid gap-3" data-testid="delete-details">
        {blockers.isPending ? (
          <Skeleton className="h-16 w-full rounded-xl" />
        ) : blockers.isError ? (
          <Alert tone="warn" title="What it holds could not be read.">IAM will refuse the delete if something is still attached; its answer is shown here.</Alert>
        ) : none && kind === "users" ? (
          <Callout tone="note" lead="No access keys." testId="delete-keys">{shown} has none, so nothing signs requests as this user.</Callout>
        ) : (
          blockers.data?.blockers.map((b) => {
            const w = groupWords(shown, b);
            return <Callout key={b.title} tone="warn" lead={w.lead} testId={`delete-group-${b.title}`}>{w.rest}</Callout>;
          })
        )}
        {kind === "users" && !none && !blockers.data?.blockers.some((b) => b.title === "Access keys") && <Callout tone="note" lead="No access keys." testId="delete-keys">{shown} has none, so nothing signs requests as this user.</Callout>}
        <p className="text-[13.5px] leading-[21px] text-muted-foreground" data-testid="delete-how">{HOW[kind]}</p>
      </div>
    </TypedConfirm>
  );
}

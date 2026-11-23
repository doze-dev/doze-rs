// Create key, step 4: what the IAM policies of each chosen key user say (kms__key-create-users). IAM answers it: the policies
// attached to each user or role, and the document of each customer managed one.
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { Note } from "../cfg-parts";
import { ROLES, USERS, useEach } from "../iam/api";
import type { AttachedPolicy } from "../iam/model";
import { usePolicyDocs } from "../iam/policy-data";
import { principalLine } from "./principal-line";
import type { Principal } from "./create-perms";

type Attached = { AttachedPolicies: AttachedPolicy[] };
type Inline = { PolicyNames: string[] };

/** One line under the table for each chosen user or role. */
export function PrincipalLines({ chosen }: { chosen: readonly Principal[] }) {
  const users = chosen.filter((p) => p.name.startsWith("user/"));
  const roles = chosen.filter((p) => p.name.startsWith("role/"));
  const userAttached = useEach<Attached>(USERS, "ListAttachedUserPolicies", users.map((p) => ({ UserName: p.name.slice(5) })));
  const userInline = useEach<Inline>(USERS, "ListUserPolicies", users.map((p) => ({ UserName: p.name.slice(5) })));
  const roleAttached = useEach<Attached>(ROLES, "ListAttachedRolePolicies", roles.map((p) => ({ RoleName: p.name.slice(5) })));
  const roleInline = useEach<Inline>(ROLES, "ListRolePolicies", roles.map((p) => ({ RoleName: p.name.slice(5) })));
  const rows = [
    ...users.map((p, i) => ({ p, attached: userAttached[i], inline: userInline[i] })),
    ...roles.map((p, i) => ({ p, attached: roleAttached[i], inline: roleInline[i] })),
  ];
  const arns = rows.flatMap((r) => (r.attached?.data?.AttachedPolicies ?? []).map((a) => a.PolicyArn));
  const docs = usePolicyDocs(arns);
  if (chosen.length === 0) return null;
  return (
    <div className="grid gap-2" data-testid="principal-lines">
      {rows.map(({ p, attached, inline }) =>
        attached?.data && inline?.data ? (
          <Note key={p.arn} className="text-foreground" testId={`principal-line-${p.name}`}>
            {principalLine({ name: p.name.slice(5), attached: attached.data.AttachedPolicies, inline: inline.data.PolicyNames, docs })}
          </Note>
        ) : (
          <Skeleton key={p.arn} className="h-5 w-2/3" />
        ),
      )}
    </div>
  );
}

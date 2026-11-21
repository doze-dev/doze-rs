// Create user (iam__user-create, user-create-permissions, user-create-review): three steps on one page. The name, the permissions (a
// group, another user's, or policies), then the review with the tags. Nothing is made until the last step: CreateUser, then one
// AddUserToGroup per group and one AttachUserPolicy per policy.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { ReviewSection } from "@doze/ui/components/app/stepper";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { useAbout } from "../../../api/queries";
import { useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, PolicyPicker, ResourcePicker, TAG_RULES, callKindOp } from "../../../kit";
import { USERS, useRead, useRefresh } from "./api";
import { list, sameIgnoringCase } from "./model";
import { CardChoice, crumbs, KINDS, Note } from "./parts";
import { PolicyChooser } from "./policy-chooser";
import { type UserForm, blankUser, permissionsOf, stepProblem, userCalls } from "./user-create-model";
import { useGroupPolicies } from "./user-data";

const STEPS = [{ id: "details", label: "Specify user details" }, { id: "permissions", label: "Set permissions" }, { id: "review", label: "Review and create" }];
const TITLES = ["Specify user details", "Set permissions", "Review and create"];
const SUBS = [
  "Name the user. You give them permissions on the next step.",
  "Add the user to a group, copy another user's permissions, or attach policies. A new user has no permissions until you do.",
  "Check the choices from the earlier steps. The user is created when you choose Create user.",
];

export function UserCreate() {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const mode = useAbout().data?.iam ?? "soft";
  const [f, setF] = useState<UserForm>(blankUser);
  const [step, setStep] = useState(0);
  const [furthest, setFurthest] = useState(0);
  const [tried, setTried] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string } | null>(null);
  const users = useResourceList("iam", "users");
  const groups = useResourceList("iam", "groups");
  const taken = useMemo(() => users.data?.pages.flatMap((p) => p.rows).map((r) => r.name) ?? [], [users.data]);
  const groupNames = useMemo(() => groups.data?.pages.flatMap((p) => p.rows).map((r) => r.name) ?? [], [groups.data]);
  const { byGroup } = useGroupPolicies(groupNames);
  // Copy permissions: what the user copied from belongs to and has attached.
  const from = f.copyFrom ?? "";
  const theirGroups = useRead<{ Groups: { GroupName: string }[] }>(USERS, "ListGroupsForUser", { UserName: from }, from !== "");
  const theirPolicies = useRead<{ AttachedPolicies: { PolicyArn: string }[] }>(USERS, "ListAttachedUserPolicies", { UserName: from }, from !== "");
  const copied = useMemo(() => ({ groups: (theirGroups.data?.Groups ?? []).map((g) => g.GroupName), policies: (theirPolicies.data?.AttachedPolicies ?? []).map((p) => p.PolicyArn) }), [theirGroups.data, theirPolicies.data]);
  const set = (patch: Partial<UserForm>) => {
    setF((x) => ({ ...x, ...patch }));
    setFailed(null);
  };
  const problem = stepProblem(f, 0, taken);
  const go = (to: number) => {
    setStep(to);
    setFurthest((x) => Math.max(x, to));
  };
  const next = () => {
    if (step === 0) {
      setTried(true);
      if (problem) return;
    }
    go(step + 1);
  };
  const perms = permissionsOf(f, copied);
  const all = userCalls(f, copied);
  const shown = step === 0 ? all.slice(0, 1) : step === 1 ? userCalls(f, copied, false).slice(0, 2) : all;
  const inherited = perms.groups.flatMap((g) => byGroup.get(g) ?? []).map((p) => p.PolicyName);
  const submit = async () => {
    setPending(true);
    setFailed(null);
    let at = "";
    try {
      for (const c of all) {
        at = c.operation;
        await callKindOp(USERS, c.operation, c.input as object);
      }
      refresh();
      void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "users" }, search: { do: "created", name: f.name } });
    } catch (e) {
      const code = e instanceof ApiFailure ? e.code : "Error";
      setFailed({ operation: at, code, message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  const groupColumns: DataColumn<{ name: string }>[] = [
    { id: "name", header: "Group name", size: 320, accessorFn: (r) => r.name, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> },
    { id: "policies", header: "Attached policies", accessorFn: (r) => (byGroup.get(r.name) ?? []).map((p) => p.PolicyName).join(", "), cell: (c) => { const p = byGroup.get(c.row.original.name) ?? []; return p.length === 0 ? <span className="text-muted-foreground">None</span> : <span className="font-mono text-[13px]">{p.map((x) => x.PolicyName).join(", ")}</span>; } },
  ];
  const similar = sameIgnoringCase(f.name, taken);
  const modeWords = mode === "enforce" ? "A user with no permissions is denied: IAM is enforcing here." : mode === "off" ? "IAM is off here, so permissions are not checked." : "A user with no permissions is denied in enforce mode; in soft mode, the default, doze-aws records the would-be denial and lets the call through.";
  return (
    <FormPage
      title={TITLES[step]!}
      sub={SUBS[step]}
      crumbs={[...crumbs(KINDS.users), { label: "Create user", to: "" }]}
      request={null}
      requests={shown}
      callTitle={step === 0 ? "The call this will make" : step === 1 ? "The call this will add" : "The calls this will make"}
      callNote={step === 0 ? "The call runs when you choose Create user on the last step, not on Next." : step === 1 ? <>{perms.groups.length > 0 ? <>One AddUserToGroup call per selected group, after CreateUser.</> : perms.policies.length > 0 ? <>One AttachUserPolicy call per selected policy, after CreateUser.</> : <>Nothing is added: the user starts with no permissions.</>}{inherited.length > 0 && <> <b className="font-semibold text-foreground">{f.name || "The user"}</b> inherits {list([...new Set(inherited)])} through {list(perms.groups)}.</>}</> : "CreateUser first; then each membership and policy is its own call, in this order."}
      refusal={failed ? { code: `${failed.operation}: ${failed.code}`, message: failed.message } : null}
      wizard={{ steps: STEPS, current: step, furthest, onStep: go, onBack: () => go(step - 1), onNext: next }}
      backLabel="Previous"
      submitLabel="Create user"
      pending={pending}
      onSubmit={() => void submit()}
      onCancel={() => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "users" }, search: {} })}
      banner={failed ? <Alert tone="bad" title={failed.operation === "CreateUser" ? "The user was not created." : `${f.name} was created, but ${failed.operation} was refused.`} testId="create-refused">{failed.code}: {failed.message}</Alert> : undefined}
      testId="user-create"
    >
      {step === 0 && (
        <FormSection title="User details" testId="section-details">
          <Field label="User name" error={tried ? (problem ?? undefined) : undefined} help={<><b>Up to 64 characters</b>: letters, digits and the characters + = , . @ _ -. Names are unique in the account. AWS treats names that differ only in case, such as TESTUSER and testuser, as the same name; doze-aws compares names exactly, so it would create both.{taken.length > 0 && <> <span className="font-mono">{list(taken.slice(0, 5))}{taken.length > 5 ? ` and ${taken.length - 5} more` : ""}</span> already {taken.length === 1 ? "exists" : "exist"}.</>}</>} hint={similar.length > 0 ? <>{f.name} differs only in case from <span className="font-mono">{list(similar)}</span>: AWS would refuse it as a duplicate.</> : undefined} testId="field-name">
            {(id) => <Input id={id} value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="dev-intern" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="user-name" />}
          </Field>
          <div className="flex items-start gap-3 opacity-70" data-testid="console-access">
            <Checkbox disabled aria-label="Provide user access to the AWS Management Console" className="mt-0.5" />
            <div className="grid gap-0.5 text-[14px] leading-5">
              <span className="font-medium text-bright">Provide user access to the AWS Management Console <span className="font-normal text-muted-foreground">optional</span></span>
              <span className="text-[13px] text-muted-foreground">Not available in doze-aws: there is no console sign-in locally. A user here works through access keys.</span>
            </div>
          </div>
        </FormSection>
      )}
      {step === 1 && (
        <>
          <FormSection title="Permissions options" testId="section-options">
            <CardChoice name="how" value={f.how} onChange={(how) => set({ how })} options={[{ value: "group", title: "Add user to group", description: "Recommended: manage permissions through the groups." }, { value: "copy", title: "Copy permissions", description: "Copy the groups and policies of an existing user." }, { value: "policies", title: "Attach policies directly", description: "Attach managed policies to this one user." }]} />
            <p className="max-w-[48rem] text-[13.5px] leading-[21px] text-muted-foreground" data-testid="mode-words">Put users in groups and attach policies to the groups; a change to a group applies to every member. {modeWords}</p>
          </FormSection>
          {f.how === "group" && (
            <FormSection title={`User groups (${groupNames.length})`} description="Select the groups the user joins. The list shows each group's name and the policies the user receives from it." testId="section-groups">
              {groupNames.length === 0 ? <p className="text-[13.5px] text-muted-foreground" data-testid="no-groups">There are no groups yet: create one from User groups, or choose another option.</p> : (
                <SelectableTable aria-label="User groups" data={groupNames.map((name) => ({ name }))} columns={groupColumns} rowId={(r) => r.name} selected={f.groups} onSelectedChange={(groups) => set({ groups })} noun="group" selectedLabel={(c) => `${c} selected`} hint="Select one or more groups." maxHeight={320} />
              )}
            </FormSection>
          )}
          {f.how === "copy" && (
            <FormSection title="Copy permissions" description="Choose the user whose groups and policies the new user gets." testId="section-copy">
              <Field label="User to copy from">
                {(id) => (
                  <ResourcePicker service="iam" kind="users" noun="user" by="name" id={id} value={f.copyFrom} onChange={(copyFrom) => set({ copyFrom })} className="max-w-sm" />
                )}
              </Field>
              {f.copyFrom && <Note testId="copy-words"><b>{f.copyFrom}</b> belongs to {copied.groups.length === 0 ? "no group" : list(copied.groups)} and has {copied.policies.length === 0 ? "no policy attached directly" : `${list(copied.policies.map((a) => a.split("/").pop() ?? a))} attached`}; the new user gets the same.</Note>}
            </FormSection>
          )}
          {f.how === "policies" && (
            <FormSection title="Permissions policies" description="Select the policies attached to this one user." testId="section-policies">
              <PolicyChooser value={f.policies} onChange={(policies) => set({ policies })} />
            </FormSection>
          )}
          <FormSection title="Set permissions boundary" optional collapsible defaultOpen={false} summary="An advanced feature: open it to use a policy as the most the user can ever do." description="A boundary is an advanced feature: it sets the most the user can ever do, and grants nothing by itself." testId="section-boundary">
            <Field label="Permissions boundary policy">{(id) => <PolicyPicker id={id} value={f.boundary} onChange={(boundary) => set({ boundary })} />}</Field>
          </FormSection>
        </>
      )}
      {step === 2 && (
        <>
          <ReviewSection title="User details" onEdit={() => go(0)} rows={[{ label: "User name", value: <span className="font-mono">{f.name}</span> }, ...(f.boundary ? [{ label: "Permissions boundary", value: <span className="font-mono">{f.boundary}</span> }] : [])]} />
          <section className="rounded-[14px] border bg-card p-6 lg:p-7" aria-label="Permissions" data-testid="review-permissions">
            <div className="flex items-start justify-between gap-3"><h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{perms.groups.length > 0 || perms.policies.length === 0 ? "User group memberships" : "Permissions policies"}</h2><button type="button" className="rounded-sm text-[13px] font-medium text-link hover:underline" onClick={() => go(1)} data-testid="review-edit-Permissions">Edit</button></div>
            <p className="mt-1 text-[13.5px] text-muted-foreground">{perms.groups.length > 0 ? "The user will be added to these user groups." : perms.policies.length > 0 ? "These policies will be attached to the user." : "The user has no permissions until you give it some."}</p>
            {(perms.groups.length > 0 || perms.policies.length > 0) && (
              <ul className="mt-3 divide-y rounded-lg border" data-testid="review-list">
                <li className="bg-muted/50 px-4 py-2 text-[12px] font-medium tracking-[0.04em] text-muted-foreground uppercase">{perms.groups.length > 0 ? "User group" : "Policy"}</li>
                {(perms.groups.length > 0 ? perms.groups : perms.policies.map((a) => a.split("/").pop() ?? a)).map((n) => <li key={n} className="px-4 py-2.5 font-mono text-[13px] text-bright">{n}</li>)}
              </ul>
            )}
          </section>
          <FormSection title="Tags" optional description="Tags are key-value pairs that label the user." testId="section-tags">
            {f.tags.length === 0 && <p className="text-[13.5px] text-muted-foreground">No tags.</p>}
            <KeyValueEditor rows={f.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} testId="create-tags" />
          </FormSection>
        </>
      )}
    </FormPage>
  );
}

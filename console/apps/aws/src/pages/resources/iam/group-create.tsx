// Create user group (iam__group-create): the name, the users who join and the policies that apply to every member, on one page, with the
// calls it makes beside it: CreateGroup, then one AddUserToGroup per user and one AttachGroupPolicy per policy.
import { useNavigate } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { type DataColumn } from "@doze/ui/components/app/data-table";
import { SelectableTable } from "@doze/ui/components/app/selectable-table";
import { Input } from "@doze/ui/components/ui/input";
import { ApiFailure } from "@doze/ui/lib/api";
import { useResourceList } from "../../../api/resources";
import { Field, FormPage, FormSection, type PreviewRequest, callKindOp } from "../../../kit";
import { GROUPS, useRefresh } from "./api";
import { list, nameProblem, takenProblem } from "./model";
import { crumbs, KINDS } from "./parts";
import { PolicyChooser } from "./policy-chooser";

export function GroupCreate() {
  const navigate = useNavigate();
  const refresh = useRefresh();
  const [name, setName] = useState("");
  const [users, setUsers] = useState<ReadonlySet<string>>(new Set());
  const [policies, setPolicies] = useState<ReadonlySet<string>>(new Set());
  const [tried, setTried] = useState(false);
  const [pending, setPending] = useState(false);
  const [failed, setFailed] = useState<{ operation: string; code: string; message: string } | null>(null);
  const groups = useResourceList("iam", "groups");
  const members = useResourceList("iam", "users");
  const taken = useMemo(() => groups.data?.pages.flatMap((p) => p.rows).map((r) => r.name) ?? [], [groups.data]);
  const userNames = useMemo(() => members.data?.pages.flatMap((p) => p.rows).map((r) => r.name) ?? [], [members.data]);
  const problem = nameProblem(name, "group") ?? takenProblem("user group", name, taken);
  const shown = name || "(a name)";
  const requests: PreviewRequest[] = [
    { service: "iam", operation: "CreateGroup", input: { GroupName: shown } },
    ...[...users].map((UserName): PreviewRequest => ({ service: "iam", operation: "AddUserToGroup", input: { UserName, GroupName: shown } })),
    ...[...policies].map((PolicyArn): PreviewRequest => ({ service: "iam", operation: "AttachGroupPolicy", input: { GroupName: shown, PolicyArn } })),
  ];
  const back = () => void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "groups" }, search: {} });
  const submit = async () => {
    setTried(true);
    if (problem) return;
    setPending(true);
    setFailed(null);
    let at = "";
    try {
      for (const c of requests) {
        at = c.operation;
        await callKindOp(GROUPS, c.operation, { ...(c.input as object), GroupName: name });
      }
      refresh();
      void navigate({ to: "/resources/$service/$second", params: { service: "iam", second: "groups" }, search: { do: "created", name } });
    } catch (e) {
      setFailed({ operation: at, code: e instanceof ApiFailure ? e.code : "Error", message: e instanceof Error ? e.message : "No reason was given." });
    } finally {
      setPending(false);
    }
  };
  const userColumns: DataColumn<{ name: string }>[] = [{ id: "name", header: "User name", accessorFn: (r) => r.name, cell: (c) => <span className="font-mono text-[13px] font-medium text-bright">{c.row.original.name}</span> }];
  return (
    <FormPage
      title="Create user group"
      sub={<>Name the group, add its members and attach the policies they share. <span className="text-muted-foreground">Members and policies can also be added later.</span></>}
      crumbs={[...crumbs(KINDS.groups), { label: "Create user group", to: "" }]}
      request={null}
      requests={requests}
      callTitle="The calls this will make"
      callNote="One AddUserToGroup call per selected user, and one AttachGroupPolicy per selected policy, after CreateGroup."
      refusal={failed ? { code: `${failed.operation}: ${failed.code}`, message: failed.message } : null}
      banner={failed ? <Alert tone="bad" title={failed.operation === "CreateGroup" ? "The group was not created." : `${name} was created, but ${failed.operation} was refused.`} testId="create-refused">{failed.code}: {failed.message}</Alert> : undefined}
      submitLabel="Create group"
      pending={pending}
      onSubmit={() => void submit()}
      onCancel={back}
      testId="group-create"
    >
      <FormSection title="Name the group" testId="section-name">
        <Field label="User group name" error={tried ? (problem ?? undefined) : undefined} help={<><b>Up to 128 characters</b>: letters, digits and + = , . @ _ -. Names are unique in the account. Names that differ only in case, such as ADMINS and admins, are the same name: a second one is refused.{taken.length > 0 && <> <span className="font-mono">{list(taken.slice(0, 5))}</span> already {taken.length === 1 ? "exists" : "exist"}.</>}</>} testId="field-name">
          {(id) => <Input id={id} value={name} onChange={(e) => { setName(e.target.value); setFailed(null); }} placeholder="support" autoComplete="off" spellCheck={false} className="bg-card font-mono" data-testid="group-name" />}
        </Field>
      </FormSection>
      <FormSection title="Users" description="Select the users who join the group." testId="section-users">
        {userNames.length === 0 ? <p className="text-[13.5px] text-muted-foreground" data-testid="no-users">There are no users yet: members can be added later.</p> : <SelectableTable aria-label="Users" data={userNames.map((n) => ({ name: n }))} columns={userColumns} rowId={(r) => r.name} selected={users} onSelectedChange={setUsers} noun="user" selectedLabel={(c) => `${c} selected`} hint="Select one or more users." maxHeight={300} />}
      </FormSection>
      <FormSection title="Permissions policies" description="Select the policies that apply to every member of the group." testId="section-policies">
        <PolicyChooser value={policies} onChange={setPolicies} footnote={<p className="max-w-[46rem] text-[13.5px] leading-[21px] text-muted-foreground" data-testid="policy-note">IAM lists a customer managed policy always, and an AWS managed one once something has it attached. The other AWS managed names shown are ones doze-aws makes a document for when they are attached by ARN, so they are offered here without having been listed.</p>} />
      </FormSection>
    </FormPage>
  );
}

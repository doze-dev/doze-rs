// A stack's "Stack info" tab (cloudformation__stack-detail): the overview, its tags, stack policy, rollback configuration and
// notification options. What doze-aws does with each setting is said where it is shown, from the crate: tags go to the resources
// (res/mod.rs `tag_list`), notifications are published (notify.rs), the stack policy is enforced on updates (stackpolicy.rs); a
// rollback configuration is not kept (describe.rs answers an empty one) and the IAM role is kept and returned, never assumed.
import { CodeBlock, pretty } from "@doze/ui/components/app/code-block";
import { Badge } from "@doze/ui/components/ui/badge";
import { Skeleton } from "@doze/ui/components/ui/skeleton";
import { LocalStatus, RecentCalls } from "../../../kit";
import type { ResourceDetail } from "../../../api/types/ResourceDetail";
import { at } from "../deep/parts";
import { ResourceTo } from "../value";
import { useCfnRead } from "./api";
import { type Stack } from "./model";
import { Facts, Status, StackTo } from "./parts";

const card = "min-w-0 rounded-[var(--radius-card)] border bg-card p-6 lg:p-7";

function Block({ title, lede, children, testId }: { title: string; lede: string; children: React.ReactNode; testId: string }) {
  return (
    <section className={card} aria-label={title} data-testid={testId}>
      <h2 className="text-[17px] leading-6 font-semibold tracking-[-0.01em] text-bright">{title}</h2>
      <p className="mt-1 text-[13.5px] text-muted-foreground">{lede}</p>
      <div className="mt-4 grid gap-3 text-[13.5px]">{children}</div>
    </section>
  );
}

const dash = <span className="text-muted-foreground">–</span>;

export function InfoTab({ stack, d }: { stack: Stack; d: ResourceDetail }) {
  const policy = useCfnRead<{ StackPolicyBody?: string }>("GetStackPolicy", { StackName: stack.StackName });
  const tags = stack.Tags ?? [];
  const topics = stack.NotificationARNs ?? [];
  const role = stack.RoleARN;
  return (
    <div className="grid gap-6" data-testid="info-tab">
      <Facts
        title="Overview"
        testId="overview"
        wide={[{ label: "Stack ID", value: stack.StackId, mono: true }]}
        rows={[
          { label: "Stack name", value: stack.StackName, mono: true },
          { label: "Root stack", value: stack.RootId ? <StackTo name={stack.RootId.split("/")[1] ?? stack.RootId} /> : dash },
          { label: "Status", value: <Status status={stack.StackStatus} /> },
          { label: "IAM role", value: role ? <ResourceTo to={{ service: "iam", kind: "roles", name: role.split("/").pop() ?? role }} className="font-mono text-[13px]">{role.split("/").pop()}</ResourceTo> : dash },
          { label: "Creation time", value: at(stack.CreationTime) },
          { label: "Termination protection", value: stack.EnableTerminationProtection ? "Enabled" : "Disabled" },
          ...(stack.LastUpdatedTime ? [{ label: "Last updated", value: at(stack.LastUpdatedTime) }] : []),
          ...(stack.Description ? [{ label: "Description", value: stack.Description }] : []),
        ]}
      >
        {role && <LocalStatus state="stored" className="mt-3" reason="The role is kept and returned by DescribeStacks; CloudFormation here does not assume it, so every resource is made with the caller's own permissions." />}
      </Facts>
      <div className="grid items-start gap-6 lg:grid-cols-2">
        <Block title="Tags" lede="Key-value pairs on the stack." testId="tags-block">
          {tags.length === 0 ? <p className="text-muted-foreground">No tags.</p> : <ul className="flex flex-wrap gap-2">{tags.map((t) => <li key={t.Key}><Badge variant="outline" className="font-mono font-normal">{t.Key}: {t.Value}</Badge></li>)}</ul>}
          <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground"><b className="font-semibold text-foreground">Applied locally.</b> The stack's tags are added to each resource it makes whose type takes tags; a tag the template sets itself wins.</p>
        </Block>
        <Block title="Stack policy" lede="Protects resources from unintended updates." testId="policy-block">
          {policy.isPending ? <Skeleton className="h-10" /> : policy.data?.StackPolicyBody ? <CodeBlock text={pretty(policy.data.StackPolicyBody)} max="max-h-56" testId="policy-body" /> : <p className="text-muted-foreground">No stack policy is set, so every resource in the stack can be updated.</p>}
          <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground"><b className="font-semibold text-foreground">Applied locally.</b> With a policy set, an update to a resource needs a statement that allows <span className="font-mono text-[12.5px]">Update:Modify</span>, <span className="font-mono text-[12.5px]">Update:Replace</span> or <span className="font-mono text-[12.5px]">Update:Delete</span> on it, and a Deny wins. SetStackPolicy stores a policy and GetStackPolicy reads it back.</p>
        </Block>
        <Block title="Rollback configuration" lede="Alarms that would roll back an operation." testId="rollback-block">
          {stack.RollbackConfiguration?.RollbackTriggers?.length ? (
            <ul className="grid gap-1">{stack.RollbackConfiguration.RollbackTriggers.map((t) => <li key={t.Arn} className="font-mono text-[13px] break-all">{t.Arn} <span className="text-muted-foreground">{t.Type}</span></li>)}</ul>
          ) : <p className="text-muted-foreground">No rollback trigger is set.</p>}
          <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground"><b className="font-semibold text-foreground">Not kept in doze-aws.</b> A call that sends a RollbackConfiguration is accepted and the stack answers an empty one, so no alarm is watched during an update.</p>
        </Block>
        <Block title="Notification options" lede="Topics for stack events." testId="notify-block">
          {topics.length === 0 ? <p className="text-muted-foreground">No topic receives this stack's events.</p> : <ul className="grid gap-1">{topics.map((a) => <li key={a} className="flex flex-wrap gap-x-4"><span className="text-muted-foreground">Amazon SNS topic</span><ResourceTo to={{ service: "sns", kind: "topics", name: a.split(":").pop() ?? a }} className="font-mono text-[13px] break-all">{a}</ResourceTo></li>)}</ul>}
          <p className="border-l-2 pl-3 text-[13px] leading-5 text-muted-foreground"><b className="font-semibold text-foreground">Applied locally.</b> Each event of the stack is published to its topics as CloudFormation words it: a topic that is gone loses the message.</p>
        </Block>
      </div>
      <RecentCalls calls={d.calls} noun="stack" title="Recent calls" scope={stack.StackName} className="mt-6" />
    </div>
  );
}

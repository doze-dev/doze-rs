// Step 3 (cloudformation__stack-create-options, stack-update-options): tags, the IAM role, what to do when a resource fails, the topic that
// receives the stack's events, the rollback configuration, the stack policy, and (a new stack only) the timeout and termination protection.
// What doze-aws does with each is said where it is asked, from the crate: see the notes on `tab-info.tsx`.
import { Callout } from "@doze/ui/components/app/callout";
import { Checkbox } from "@doze/ui/components/ui/checkbox";
import { Input } from "@doze/ui/components/ui/input";
import { Textarea } from "@doze/ui/components/ui/textarea";
import { KeyValueEditor } from "@doze/ui/components/app/key-value-editor";
import { Field, FormSection, LocalStatus, RolePicker, TAG_RULES, TopicPicker } from "../../../kit";
import { useAbout } from "../../../api/queries";
import { Choices, Note } from "./wizard-parts";
import type { Summary, Wizard } from "./wizard-model";

export function StepOptions({ w, set, summary, tried }: { w: Wizard; set: (p: Partial<Wizard>) => void; summary: Summary; tried: boolean }) {
  const account = useAbout().data?.account ?? "";
  const update = w.mode !== "create";
  const caps = summary.Capabilities ?? [];
  return (
    <>
      <FormSection title="Tags" optional description={update ? "The stack's current tags. Changing them is itself an update." : "Key-value pairs on the stack. Up to 50."} testId="section-tags">
        <KeyValueEditor rows={w.tags} onChange={(tags) => set({ tags })} rules={TAG_RULES} showErrors={tried} addLabel="Add tag" removeLabel="Remove" testId="tags" />
        <Note lead="Applied locally." testId="tags-note">The stack's tags are added to each resource it makes whose type takes tags; a tag the template sets itself wins. Up to 50 tags.</Note>
      </FormSection>
      <FormSection title="Permissions" optional description="An existing IAM service role that CloudFormation can assume." testId="section-permissions">
        <Field label="IAM role" testId="field-role">
          {(id) => <RolePicker id={id} value={w.role ? (w.role.split("/").pop() ?? null) : null} onChange={(name) => set({ role: name ? `arn:aws:iam::${account}:role/${name}` : null })} />}
        </Field>
        <LocalStatus state="stored" reason="The call keeps the role and DescribeStacks returns it; CloudFormation here does not assume it, so resources are made with the caller's own permissions." />
      </FormSection>
      <FormSection title={update ? "Behavior on provisioning failure" : "Stack failure options"} description={update ? "What happens if a resource fails to update." : "What happens to the resources if one of them fails to create."} testId="section-failure">
        <Choices rows name="failure" label="Stack failure options" value={w.failure} onChange={(failure) => set({ failure })}
          options={[
            { value: "rollback", title: "Roll back all stack resources", body: update ? "The stack returns to its last working state, UPDATE_ROLLBACK_COMPLETE." : "CloudFormation deletes what it created and the stack ends in ROLLBACK_COMPLETE." },
            { value: "preserve", title: "Preserve successfully provisioned resources", body: update ? "What the update changed stays as it is and the failed resource stays failed: the stack ends UPDATE_FAILED." : "Resources that were created stay as they are, and the failed one stays failed: the stack ends CREATE_FAILED." },
          ]} />
        <Note testId="failure-note">Choosing to preserve sends DisableRollback true; the default is to roll back. {update ? "" : "A rolled-back stack can only be deleted, so a failed trial costs one delete before the name is free again."}</Note>
      </FormSection>
      <FormSection title="Notification options" optional description={update ? "Topics that receive this update's events." : "Where CloudFormation publishes stack events."} testId="section-notify">
        <Choices rows name="notify" label="Notification options" value="existing" onChange={() => undefined}
          options={[
            { value: "existing", title: "Existing Amazon SNS topic" },
            { value: "new", off: true, title: "New Amazon SNS topic", body: "Not available in doze-aws: it asks for an email address to notify, and doze-aws stores email subscriptions without sending anything." },
          ]} />
        <Field label="Amazon SNS topic ARN" testId="field-topic">
          {(id) => <TopicPicker id={id} value={w.topic || null} onChange={(topic) => set({ topic: topic ?? "" })} />}
        </Field>
        <Note lead="Applied locally." testId="notify-note">Each stack event is published to the topic as CloudFormation words it; the API takes up to 5 topic ARNs and this form sets one.</Note>
      </FormSection>
      <FormSection title="Rollback configuration" optional collapsible defaultOpen={false} summary="CloudWatch alarms that would roll the operation back: up to 5, watched for 0 to 180 minutes after it. None set. Not kept in doze-aws." description="CloudWatch alarms that would roll the operation back." testId="section-rollback">
        <Note lead="Not kept in doze-aws." testId="rollback-note">A call that sends a RollbackConfiguration is accepted and the stack answers an empty one, so no alarm is watched during an operation. The form leaves it out of the call.</Note>
      </FormSection>
      <FormSection title="Stack policy" optional description={update ? "A policy that protects resources from unintended updates." : "No policy: every resource can be updated."} testId="section-policy">
        <Field label="Stack policy (JSON)" optional testId="field-policy">
          {(id) => <Textarea id={id} value={w.policy} onChange={(e) => set({ policy: e.target.value })} rows={5} className="bg-card font-mono text-[12.5px]" placeholder={'{"Statement":[{"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*"}]}'} spellCheck={false} data-testid="stack-policy" />}
        </Field>
        <Note lead="Applied locally." testId="policy-note">CreateStack and UpdateStack keep the policy, and an update to a resource then needs a statement that allows it. GetStackPolicy returns it.</Note>
      </FormSection>
      {!update && (
        <FormSection title="Stack creation options" description="These apply only when the stack is created; an update has no such options." testId="section-creation">
          <Field label="Timeout" optional testId="field-timeout">
            {(id) => <div className="flex items-center gap-2"><Input id={id} value={w.timeout} onChange={(e) => set({ timeout: e.target.value })} inputMode="numeric" placeholder="No timeout" className="w-40 bg-card" data-testid="stack-timeout" /><span className="text-[13px] text-muted-foreground">minutes</span></div>}
          </Field>
          <LocalStatus state="stored" reason="TimeoutInMinutes is kept and returned by DescribeStacks, but nothing times a stack out: its resources are made in the background and the stack ends when they do." />
          <Field label="Termination protection" testId="field-protect">
            {() => (
              <Choices rows name="protect" label="Termination protection" value={w.protect ? "on" : "off"} onChange={(v) => set({ protect: v === "on" })}
                options={[{ value: "off", title: "Disable", body: "Disabled is the default." }, { value: "on", title: "Enable", body: "DeleteStack then fails and leaves the stack as it is, until you turn it off." }]} />
            )}
          </Field>
        </FormSection>
      )}
      {update && <Callout tone="info" lead="Timeout and termination protection are stack creation options." testId="creation-only-note">They are not part of a stack update. Termination protection is changed from the stack page, Actions, Turn on termination protection.</Callout>}
      {(caps.length > 0 || update) && <FormSection title="Capabilities" description={caps.length > 0 ? "Acknowledge what this template needs." : "What the template needs acknowledged."} testId="section-capabilities">
        {caps.length > 0 ? (
          <label className="flex items-start gap-3 text-[13.5px] leading-5"><Checkbox checked={w.capabilities} onCheckedChange={(c) => set({ capabilities: c === true })} className="mt-0.5" data-testid="capabilities" /><span>I acknowledge that CloudFormation may create IAM resources: <span className="font-mono text-[12.5px]">{caps.join(", ")}</span>. {summary.CapabilitiesReason}</span></label>
        ) : (
          <p className="text-[13.5px] text-muted-foreground" data-testid="no-capabilities">Nothing to acknowledge. {update ? "This template has" : "This template holds"} no IAM resources, so no capabilities are sent.</p>
        )}
      </FormSection>}
    </>
  );
}

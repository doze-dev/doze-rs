import { BellOffIcon, KeyRoundIcon, SendIcon, TagsIcon } from "lucide-react";
import { useState } from "react";
import { EmptyTab } from "@doze/ui/components/app/empty-tab";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { ResultBanner } from "@doze/ui/components/app/result-banner";
import { Button } from "@doze/ui/components/ui/button";
import { DetailFields, type ResourceTab, ResourcePage, ResourcePageSkeleton, TagsTab, TagsView, type Verb } from "@aws-kit";
import { Section, Specimen } from "../specimen";
import { BARE, QUEUE } from "./resource-fixtures";
import { Blockers, Confirms, Edits } from "./resource-overlays";

const TARGET = { service: "sqs", kind: "queues", name: "pick-lists" };

function QueuePage() {
  const [said, setSaid] = useState<string | null>(null);
  const tabs: ResourceTab[] = [
    { id: "sns", label: "SNS subscriptions", render: () => <EmptyTab icon={BellOffIcon} title="No topic is subscribed to this queue">Messages published to a topic reach pick-lists only through a subscription.</EmptyTab> },
    { id: "lambda", label: "Lambda triggers", count: 0, render: () => <EmptyTab icon={SendIcon} title="No function is triggered by this queue" action={<Button variant="outline" size="sm" className="bg-card">Create trigger</Button>}>Messages wait in the queue until something receives them.</EmptyTab> },
    { id: "tagging", label: "Tagging", render: () => <TagsTab target={TARGET} noun="cost allocation tag" /> },
    { id: "encryption", label: "Encryption", render: () => <EmptyTab icon={KeyRoundIcon} title="Server-side encryption is stored, not applied">doze-aws keeps the setting and returns it; it encrypts no message.</EmptyTab> },
  ];
  const onVerb = (v: Verb) => setSaid(`${v.label} was chosen.`);
  return (
    <div className="rounded-xl border bg-background p-6" data-testid="queue-page">
      <ResourcePage
        detail={QUEUE}
        meta="Created Nov 6, 2026 18:42. Last modified Nov 7, 2026 09:05."
        banner={said ? <ResultBanner title={said} returned="This is a specimen: nothing was sent." onDismiss={() => setSaid(null)} /> : undefined}
        primary="send"
        verbStates={{ redrive: { disabled: "pick-lists-dlq holds no messages that failed." } }}
        onVerb={onVerb}
        overview={<DetailFields detail={QUEUE} />}
        tabs={tabs}
      />
    </div>
  );
}

function Banners() {
  const [on, setOn] = useState(true);
  return (
    <div className="grid w-full gap-3">
      <ResultBanner title="Queue stock-moves.fifo created." returned="CreateQueue returned its URL, and the queue's Details page opens." />
      <ResultBanner title="Purged." returned="PurgeQueue was accepted." left="Messages sent in the last 60 seconds may still arrive, as SQS says." tone="info" />
      <ResultBanner tone="warn" title="Message sent, with a warning." returned="SendMessage returned MessageId 6f1c-0b2e." left="The queue has no consumer, so it waits." actions={<Button size="sm" variant="outline" className="bg-card">Receive it</Button>} />
      <ResultBanner tone="bad" title="Topic not created." returned="InvalidParameter: Topic names are made of letters, numbers, hyphens and underscores." />
      {on && <ResultBanner title="Published." returned="Publish returned MessageId a2f9 and the topic has 2 subscriptions, so 2 deliveries are queued." onDismiss={() => setOn(false)} testId="dismissable" />}
      <ResultBanner title="A result with a very long name: a-resource-whose-name-is-far-longer-than-its-column-so-that-the-wrap-is-seen." returned="The call returned an answer that is also long enough to wrap onto a second and a third line on a narrow window, as a person's own words can." />
    </div>
  );
}

export function ResourcePageGallery() {
  return (
    <>
      <PageHeader title="Resource" sub="One page for every kind of resource: the header with its verbs, a live sentence, what it is wired to, tabs kept in the address, and the pieces a delete or an edit asks for." />

      <Section id="resourcepage" title="ResourcePage" source="apps/aws/src/kit/resource-page.tsx" note="Verbs that cannot apply say why. The Wired to strip links to what the resource is connected to. Tabs are kept in the address (?tab=), and only the tab shown is drawn.">
        <Specimen label="A queue" stack>
          <QueuePage />
        </Specimen>
        <Specimen label="A kind with nothing of its own" stack>
          <div className="w-full rounded-xl border bg-background p-6" data-testid="bare-page">
            <ResourcePage detail={BARE} testId="bare" />
          </div>
        </Specimen>
        <Specimen label="Loading" stack>
          <div className="w-full rounded-xl border bg-background p-6">
            <ResourcePageSkeleton />
          </div>
        </Specimen>
      </Section>

      <Section id="banner" title="ResultBanner" source="components/app/result-banner.tsx" note="What a write did, said where the person is looking: the result, what the call returned, what is left. It replaces a toast and a silent navigation.">
        <Specimen label="Tones" stack>
          <Banners />
        </Specimen>
      </Section>

      <Section id="tags" title="TagsView and EmptyTab" source="apps/aws/src/kit/tags-tab.tsx" note="Tags as a tab: what there are, or that there are none and what to do about it, and the edit in place. The save reports what it changed; a refusal stays on the form.">
        <Specimen label="Tags of pick-lists" stack>
          <TagsTab target={TARGET} noun="cost allocation tag" />
        </Specimen>
        <Specimen label="None" stack>
          <TagsView answer={{ tags: [], editable: true }} onSave={async () => undefined} noun="cost allocation tag" />
        </Specimen>
        <Specimen label="Cannot be tagged here" stack>
          <TagsView answer={{ tags: [{ key: "managed-by", value: "aws" }], editable: false }} onSave={async () => undefined} />
        </Specimen>
        <Specimen label="Loading" stack>
          <TagsView answer={undefined} loading onSave={async () => undefined} />
        </Specimen>
        <Specimen label="Could not be read" stack>
          <TagsView answer={undefined} loadError={new Error("The tags could not be read.")} onSave={async () => undefined} />
        </Specimen>
        <Specimen label="EmptyTab, with an action" stack>
          <EmptyTab icon={TagsIcon} title="This role has no inline policies" action={<Button size="sm" variant="outline" className="bg-card">Add inline policy</Button>} className="w-full">
            Everything the role may do comes from its attached policies. An inline policy applies to this role alone and is deleted with it.
          </EmptyTab>
        </Specimen>
      </Section>

      <Blockers />
      <Confirms />
      <Edits />
    </>
  );
}

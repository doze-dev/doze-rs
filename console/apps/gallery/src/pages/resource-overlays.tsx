import { useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { JsonEditor } from "@doze/ui/components/app/json-editor";
import { TypedConfirm } from "@doze/ui/components/app/typed-confirm";
import { Button } from "@doze/ui/components/ui/button";
import { Input } from "@doze/ui/components/ui/input";
import { type Blockers as BlockersData, BlockersList, DeletionWindow, Field, SectionEdit } from "@aws-kit";
import { Section, Specimen } from "../specimen";

const ROLE: BlockersData = { blockers: [{ title: "Attached policies", items: [{ name: "AdministratorAccess", link: { service: "iam", kind: "policies", name: "AdministratorAccess" } }], clears: true, blocks: true, more: false }], kept: ["The policy itself stays."], words: "larder-admin" };
const BUCKET: BlockersData = { blockers: [{ title: "Objects", items: [{ name: "invoices/2026-10.pdf", link: null }, { name: "invoices/2026-11.pdf", link: null }], clears: false, blocks: true, more: true }], kept: [], words: "larder-exports" };
const FN: BlockersData = { blockers: [{ title: "Event source mappings", items: [{ name: "pick-lists", link: { service: "sqs", kind: "queues", name: "pick-lists" } }], clears: true, blocks: false, more: false }, { title: "Aliases", items: [{ name: "live", link: null }], clears: false, blocks: true, more: false }, { title: "Layers", items: [{ name: "shared-deps", link: null }], clears: false, blocks: false, more: false }], kept: ["The log group the function wrote to stays.", "The role it ran as stays."], words: "resize-images" };

export function Blockers() {
  const [days, setDays] = useState("30");
  const [bad, setBad] = useState("3");
  return (
    <Section id="blockers" title="BlockersList and DeletionWindow" source="apps/aws/src/kit/blockers.tsx" note="What stands in the way of a delete: what it clears (a warning), what must be dealt with first (a stop), and what survives. A deletion that waits has a window of 7 to 30 days, and a scheduled one says when it ends and offers Cancel deletion.">
      <Specimen label="The delete clears it" stack>
        <BlockersList data={ROLE} className="w-full max-w-2xl" />
      </Specimen>
      <Specimen label="It must be removed first" stack>
        <BlockersList data={BUCKET} className="w-full max-w-2xl" />
      </Specimen>
      <Specimen label="Both, and what stays" stack>
        <BlockersList data={FN} className="w-full max-w-2xl" />
      </Specimen>
      <Specimen label="Loading" stack>
        <BlockersList data={undefined} loading className="w-full max-w-2xl" />
      </Specimen>
      <Specimen label="Choosing the window" stack>
        <DeletionWindow mode="choose" noun="key" days={days} onDays={setDays} now={Date.UTC(2026, 10, 8, 9, 0)} className="w-full max-w-2xl" />
      </Specimen>
      <Specimen label="A window that is not allowed" stack>
        <DeletionWindow mode="choose" noun="secret" days={bad} onDays={setBad} now={Date.UTC(2026, 10, 8, 9, 0)} className="w-full max-w-2xl" />
      </Specimen>
      <Specimen label="Scheduled" stack>
        <DeletionWindow mode="scheduled" noun="key" on={Date.UTC(2026, 10, 15, 9, 0)} onRestore={() => undefined} className="w-full max-w-3xl" />
      </Specimen>
    </Section>
  );
}

function RoleDelete() {
  const [open, setOpen] = useState(false);
  const [gone, setGone] = useState(false);
  return (
    <>
      <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setOpen(true)} data-testid="open-role-delete">
        Delete larder-admin…
      </Button>
      {gone && <span className="text-[13px] text-muted-foreground" data-testid="role-deleted">larder-admin deleted.</span>}
      <TypedConfirm open={open} onOpenChange={setOpen} title="Delete role" description="This deletes larder-admin." words="larder-admin" noun="role name" verb="Delete" onConfirm={() => { setGone(true); setOpen(false); }} testId="role-confirm">
        <BlockersList data={ROLE} />
      </TypedConfirm>
    </>
  );
}

function BucketDelete() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setOpen(true)} data-testid="open-bucket-delete">
        Delete larder-exports…
      </Button>
      <TypedConfirm
        open={open}
        onOpenChange={setOpen}
        title="Delete bucket"
        description="This deletes larder-exports."
        words="larder-exports"
        noun="bucket name"
        verb="Delete bucket"
        onConfirm={() => undefined}
        refusal={
          <Alert tone="bad" title="BucketNotEmpty: the bucket you tried to delete is not empty." testId="bucket-refused">
            Empty the bucket first, then delete it.
          </Alert>
        }
        testId="bucket-confirm"
      >
        <BlockersList data={BUCKET} />
      </TypedConfirm>
    </>
  );
}

function KeyDelete() {
  const [open, setOpen] = useState(false);
  const [days, setDays] = useState("30");
  return (
    <>
      <Button variant="outline" className="border-bad/40 bg-card text-bad hover:text-bad" onClick={() => setOpen(true)} data-testid="open-key-delete">
        Schedule deletion of alias/larder-app…
      </Button>
      <TypedConfirm open={open} onOpenChange={setOpen} title="Schedule key deletion" description="The key cannot be used while it waits, and what it encrypted cannot be read after it is gone." words="alias/larder-app" noun="key alias" verb="Schedule deletion" onConfirm={() => undefined} testId="key-confirm">
        <DeletionWindow mode="choose" noun="key" days={days} onDays={setDays} now={Date.UTC(2026, 10, 8, 9, 0)} />
      </TypedConfirm>
    </>
  );
}

export function Confirms() {
  return (
    <Section id="confirm" title="TypedConfirm" source="components/app/typed-confirm.tsx" note="A dialog that asks for the resource's name. The button is off until it is typed; Esc and Cancel close it and the focus returns to what opened it. The page's own words go between the description and the field.">
      <Specimen label="Delete a role (clears a policy)">
        <RoleDelete />
      </Specimen>
      <Specimen label="Delete a bucket (refused)">
        <BucketDelete />
      </Specimen>
      <Specimen label="Schedule a key's deletion">
        <KeyDelete />
      </Specimen>
    </Section>
  );
}

const CORS = JSON.stringify([{ AllowedMethods: ["GET", "PUT"], AllowedOrigins: ["http://localhost:3000"], AllowedHeaders: ["*"], MaxAgeSeconds: 3000 }], null, 2);

function Retention() {
  const [open, setOpen] = useState(false);
  const [hours, setHours] = useState("48");
  const n = Number(hours);
  const bad = !Number.isInteger(n) || n < 24 || n > 8760;
  return (
    <>
      <Button variant="outline" className="bg-card" onClick={() => setOpen(true)} data-testid="open-retention">
        Edit retention…
      </Button>
      <SectionEdit open={open} onOpenChange={setOpen} title="Edit retention period" description="How long Kinesis keeps a record after it is written." request={{ service: "kinesis", operation: "IncreaseStreamRetentionPeriod", input: { StreamName: "clicks", RetentionPeriodHours: bad ? undefined : n } }} onSave={() => setOpen(false)} disabled={bad} disabledWhy="Retention is 24 to 8,760 hours." testId="retention-edit">
        <Field label="Retention period (hours)" help={<>Range <b>24</b> to <b>8,760</b> hours, default <b>24</b>.</>} error={bad ? "Enter a whole number from 24 to 8,760." : undefined}>
          {(id) => <Input id={id} value={hours} onChange={(e) => setHours(e.target.value)} inputMode="numeric" className="w-40 font-mono" data-testid="retention-hours" />}
        </Field>
      </SectionEdit>
    </>
  );
}

function Cors() {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState(CORS);
  return (
    <>
      <Button variant="outline" className="bg-card" onClick={() => setOpen(true)} data-testid="open-cors">
        Edit CORS…
      </Button>
      <SectionEdit open={open} onOpenChange={setOpen} wide title="Edit cross-origin resource sharing" request={{ service: "s3", operation: "PutBucketCors", input: { Bucket: "larder-exports", CORSConfiguration: { CORSRules: JSON.parse(text) as unknown } } }} onSave={() => setOpen(false)} error={new Error("The CORS configuration could not be saved.")} testId="cors-edit">
        <JsonEditor value={text} onChange={setText} label="CORS rules" rows={12} />
      </SectionEdit>
    </>
  );
}

export function Edits() {
  return (
    <Section id="sectionedit" title="SectionEdit" source="apps/aws/src/kit/section-edit.tsx" note="A small edit of one part of a resource, as an overlay: the fields, the call they make beside them, and the service's refusal in place.">
      <Specimen label="A number">
        <Retention />
      </Specimen>
      <Specimen label="A JSON document, refused">
        <Cors />
      </Specimen>
    </Section>
  );
}

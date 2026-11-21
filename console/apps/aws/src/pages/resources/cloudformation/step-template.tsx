// Step 1 of the create and update wizards (cloudformation__stack-create, stack-create-invalid, stack-update): which template, from an S3 URL or
// a file; the call that reads it is ValidateTemplate.
import { Alert } from "@doze/ui/components/app/alert";
import { Button } from "@doze/ui/components/ui/button";
import { FileTextIcon } from "lucide-react";
import { Field, FormSection, RuleHelp } from "../../../kit";
import { Input } from "@doze/ui/components/ui/input";
import { Choices, Note, s3Of } from "./wizard-parts";
import { BODY_MAX, type Wizard } from "./wizard-model";
import { useRef } from "react";

export function StepTemplate({ w, set, problem, stackName }: { w: Wizard; set: (p: Partial<Wizard>) => void; problem: string | null; stackName: string }) {
  const update = w.mode !== "create";
  const file = useRef<HTMLInputElement>(null);
  const bucket = s3Of(w.url)?.bucket;
  const choose = async (f: File | undefined) => {
    if (f) set({ file: { name: f.name, text: await f.text() } });
  };
  const bytes = w.file ? new TextEncoder().encode(w.file.text).length : 0;
  return (
    <>
      <FormSection title={update ? "Prepare template" : "Prerequisite - Prepare template"} description={update ? "What the update should deploy." : "Every stack starts from a template."} testId="section-prepare">
        {update ? (
          <Choices name="prepare" label="Prepare template" columns={3} value={w.keepTemplate ? "existing" : "replace"} onChange={(v) => set({ keepTemplate: v === "existing" })}
            options={[
              { value: "existing", title: "Use existing template", body: "Keep the template as it is and change only parameters or options." },
              { value: "replace", title: "Replace existing template", body: "Point to a changed copy of the template." },
              { value: "studio", off: true, title: "Open in template studio", body: "Not available in doze-aws: template studio is an editor hosted in the AWS console." },
            ]} />
        ) : (
          <Choices name="prepare" label="Prepare template" value="existing" onChange={() => undefined}
            options={[
              { value: "existing", title: "Choose an existing template", body: "Specify a template you have already written." },
              { value: "studio", off: true, title: "Build with template studio", body: "Not available in doze-aws: template studio is an editor hosted in the AWS console." },
            ]} />
        )}
      </FormSection>
      {(!update || !w.keepTemplate) && (
        <FormSection title="Specify template" description={update ? "Where the changed template is." : "Where CloudFormation should read the template from."} testId="section-specify">
          <Choices name="source" label="Template source" value={w.source} onChange={(source) => set({ source })}
            options={[
              { value: "url", title: "Amazon S3 URL", body: "A template in an S3 bucket you can read." },
              { value: "file", title: "Upload a template file", body: `A file from your computer, ${(BODY_MAX / 1024).toFixed(1).replace(/\.0$/, "")} KB or less.` },
            ]} />
          {w.source === "url" ? (
            <Field label="Amazon S3 URL" error={problem ?? undefined} testId="field-url"
              help={<>The URL can be up to 1024 characters and must point to a template of 1 MB or less. To use one version of the file in a versioning-enabled bucket, end the URL with <span className="font-mono text-[12.5px]">?versionId=&lt;version-id&gt;</span>.</>}>
              {(id) => <Input id={id} value={w.url} onChange={(e) => set({ url: e.target.value })} placeholder="https://bucket.s3.us-east-1.amazonaws.com/templates/app.yaml" className="bg-card font-mono text-[13px]" autoComplete="off" spellCheck={false} data-testid="template-url" />}
            </Field>
          ) : (
            <Field label="Upload a template file" error={problem ?? undefined} testId="field-file">
              {() => (
                <div className="flex flex-wrap items-center gap-3">
                  <input ref={file} type="file" accept=".json,.yaml,.yml,.template,.txt,application/json,text/yaml" className="sr-only" onChange={(e) => void choose(e.target.files?.[0])} data-testid="template-file" aria-label="Template file" />
                  <Button type="button" variant="outline" className="bg-card" onClick={() => file.current?.click()} data-testid="choose-file"><FileTextIcon /> Choose file</Button>
                  <span className="font-mono text-[13px] text-muted-foreground" data-testid="file-name">{w.file ? `${w.file.name} · ${(bytes / 1024).toFixed(1)} KB` : "No file chosen"}</span>
                </div>
              )}
            </Field>
          )}
          {w.source === "file" && <RuleHelp>The file must be {(BODY_MAX / 1024).toFixed(1).replace(/\.0$/, "")} KB or less: it goes to CloudFormation as the call's TemplateBody, which the API limits to {BODY_MAX.toLocaleString("en-US")} bytes. {w.file && bytes <= BODY_MAX ? `This one is ${(bytes / 1024).toFixed(1)} KB, so the size is not the problem.` : "A bigger template goes in an S3 bucket and is read from its URL."}</RuleHelp>}
          {update && <Note testId="update-note">Properties that can't be updated must keep their current values, and changing some properties interrupts or replaces a resource. The last step previews the changes.</Note>}
          <Note testId="check-note">When you choose Next, CloudFormation checks the file as JSON first and, if that fails, as YAML; it also catches some semantic mistakes such as circular dependencies. If both fail you get a template validation error before the wizard continues.{w.source === "url" && bucket ? ` The file is read from the ${bucket} bucket of this doze-aws's S3.` : ""}</Note>
        </FormSection>
      )}
      {update && w.keepTemplate && <Alert tone="info" title={`${stackName} keeps its template.`} testId="keep-note">Next reads the template the stack has now, for its parameters. The update will send UsePreviousTemplate true instead of a URL.</Alert>}
    </>
  );
}

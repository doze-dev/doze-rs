// A stack's Template tab (cloudformation__stack-template): the template as it was sent, or as CloudFormation processed it (a
// transform such as SAM or AWS::Include is expanded), read only.
import { useState } from "react";
import { CodeBlock, pretty } from "@doze/ui/components/app/code-block";
import { CopyButton } from "@doze/ui/components/app/copy-button";
import { Button } from "@doze/ui/components/ui/button";
import { RuleHelp, Segmented } from "../../../kit";
import { Loaded } from "../deep/parts";
import { templateText, useTemplate } from "./api";
import { Section } from "./parts";

export function TemplateTab({ name }: { name: string }) {
  const [stage, setStage] = useState<"Original" | "Processed">("Original");
  const [wrap, setWrap] = useState(false);
  const q = useTemplate(name, stage);
  const body = templateText(q.data?.TemplateBody);
  const json = body.trim().startsWith("{");
  return (
    <Section
      title="Template"
      lede={stage === "Original" ? "The template this stack was created from." : "The template after CloudFormation expanded its transforms: what it deployed."}
      testId="stack-template"
      actions={
        <>
          <Segmented label="Template stage" value={stage} onChange={setStage} options={[{ value: "Original", label: "View original" }, { value: "Processed", label: "View processed" }]} testId="template-stage" />
          <Button variant="outline" size="sm" className="bg-card" onClick={() => setWrap(!wrap)} aria-pressed={wrap}>{wrap ? "No wrap" : "Wrap lines"}</Button>
          <CopyButton text={body} what="Template copied" className="bg-card" />
        </>
      }
    >
      <Loaded q={q} what="The template">{() => <CodeBlock text={json ? pretty(body) : body} wrap={wrap} testId="template-body" max="max-h-[640px]" />}</Loaded>
      <RuleHelp className="max-w-[56rem]">
        This is the {stage.toLowerCase()} stage{json ? " (JSON, indented here)" : ""}. A template with a transform (AWS::Serverless, AWS::Include) differs between the two stages; the processed one is what CloudFormation deploys. With none, the processed template is the same one in JSON.
      </RuleHelp>
    </Section>
  );
}

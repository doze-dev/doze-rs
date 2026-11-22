import { useAbout } from "../api/queries";
import { CodeBlock } from "../pages/card";
import { snippetFor } from "./app-snippets";

/** "Or from your app": under an empty place, the call that makes the first resource, as the AWS CLI and as boto3, for this
 * address. A script that creates it has the same result as the button. */
export function FromYourApp({ service }: { service: string }) {
  const about = useAbout().data;
  const s = about ? snippetFor(service, about.endpoint, about.region) : null;
  if (!s) return null;
  return (
    <section className="mx-auto mt-6 grid max-w-[46rem] gap-4 text-left" aria-label="Or from your app" data-testid="from-your-app">
      <h3 className="text-[13px] font-semibold text-bright">Or from your app</h3>
      <div>
        <div className="mb-1.5 text-[12px] text-muted-foreground">AWS CLI</div>
        <CodeBlock code={s.cli} what="Command copied" testId="from-app-cli" />
      </div>
      <div>
        <div className="mb-1.5 text-[12px] text-muted-foreground">Python · boto3</div>
        <CodeBlock code={s.py} what="Snippet copied" testId="from-app-py" />
      </div>
    </section>
  );
}

// Create resource (apigateway__rest-resource-create): a path segment under a parent, a proxy or a {parameter}, and CORS (the OPTIONS
// method on a mock integration). Methods are added to the resource afterwards.
import { useMemo, useState } from "react";
import { Alert } from "@doze/ui/components/app/alert";
import { Input } from "@doze/ui/components/ui/input";
import { Label } from "@doze/ui/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@doze/ui/components/ui/select";
import { Switch } from "@doze/ui/components/ui/switch";
import { ApiFailure } from "@doze/ui/lib/api";
import { Field, FormPage, FormSection, RuleHelp, callKindOp } from "../../../kit";
import { sdkShown } from "./calls";
import { REST, useRefreshReads, useResources, useWrite } from "./api";
import { corsCalls, createResourceInput, fullPath, partOf, proxyName, resourceProblem, type ResourceForm } from "./resource-model";
import type { RestResource } from "./types";
import { flattenTree, methodPhrase, methodsOf } from "./words";

export function ResourceCreate({ api, apiName, parent, crumbs, onClose }: { api: string; apiName: string; parent: string | undefined; crumbs: { label: string; to: string }[]; onClose: (created?: string) => void }) {
  const tree = useResources(api).data?.item ?? [];
  const refresh = useRefreshReads();
  const root = tree.find((r) => !r.parentId);
  const [f, setF] = useState<ResourceForm>({ proxy: false, parent: parent ?? "", name: "", cors: false });
  const [tried, setTried] = useState(false);
  const [stage, setStage] = useState<string | null>(null);
  const make = useWrite<RestResource>(REST, "CreateResource");
  const where = f.parent || root?.id || "";
  const form = { ...f, parent: where };
  const parentRes = tree.find((r) => r.id === where);
  const set = (patch: Partial<ResourceForm>) => { setF((x) => ({ ...x, parent: where, ...patch })); make.reset(); setStage(null); };
  const problem = resourceProblem(form);
  const input = useMemo(() => createResourceInput(api, form), [api, form.parent, form.proxy, form.name]); // eslint-disable-line react-hooks/exhaustive-deps
  const failure = make.error instanceof ApiFailure ? { code: make.error.code, message: make.error.message } : null;
  const requests = [{ service: "apigateway", operation: "CreateResource", input }, ...(f.cors ? corsCalls(api, "<new resource>").map((c) => ({ service: "apigateway", operation: c.operation, input: c.input })) : [])];
  const submit = () => {
    setTried(true);
    if (problem) return;
    make.mutate(input as never, {
      onSuccess: async (made) => {
        try {
          if (f.cors) for (const c of corsCalls(api, made.id)) { setStage(c.operation); await callKindOp(REST, c.operation, c.input); }
        } catch (e) {
          refresh();
          setStage(`The resource was made, but ${e instanceof ApiFailure ? e.message : "its OPTIONS method was not"}`);
          return;
        }
        refresh();
        onClose(made.path);
      },
    });
  };
  const parents = flattenTree(tree);
  return (
    <FormPage
      title="Create resource"
      sub={<>Add a path to <span className="font-mono">{apiName}</span>. A resource is a path segment; methods are added to it afterwards.</>}
      crumbs={[...crumbs, { label: "Resources", to: `/resources/apigateway/restapis/${api}` }, { label: "Create resource", to: `/resources/apigateway/restapis/${api}?do=create-resource` }]}
      request={requests[0]!}
      requests={requests}
      showInput
      shown={sdkShown}
      refusal={failure}
      callNote={<><span className="font-mono">parentId</span> is the resource ID of {parentRes?.path ?? "the parent"}. The answer returns the new resource&apos;s ID and its full path.{f.cors && <> With CORS on, the four calls of the OPTIONS method follow, each with the new resource&apos;s ID.</>}</>}
      banner={failure || (stage && stage.startsWith("The resource")) ? <Alert tone="bad" title="The resource wasn't completed." testId="create-refused">{failure ? <>CreateResource answered <span className="font-mono font-semibold text-bright">{failure.code}</span>: “{failure.message}”</> : stage}</Alert> : undefined}
      submitLabel="Create resource"
      pending={make.isPending || (stage !== null && !stage.startsWith("The resource"))}
      disabled={tried && problem !== null}
      disabledWhy={problem ?? undefined}
      onCancel={() => onClose()}
      onSubmit={submit}
      testId="resource-create"
    >
      <FormSection title="Resource details" testId="section-details">
        <div className="flex items-start gap-3">
          <Switch id="proxy" checked={f.proxy} onCheckedChange={(proxy) => set({ proxy })} className="mt-0.5" data-testid="resource-proxy" />
          <div className="grid gap-0.5"><Label htmlFor="proxy">Proxy resource</Label><p className="text-[13px] text-muted-foreground">{f.proxy ? <>On. The resource is <span className="font-mono">{proxyName}</span>, and catches every child path.</> : <>Off. A proxy resource uses a greedy path parameter, such as <span className="font-mono">{proxyName}</span>, that catches every child path.</>}</p></div>
        </div>
        <Field label="Resource path" hint={parentRes ? <>The parent of the new resource. <span className="font-mono">{parentRes.path}</span> has {methodPhrase(methodsOf(parentRes).map((m) => m.httpMethod))}; the new resource is its child.</> : undefined}>
          {(id) => (
            <Select value={where} onValueChange={(v) => set({ parent: v })}>
              <SelectTrigger id={id} className="w-full bg-card font-mono" data-testid="resource-parent"><SelectValue /></SelectTrigger>
              <SelectContent>{parents.map(({ resource: r }) => <SelectItem key={r.id} value={r.id} className="font-mono">{r.path === "/" ? "/" : `${r.path}/`}</SelectItem>)}</SelectContent>
            </Select>
          )}
        </Field>
        <Field label="Resource name" error={tried ? (resourceProblem(form) ?? undefined) : undefined} help={<>Wrap a name in curly braces to make it a path parameter, so the full path reads <span className="font-mono">{fullPath(parentRes?.path ?? "/", "{sku}")}</span>. A product SKU such as <span className="font-mono">FV-TOM-V</span> then arrives as the value of <span className="font-mono">sku</span>.</>}>
          {(id) => <Input id={id} value={f.proxy ? proxyName : f.name} disabled={f.proxy} onChange={(e) => set({ name: e.target.value })} placeholder="{sku}" autoComplete="off" spellCheck={false} className="font-mono" data-testid="resource-name" />}
        </Field>
        <div className="flex items-start gap-3">
          <Switch id="cors" checked={f.cors} onCheckedChange={(cors) => set({ cors })} className="mt-0.5" data-testid="resource-cors" />
          <div className="grid gap-0.5"><Label htmlFor="cors">CORS (Cross Origin Resource Sharing)</Label><p className="text-[13px] text-muted-foreground">{f.cors ? <>On. The resource gets an OPTIONS method on a mock integration that answers a preflight request.</> : <>Off. A REST API answers CORS preflight requests with an OPTIONS method on a mock integration, like the one on {tree.find((r) => r.resourceMethods?.OPTIONS)?.path ?? "a resource"}.</>}</p></div>
        </div>
        <RuleHelp>The full path will be <span className="font-mono">{fullPath(parentRes?.path ?? "/", partOf(form))}</span>.</RuleHelp>
      </FormSection>
    </FormPage>
  );
}

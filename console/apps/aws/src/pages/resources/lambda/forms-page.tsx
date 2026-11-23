// The pages a function opens with `?view=`: its forms. An unknown one says so instead of showing nothing.
import { Alert } from "@doze/ui/components/app/alert";
import { ConfigEdit, EnvEdit } from "./forms-config";
import { AliasCreate, PermissionAdd, UrlCreate } from "./forms-access";
import { LayerAdd } from "./forms-layer";
import { DestinationAdd, TriggerAdd, TriggerEdit } from "./forms-trigger";
import type { FormProps } from "./form-bits";
import { useFunctionSearch } from "./hooks";

export function FunctionForm({ view, ...p }: Omit<FormProps, "tab" | "p"> & { view: string }) {
  const search = useFunctionSearch();
  const props: FormProps = { ...p, tab: search.tab, p: search.p };
  switch (view) {
    case "config-edit": return <ConfigEdit {...props} />;
    case "env-edit": return <EnvEdit {...props} />;
    case "trigger-add": return <TriggerAdd {...props} />;
    case "trigger-edit": return <TriggerEdit {...props} />;
    case "destination-add": return <DestinationAdd {...props} />;
    case "permission-add": return <PermissionAdd {...props} />;
    case "url-create": return <UrlCreate {...props} />;
    case "alias-create": return <AliasCreate {...props} />;
    case "layer-add": return <LayerAdd {...props} />;
    default: return <Alert tone="warn" title="No such page">{props.d.name} has no page called {view}.</Alert>;
  }
}

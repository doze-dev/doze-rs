// Lambda's pages of a function: test, configuration, versions and aliases, invocations.
import { lazy } from "react";
import { type DeepProps, register } from "../deep/registry";

register("lambda", {
  test: { page: lazy(() => import("./test-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionTest name={d.name} /> }))) },
  configuration: { page: lazy(() => import("./config-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionConfig name={d.name} /> }))) },
  versions: { page: lazy(() => import("./lists").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionVersions name={d.name} /> }))) },
  invocations: { page: lazy(() => import("./lists").then((m) => ({ default: ({ d }: DeepProps) => <m.FunctionInvocations d={d} /> }))) },
});

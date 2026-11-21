// Step Functions' pages of a state machine: its executions.
import { lazy } from "react";
import { type DeepProps, register } from "../deep/registry";

register("stepfunctions", {
  executions: { page: lazy(() => import("./executions-tab").then((m) => ({ default: ({ d }: DeepProps) => <m.MachineExecutions arn={d.id} name={d.name} /> }))) },
});

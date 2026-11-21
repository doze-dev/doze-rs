// CloudFormation's pages of a stack: its events, template and change sets, and what its details open with.
import { lazy } from "react";
import { type DeepProps, register, registerGone, registerLead } from "../deep/registry";

register("cloudformation", {
  events: { page: lazy(() => import("./stack-tabs").then((m) => ({ default: ({ d }: DeepProps) => <m.StackEvents name={d.name} /> }))) },
  template: { page: lazy(() => import("./stack-tabs").then((m) => ({ default: ({ d }: DeepProps) => <m.StackTemplate name={d.name} /> }))) },
  // Never listed as a tab (`when` is never true): the descriptor names ListStacks (deleted stacks only) so that the page of a name that is gone may read it.
  deleted: { page: lazy(() => import("./deleted").then((m) => ({ default: ({ d }: DeepProps) => <m.DeletedStack name={d.name} /> }))), when: () => false },
  changesets: { page: lazy(() => import("./stack-tabs").then((m) => ({ default: ({ d }: DeepProps) => <m.StackChangeSets name={d.name} /> }))) },
});
registerGone("cloudformation", "stacks", lazy(() => import("./deleted").then((m) => ({ default: m.DeletedStack }))));
registerLead("cloudformation", "stacks", lazy(() => import("./stack-tabs").then((m) => ({ default: m.StackSentence }))));

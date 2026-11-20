// The deep pages that are not a resource of a descriptor's kind: an execution (`/resources/stepfunctions/executions/<arn>`)
// and a log stream (`/resources/logs/streams/<stream>?group=<group>`). Loaded only when one is opened.
import { ExecutionPage } from "./sfn/execution-page";
import { StreamPage } from "./logs/stream-page";

export function DeepRoute({ service, name, group }: { service: string; kind: string; name: string; group?: string }) {
  return service === "stepfunctions" ? <ExecutionPage arn={name} /> : <StreamPage group={group ?? ""} stream={name} />;
}

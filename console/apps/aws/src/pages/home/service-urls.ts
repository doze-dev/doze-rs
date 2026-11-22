// What "By service" says: for each service of the build, the address it answers at, the environment variable an SDK reads for it,
// and the one thing about its URLs that is not the endpoint. Pure: the page draws the rows.
import type { ServiceRow } from "../../api/types/ServiceRow";
import { hostOf } from "./address";

/** The drawers a row can open. */
export type Drawer = "invoke" | "s3";

export type ServiceUrl = {
  name: string;
  title: string;
  /** `host:port`: the same for every service, since one address serves them all. */
  address: string;
  /** The variables an SDK reads to send this service's calls to one address (`AWS_ENDPOINT_URL_SQS`). */
  vars: string[];
  /** What is special about its URLs, in a line; null when nothing is. */
  detail: string | null;
  /** The drawer that says more, and what its link says. */
  more: { drawer: Drawer; label: string } | null;
};

/** The variable the AWS SDKs read for an API: its service id in capitals, every run of other characters one underscore
 * (`API Gateway` → `AWS_ENDPOINT_URL_API_GATEWAY`, `CloudWatch Logs` → `AWS_ENDPOINT_URL_CLOUDWATCH_LOGS`). */
export function endpointVar(sdkId: string): string {
  return `AWS_ENDPOINT_URL_${sdkId.toUpperCase().replace(/[^A-Z0-9]+/g, "_")}`;
}

type Note = { detail: (c: { account: string; region: string }) => string; more?: ServiceUrl["more"] };

const NOTES: Record<string, Note> = {
  sqs: { detail: ({ account }) => `Queue URLs: /${account}/<queue>` },
  s3: { detail: () => "Path style: /<bucket>/<key>", more: { drawer: "s3", label: "S3 addressing" } },
  lambda: { detail: ({ region }) => `Function URLs: /_aws/lambda-url/${region}/<id>/`, more: { drawer: "invoke", label: "Invoke URLs" } },
  stepfunctions: { detail: () => "Sync calls need disableHostPrefix", more: { drawer: "invoke", label: "Sync calls" } },
  apigateway: { detail: () => "Deployed APIs: /_aws/execute-api/<id>/<stage>/", more: { drawer: "invoke", label: "Invoke URLs" } },
};

export function serviceUrls(services: readonly ServiceRow[], endpoint: string, account: string, region: string): ServiceUrl[] {
  return services
    .filter((s) => s.served)
    .map((s) => {
      const note = NOTES[s.name];
      return {
        name: s.name,
        title: s.title,
        address: hostOf(endpoint),
        vars: s.apis.filter((a) => a.sdk_id !== "ApiGatewayManagementApi").map((a) => endpointVar(a.sdk_id)),
        detail: note?.detail({ account, region }) ?? null,
        more: note?.more ?? null,
      };
    });
}

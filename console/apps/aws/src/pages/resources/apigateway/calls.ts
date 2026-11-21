// How the call panels show an API Gateway call. The console's input names a member as the wire does; for PutIntegration the wire's names
// clash (the method is `requestHttpMethod`, the integration's own verb is `httpMethod`), while the SDK and the boards say `httpMethod` and
// `integrationHttpMethod`. The AWS CLI and boto3 tabs are rendered by the server and are right already.
export function sdkShown(operation: string, input: unknown): unknown {
  if (operation !== "PutIntegration" || typeof input !== "object" || input === null) return input;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(input as Record<string, unknown>)) {
    if (k === "requestHttpMethod") out.httpMethod = v;
    else if (k === "httpMethod") out.integrationHttpMethod = v;
    else out[k] = v;
  }
  // The method comes before the rest, as the boards write it.
  const { restApiId, resourceId, httpMethod, ...rest } = out;
  return { restApiId, resourceId, httpMethod, ...rest };
}

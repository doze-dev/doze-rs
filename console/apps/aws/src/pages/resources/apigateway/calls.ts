// How the call panels show an API Gateway call. The console's input names a member as the wire does. For PutIntegration the wire's names
// clash (the method is `requestHttpMethod`, the integration's own verb is `httpMethod`), while the SDK and the boards say `httpMethod` and
// `integrationHttpMethod`. The HTTP APIs' service takes its path members capitalised on the wire (`ApiId`, `RouteId`) and its bodies in camelCase,
// where the SDKs and the boards write every member in camelCase (`apiId`). The AWS CLI and boto3 tabs are rendered by the server and are right already.
const lowerFirst = (k: string): string => k.charAt(0).toLowerCase() + k.slice(1);

export function sdkShown(operation: string, input: unknown): unknown {
  if (typeof input !== "object" || input === null || Array.isArray(input)) return input;
  const cased: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(input as Record<string, unknown>)) cased[/^[A-Z]/.test(k) ? lowerFirst(k) : k] = v;
  if (operation !== "PutIntegration") return cased;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(cased)) {
    if (k === "requestHttpMethod") out.httpMethod = v;
    else if (k === "httpMethod") out.integrationHttpMethod = v;
    else out[k] = v;
  }
  // The method comes before the rest, as the boards write it.
  const { restApiId, resourceId, httpMethod, ...rest } = out;
  return { restApiId, resourceId, httpMethod, ...rest };
}

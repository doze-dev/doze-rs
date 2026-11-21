// The forms that add a response to a method, and the call each makes: PutMethodResponse (the contract with the caller) and
// PutIntegrationResponse (how a backend's answer becomes one of them).

export type MethodResponseForm = { status: string; headers: string; contentType: string; model: string };
export type IntegrationResponseForm = { status: string; pattern: string; headers: { name: string; value: string }[]; template: string; contentType: string };

const statusOk = (s: string) => /^[1-5]\d\d$/.test(s);

export function methodResponseProblem(f: MethodResponseForm, have: readonly string[]): string | null {
  if (!statusOk(f.status)) return "The HTTP status code is three digits, such as 200.";
  if (have.includes(f.status)) return `The method already has a ${f.status} response; delete it to make a new one.`;
  return null;
}

export function methodResponseInput(base: { api: string; resource: string; method: string }, f: MethodResponseForm) {
  const headers = f.headers.split(",").map((h) => h.trim()).filter(Boolean);
  return {
    restApiId: base.api,
    resourceId: base.resource,
    httpMethod: base.method,
    statusCode: f.status || "<status>",
    ...(headers.length > 0 ? { responseParameters: Object.fromEntries(headers.map((h) => [`method.response.header.${h}`, false])) } : {}),
    ...(f.contentType.trim() ? { responseModels: { [f.contentType.trim()]: f.model || "Empty" } } : {}),
  };
}

export function integrationResponseProblem(f: IntegrationResponseForm, have: readonly string[]): string | null {
  if (!statusOk(f.status)) return "The method response status is three digits, such as 200.";
  if (have.includes(f.status)) return `The integration already has a ${f.status} response; delete it to make a new one.`;
  if (f.pattern !== "") {
    try { new RegExp(f.pattern); } catch { return "The HTTP status regex is not a regular expression."; }
  }
  const bad = f.headers.find((h) => (h.name === "") !== (h.value === ""));
  return bad ? "A header mapping needs a name and a value." : null;
}

export function integrationResponseInput(base: { api: string; resource: string; method: string }, f: IntegrationResponseForm) {
  const headers = f.headers.filter((h) => h.name !== "" && h.value !== "");
  return {
    restApiId: base.api,
    resourceId: base.resource,
    httpMethod: base.method,
    statusCode: f.status || "<status>",
    ...(f.pattern ? { selectionPattern: f.pattern } : {}),
    ...(headers.length > 0 ? { responseParameters: Object.fromEntries(headers.map((h) => [`method.response.header.${h.name}`, h.value])) } : {}),
    ...(f.template.trim() ? { responseTemplates: { [f.contentType || "application/json"]: f.template } } : {}),
  };
}

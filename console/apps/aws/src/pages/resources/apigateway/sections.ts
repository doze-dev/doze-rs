// The sections of an API's page, in the order of the chips between them.
export const REST_SECTIONS = [{ id: "resources", label: "Resources" }, { id: "stages", label: "Stages" }, { id: "authorizers", label: "Authorizers" }] as const;
export const HTTP_SECTIONS = [{ id: "routes", label: "Routes" }, { id: "integrations", label: "Integrations" }, { id: "authorization", label: "Authorization" }, { id: "cors", label: "CORS" }, { id: "stages", label: "Stages" }] as const;

/** The sections of an HTTP or a WebSocket API's page: a WebSocket API is not called from a browser page of another origin, so it has no CORS. */
export const httpSections = (websocket: boolean) => (websocket ? HTTP_SECTIONS.filter((x) => x.id !== "cors") : HTTP_SECTIONS);

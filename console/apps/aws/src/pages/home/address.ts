// The address an app reaches this doze-aws at. A doze-aws on its own has one: the IP and port it listens on, and every service
// answers there. The .doze name belongs to the doze app that hosts doze-aws as a module; doze-aws does not know it, so this
// console never draws one.
export const ADDRESS = { id: "ip", label: "IP and port" } as const;

/** What the address is: `IP and port` for an address of numbers (or localhost), `.doze name` for a name (under the doze app, with names
 * on, the address doze-aws is told to say is the name's). */
export function addressLabel(endpoint: string | undefined): string {
  const host = hostOf(endpoint ?? "").replace(/:\d+$/, "").replace(/^\[|\]$/g, "");
  if (host === "" || host === "localhost" || /^\d{1,3}(\.\d{1,3}){3}$/.test(host) || host.includes(":")) return ADDRESS.label;
  return ".doze name";
}

/** `127.0.0.1:4566` for `http://127.0.0.1:4566`. */
export function hostOf(endpoint: string): string {
  return endpoint.replace(/^https?:\/\//, "").replace(/\/$/, "");
}

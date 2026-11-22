// The address an app reaches this doze-aws at. A doze-aws on its own has one: the IP and port it listens on, and every service
// answers there. The .doze name belongs to the doze app that hosts doze-aws as a module; doze-aws does not know it, so this
// console never draws one.
export const ADDRESS = { id: "ip", label: "IP and port" } as const;

/** `127.0.0.1:4566` for `http://127.0.0.1:4566`. */
export function hostOf(endpoint: string): string {
  return endpoint.replace(/^https?:\/\//, "").replace(/\/$/, "");
}

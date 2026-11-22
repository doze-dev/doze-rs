// "The URL shapes you will see": what the instance hands back, from the address a request arrived on. A URL is made from that
// address, so these are the shapes for this one. Pure.
import { hostOf } from "./address";

export type Shape = { what: string; urls: { url: string; note?: string }[] };

/** The host a Function URL is minted at: an IP is `localhost`, which browsers and curl resolve to this machine. */
function lambdaHost(host: string): string {
  const [name = "", port = ""] = host.split(":");
  const ip = /^[\d.]+$/.test(name) || name === "[::1]" || name === "localhost";
  return `${ip ? "localhost" : name}${port ? `:${port}` : ""}`;
}

export function urlShapes(endpoint: string, account: string, region: string): Shape[] {
  const host = hostOf(endpoint);
  return [
    { what: "Queue URLs", urls: [{ url: `http://${host}/${account}/pick-lists` }] },
    { what: "S3 objects (path style)", urls: [{ url: `http://${host}/product-images/sku-FV-TOM-V.jpg` }] },
    { what: "Deployed APIs", urls: [{ url: `http://${host}/_aws/execute-api/<id>/<stage>/` }] },
    { what: "Lambda function URLs", urls: [{ url: `http://<id>.lambda-url.${region}.${lambdaHost(host)}/`, note: "as created" }, { url: `http://${host}/_aws/lambda-url/${region}/<id>/`, note: "by path" }] },
  ];
}

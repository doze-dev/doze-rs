// Which product's theme the gallery wears. A theme is one stylesheet (packages/ui/themes/<product>.css); switching swaps the
// <link>, the new one in before the old one out, so the page is never without its roles.
import { useSyncExternalStore } from "react";
import awsUrl from "@doze/ui/themes/aws.css?url";
import kafkaUrl from "@doze/ui/themes/kafka.css?url";

export const PRODUCTS = [
  { value: "kafka", label: "doze-kafka", mark: "/brand/doze-kafka.svg", url: kafkaUrl },
  { value: "aws", label: "doze-aws", mark: "/brand/doze-aws.svg", url: awsUrl },
] as const;
export type Product = (typeof PRODUCTS)[number]["value"];

const KEY = "doze-gallery.product";
const listeners = new Set<() => void>();
const isProduct = (v: unknown): v is Product => v === "kafka" || v === "aws";

function stored(): Product {
  try {
    const v = localStorage.getItem(KEY);
    return isProduct(v) ? v : "kafka";
  } catch {
    return "kafka";
  }
}

let current: Product = stored();
let link: HTMLLinkElement | null = null;

function load(product: Product): Promise<void> {
  return new Promise((resolve) => {
    const next = document.createElement("link");
    next.rel = "stylesheet";
    next.href = PRODUCTS.find((p) => p.value === product)!.url;
    next.dataset.theme = product;
    const done = () => {
      link?.remove();
      link = next;
      document.documentElement.dataset.product = product;
      resolve();
    };
    next.addEventListener("load", done, { once: true });
    next.addEventListener("error", done, { once: true });
    document.head.append(next);
  });
}

/** Before the first render: the product named in the address (`?theme=aws`) or the one chosen last, its stylesheet loaded. */
export async function startProduct(fromAddress: string | null): Promise<void> {
  if (isProduct(fromAddress)) current = fromAddress;
  await load(current);
}

export async function setProduct(product: Product): Promise<void> {
  if (product === current) return;
  await load(product);
  current = product;
  try {
    localStorage.setItem(KEY, product);
  } catch {
    // private mode: the choice lasts this page only
  }
  for (const l of listeners) l();
}

export function useProduct(): Product {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => current,
    () => "kafka" as Product,
  );
}

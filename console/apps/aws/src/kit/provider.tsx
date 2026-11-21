import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";

/** The kit's reads are TanStack queries: a page that is not one of the console's (the gallery) wraps itself in this. */
export function KitProvider({ children }: { children: React.ReactNode }) {
  const [client] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } }));
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

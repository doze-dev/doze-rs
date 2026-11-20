import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "@doze/ui/components/ui/sonner";
import { TooltipProvider } from "@doze/ui/components/ui/tooltip";
import { startTheme } from "@doze/ui/lib/theme";
import { applyDensity } from "./density";
import { THEME_KEY } from "./nav";
import { router } from "./router";
import "./index.css";

// Before the first render: light, dark, or the OS's, as chosen in this browser.
startTheme({ storageKey: THEME_KEY });
applyDensity();

const queries = new QueryClient({
  defaultOptions: {
    queries: {
      // A page is a snapshot: nothing refetches by itself; the RefreshControl decides (its button, its interval).
      staleTime: Infinity,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      retry: 1,
    },
  },
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queries}>
      <TooltipProvider delayDuration={300}>
        <RouterProvider router={router} />
        <Toaster position="bottom-right" />
      </TooltipProvider>
    </QueryClientProvider>
  </StrictMode>,
);

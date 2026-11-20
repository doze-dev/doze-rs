import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Toaster } from "@doze/ui/components/ui/sonner";
import { TooltipProvider } from "@doze/ui/components/ui/tooltip";
import { setTheme, startTheme } from "@doze/ui/lib/theme";
import { startProduct } from "./product";
import { router } from "./router";
import "./index.css";

// The address can name the look (`?theme=aws&mode=dark`): how the Playwright run and the screenshots ask for each of the
// four. Both are taken once, kept as the choice, and removed from the address.
const asked = new URLSearchParams(location.search);
const mode = asked.get("mode");
startTheme({ storageKey: "doze-gallery.theme" });
if (mode === "light" || mode === "dark" || mode === "system") setTheme(mode);
await startProduct(asked.get("theme"));
if (asked.has("theme") || asked.has("mode")) {
  asked.delete("theme");
  asked.delete("mode");
  const rest = asked.toString();
  history.replaceState(null, "", `${location.pathname}${rest ? `?${rest}` : ""}${location.hash}`);
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TooltipProvider delayDuration={300}>
      <RouterProvider router={router} />
      <Toaster position="bottom-right" />
    </TooltipProvider>
  </StrictMode>,
);

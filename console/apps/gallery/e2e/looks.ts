// The four looks every page is rendered under, and the gallery's pages. Shared by the tests and the screenshot script.
export const LOOKS = [
  { theme: "kafka", mode: "light" },
  { theme: "kafka", mode: "dark" },
  { theme: "aws", mode: "light" },
  { theme: "aws", mode: "dark" },
] as const;
export type Look = (typeof LOOKS)[number];

export const PAGES = [
  { path: "/", name: "foundations", title: "Foundations" },
  { path: "/actions", name: "actions", title: "Actions" },
  { path: "/forms", name: "forms", title: "Forms" },
  { path: "/overlays", name: "overlays", title: "Overlays" },
  { path: "/data", name: "data", title: "Data" },
  { path: "/states", name: "states", title: "States" },
  { path: "/frame", name: "frame", title: "Frame" },
  { path: "/create", name: "create", title: "Create" },
  { path: "/fields", name: "fields", title: "Fields" },
  { path: "/resource", name: "resource", title: "Resource" },
  { path: "/lists", name: "lists", title: "Lists" },
  { path: "/charts", name: "charts", title: "Charts" },
] as const;

/** The address of a page under a look (the gallery takes both from the address, once, and keeps them). */
export const at = (path: string, look: Look) => `${path}?theme=${look.theme}&mode=${look.mode}`;

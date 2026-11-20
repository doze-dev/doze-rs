import { FlaskConicalIcon, HomeIcon, KeyRoundIcon, LayoutGridIcon, type LucideIcon, RadioIcon, SlidersHorizontalIcon } from "lucide-react";

/** Where this console keeps the theme choice and the palette's recents in this browser (its own keys: doze-kafka's console on
 * another port of the same host must not share them). */
export const THEME_KEY = "doze-aws.theme";
export const RECENTS_KEY = "doze-aws.recents";

export type SpacePath = "/" | "/live" | "/resources" | "/access" | "/lab" | "/settings";
export type Space = { label: string; to: SpacePath; icon: LucideIcon; keywords: string };

/** The six spaces, in the design's order. A service never adds one: it joins Resources. */
export const SPACES: Space[] = [
  { label: "Home", to: "/", icon: HomeIcon, keywords: "endpoint account region version services connect" },
  { label: "Live", to: "/live", icon: RadioIcon, keywords: "calls requests wire tail stream" },
  { label: "Resources", to: "/resources", icon: LayoutGridIcon, keywords: "queues buckets tables functions topics" },
  { label: "Access", to: "/access", icon: KeyRoundIcon, keywords: "iam users roles policies keys" },
  { label: "Lab", to: "/lab", icon: FlaskConicalIcon, keywords: "faults rehearse experiments" },
  { label: "Settings", to: "/settings", icon: SlidersHorizontalIcon, keywords: "config flags options" },
];

/** Whether `path` is in the space (Home only at `/`). */
export function isActive(space: { to: string }, path: string): boolean {
  if (space.to === "/") return path === "/";
  return path === space.to || path.startsWith(`${space.to}/`);
}

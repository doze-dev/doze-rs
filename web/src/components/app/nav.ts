import { ActivityIcon, BoxesIcon, FlaskConicalIcon, HomeIcon, type LucideIcon, PlugIcon, Rows3Icon, SettingsIcon, ShieldIcon, SwatchBookIcon, UsersIcon } from "lucide-react";

export type NavItem = { label: string; to: string; icon: LucideIcon; count?: "topics" | "groups"; keywords?: string };

/** The sidebar, in the direction's order: Home, Live, the resources, Access, Lab, Settings. */
export const NAV: NavItem[] = [
  { label: "Home", to: "/", icon: HomeIcon },
  { label: "Live", to: "/live", icon: ActivityIcon, keywords: "wire requests tape" },
  { label: "Topics", to: "/topics", icon: Rows3Icon, count: "topics" },
  { label: "Groups", to: "/groups", icon: UsersIcon, count: "groups", keywords: "consumers lag" },
  { label: "Access", to: "/access", icon: ShieldIcon, keywords: "acl sasl tls security" },
  { label: "Lab", to: "/lab", icon: FlaskConicalIcon, keywords: "faults rehearse" },
  { label: "Settings", to: "/settings", icon: SettingsIcon, keywords: "config flags" },
];

/** Every page the palette offers. */
export const PAGES: NavItem[] = [
  ...NAV,
  { label: "Connect an app", to: "/connect", icon: PlugIcon, keywords: "snippets client bootstrap" },
  { label: "Component gallery", to: "/gallery", icon: SwatchBookIcon, keywords: "components ui" },
];

export const RESOURCE_ICON: LucideIcon = BoxesIcon;

/** Whether `path` is the item's place (Home only at `/`). */
export function isActive(item: { to: string }, path: string): boolean {
  if (item.to === "/") return path === "/" || path === "/connect";
  return path === item.to || path.startsWith(`${item.to}/`);
}

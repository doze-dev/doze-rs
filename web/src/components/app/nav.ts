import { ActivityIcon, BoxesIcon, RadioIcon, FileJson2Icon, FlaskConicalIcon, HardDriveIcon, HomeIcon, type LucideIcon, PlugIcon, Rows3Icon, SettingsIcon, ShieldIcon, SwatchBookIcon, UsersIcon } from "lucide-react";

export type NavItem = { label: string; to: string; icon: LucideIcon; count?: "topics" | "groups" | "schemas"; keywords?: string; disk?: true };

/** The sidebar, in the direction's order: Home, Live and its Live tail of records, the resources, what they take on disk, Access, Lab, Settings. */
export const NAV: NavItem[] = [
  { label: "Home", to: "/", icon: HomeIcon },
  { label: "Live", to: "/live", icon: ActivityIcon, keywords: "wire requests tape" },
  { label: "Live tail", to: "/tail", icon: RadioIcon, keywords: "tail follow stream messages events consume watch" },
  { label: "Topics", to: "/topics", icon: Rows3Icon, count: "topics" },
  { label: "Groups", to: "/groups", icon: UsersIcon, count: "groups", keywords: "consumers lag" },
  { label: "Schemas", to: "/schemas", icon: FileJson2Icon, count: "schemas", keywords: "registry avro protobuf json subjects" },
  { label: "Data", to: "/data", icon: HardDriveIcon, keywords: "disk storage size space empty erase truncate retention", disk: true },
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

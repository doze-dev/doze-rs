import { Link, useRouterState } from "@tanstack/react-router";
import { ExternalLinkIcon } from "lucide-react";
import { useShell } from "@/api/queries";
import logo from "@/assets/doze-kafka.svg";
import { Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent, SidebarHeader, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarRail } from "@/components/ui/sidebar";
import { number } from "@/lib/format";
import { isActive, NAV } from "./nav";
import { ThemeMenu } from "./theme-menu";

export function AppSidebar() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const shell = useShell();
  return (
    <Sidebar collapsible="icon" data-testid="sidebar">
      <SidebarHeader className="h-[52px] justify-center border-b border-sidebar-border px-4 group-data-[collapsible=icon]:px-2">
        <Link to="/" className="flex items-center gap-2.5 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50" aria-label="doze-kafka, Home">
          <img src={logo} alt="" width={28} height={28} className="size-7 shrink-0" draggable={false} />
          <span className="text-[16px] font-semibold tracking-[-0.01em] text-bright group-data-[collapsible=icon]:hidden">doze-kafka</span>
        </Link>
      </SidebarHeader>
      <SidebarContent className="pt-3">
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu className="gap-0.5">
              {NAV.map((item) => {
                const active = isActive(item, path);
                const count = item.count && shell.data ? shell.data[item.count] : undefined;
                return (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton asChild isActive={active} tooltip={item.label} className="h-9 text-[14px] data-[active=true]:font-medium [&>svg]:text-muted-foreground data-[active=true]:[&>svg]:text-link">
                      <Link to={item.to} aria-current={active ? "page" : undefined}>
                        <item.icon />
                        <span>{item.label}</span>
                      </Link>
                    </SidebarMenuButton>
                    {count !== undefined && <SidebarMenuBadge className="font-normal text-muted-foreground tabular-nums">{number(count)}</SidebarMenuBadge>}
                  </SidebarMenuItem>
                );
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="pb-4">
        <SidebarMenu>
          <SidebarMenuItem>
            <ThemeMenu />
          </SidebarMenuItem>
          <SidebarMenuItem>
            <SidebarMenuButton asChild tooltip="Classic console" className="text-[13px] text-muted-foreground">
              <a href={shell.data?.classic ?? "/classic/"} data-testid="classic-link">
                <ExternalLinkIcon />
                <span>Classic console</span>
              </a>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}

import { Link, Outlet, useNavigate, useRouterState } from "@tanstack/react-router";
import { ComponentIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { CommandPalette, type PaletteGroup } from "@doze/ui/components/app/command-palette";
import { ThemeSwitch } from "@doze/ui/components/app/theme-menu";
import { TopBar } from "@doze/ui/components/app/top-bar";
import { Sidebar, SidebarContent, SidebarGroup, SidebarGroupContent, SidebarHeader, SidebarInset, SidebarMenu, SidebarMenuBadge, SidebarMenuButton, SidebarMenuItem, SidebarProvider, SidebarRail } from "@doze/ui/components/ui/sidebar";
import { ToggleGroup, ToggleGroupItem } from "@doze/ui/components/ui/toggle-group";
import { useSidebarOpen } from "@doze/ui/lib/sidebar-state";
import { PAGES } from "./nav";
import { PRODUCTS, type Product, setProduct, useProduct } from "./product";

/** Which product's theme the gallery wears: the one control here that no console has. */
function ProductSwitch() {
  const product = useProduct();
  return (
    <ToggleGroup type="single" variant="outline" size="sm" value={product} onValueChange={(v) => v && void setProduct(v as Product)} aria-label="Product theme" data-testid="product-switch">
      {PRODUCTS.map((p) => (
        <ToggleGroupItem key={p.value} value={p.value} className="px-3 text-[12.5px]" data-testid={`product-${p.value}`}>
          {p.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

/** The gallery's frame, made of the shared frame pieces (Sidebar, TopBar, CommandPalette, ThemeSwitch): it is their specimen. */
export function Shell() {
  const [open, setOpen] = useSidebarOpen();
  const [palette, setPalette] = useState(false);
  const product = useProduct();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const navigate = useNavigate();
  const brand = PRODUCTS.find((p) => p.value === product)!;
  const groups = useMemo<PaletteGroup[]>(
    () => [
      { heading: "Pages", items: PAGES.map((p) => ({ label: p.label, to: p.to, kind: "page", icon: p.icon, keywords: p.keywords })) },
      { heading: "Components", separated: true, items: PAGES.flatMap((p) => p.holds.map((name) => ({ label: name, to: `${p.to}#${name}`, kind: "component", icon: ComponentIcon, mono: true }))) },
    ],
    [],
  );
  return (
    <SidebarProvider open={open} onOpenChange={setOpen}>
      <Sidebar collapsible="icon" data-testid="sidebar">
        <SidebarHeader className="h-[52px] justify-center border-b border-sidebar-border px-4 group-data-[collapsible=icon]:px-2">
          <Link to="/" className="flex items-center gap-2.5 rounded-md outline-none focus-visible:ring-2 focus-visible:ring-ring/50" aria-label={`${brand.label} components, Foundations`}>
            <img src={brand.mark} alt="" width={32} height={32} className="size-8 shrink-0 rounded-[8px]" />
            <span className="flex min-w-0 flex-col leading-none group-data-[collapsible=icon]:hidden">
              <span className="font-display text-[17px] font-semibold tracking-[-0.01em] text-bright">{brand.label}</span>
              <span className="mt-1 text-[11.5px] text-muted-foreground">components</span>
            </span>
          </Link>
        </SidebarHeader>
        <SidebarContent className="pt-3">
          <SidebarGroup>
            <SidebarGroupContent>
              <SidebarMenu className="gap-0.5">
                {PAGES.map((item) => {
                  const active = item.to === path;
                  return (
                    <SidebarMenuItem key={item.to}>
                      <SidebarMenuButton asChild isActive={active} tooltip={item.label} className="h-9 text-[14px] data-[active=true]:font-medium [&>svg]:text-muted-foreground data-[active=true]:[&>svg]:text-link">
                        <Link to={item.to} aria-current={active ? "page" : undefined}>
                          <item.icon />
                          <span>{item.label}</span>
                        </Link>
                      </SidebarMenuButton>
                      <SidebarMenuBadge className="font-normal text-muted-foreground tabular-nums">{item.holds.length}</SidebarMenuBadge>
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>
        <SidebarRail />
      </Sidebar>
      <SidebarInset className="min-w-0">
        <TopBar onPalette={() => setPalette(true)} paletteLabel="Jump to a component">
          <ProductSwitch />
          <ThemeSwitch />
        </TopBar>
        {/* Keyed by the product: a page reads the theme's density once (a table's row height), so it is drawn again under the other. */}
        <main id="main" tabIndex={-1} key={product} className="mx-auto w-full max-w-[1180px] px-6 pt-8 pb-24 outline-none md:px-10" data-testid="page">
          <Outlet />
        </main>
      </SidebarInset>
      <CommandPalette
        open={palette}
        onOpenChange={setPalette}
        returnFocus={() => document.querySelector<HTMLElement>("[data-testid=palette-trigger]")}
        groups={groups}
        onGo={(item) => void navigate({ to: item.to.split("#")[0] })}
        title="Jump to a component"
        placeholder="Jump to a page or a component…"
        recentsKey="doze-gallery.recents"
      />
    </SidebarProvider>
  );
}

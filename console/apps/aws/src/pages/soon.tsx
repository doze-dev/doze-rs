import { Link } from "@tanstack/react-router";
import { FlaskConicalIcon, KeyRoundIcon, LayoutGridIcon, type LucideIcon, SlidersHorizontalIcon } from "lucide-react";
import { EmptyState } from "@doze/ui/components/app/empty-state";
import { PageHeader } from "@doze/ui/components/app/page-header";
import { Button } from "@doze/ui/components/ui/button";

/** A space of the design that this build does not have yet: it says so, in one line says what will be here, and shows
 * nothing made up (no sample table, no example data). */
function NotBuilt({ title, icon, will, testId }: { title: string; icon: LucideIcon; /** One line on what the space will hold. */ will: string; testId: string }) {
  return (
    <>
      <PageHeader title={title} />
      <div data-testid={testId}>
        <EmptyState
          icon={icon}
          title={`${title} is not built yet`}
          action={
            <Button asChild variant="outline" className="bg-card">
              <Link to="/live">Open Live</Link>
            </Button>
          }
        >
          {will}
        </EmptyState>
      </div>
    </>
  );
}

export const ResourcesPage = () => <NotBuilt testId="not-built-resources" title="Resources" icon={LayoutGridIcon} will="What each service holds will be listed here: queues, buckets, tables, functions and the rest, each opening to its own page." />;
export const AccessPage = () => <NotBuilt testId="not-built-access" title="Access" icon={KeyRoundIcon} will="IAM will be here: users, roles and policies, and the calls a policy refused or would have refused." />;
export const LabPage = () => <NotBuilt testId="not-built-lab" title="Lab" icon={FlaskConicalIcon} will="Rehearsals will be here: making a service slow or failing on purpose, to see what your app does." />;
export const SettingsPage = () => <NotBuilt testId="not-built-settings" title="Settings" icon={SlidersHorizontalIcon} will="How this doze-aws was started will be here: every flag, its value, and where the value came from." />;

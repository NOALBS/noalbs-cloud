import { Link } from "@tanstack/react-router";
import { Headset } from "lucide-react";
import type { Workspace } from "@/api/types";
import { BreakGlassDialog } from "@/components/support/break-glass-dialog";
import { Button } from "@/components/ui/button";

/** Staff opening a workspace that hasn't let NOALBS support in. */
export function SupportGate({ workspace }: { workspace: Workspace }) {
  return (
    <main className="flex min-h-dvh items-center justify-center px-4 py-10">
      <div className="flex w-full max-w-md flex-col gap-5 rounded-2xl border bg-card p-6">
        <Headset className="size-6 text-muted-foreground" />
        <div className="flex flex-col gap-2">
          <h1 className="text-xl font-bold tracking-tight">
            {workspace.name} hasn’t let NOALBS support in
          </h1>
          <p className="text-sm text-muted-foreground">
            Ask one of its owners or admins to allow support access under Settings → General →
            NOALBS support access. They choose whether you may only look or also make changes, and
            for how long.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button asChild variant="outline">
            <Link to="/admin/workspaces/$workspace" params={{ workspace: workspace.id }}>
              Back to Admin
            </Link>
          </Button>
          <BreakGlassDialog workspaceId={workspace.id} workspaceName={workspace.name}>
            <Button variant="ghost" className="text-destructive-foreground">
              Emergency access…
            </Button>
          </BreakGlassDialog>
        </div>
      </div>
    </main>
  );
}

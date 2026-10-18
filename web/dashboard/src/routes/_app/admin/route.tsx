import { createFileRoute, Outlet, redirect } from "@tanstack/react-router";
import { AdminShell } from "@/components/admin/admin-shell";

/** The staff admin section. Everyone else goes back to the dashboard. */
export const Route = createFileRoute("/_app/admin")({
  beforeLoad: ({ context }) => {
    if (!context.me.staff) throw redirect({ to: "/" });
  },
  component: AdminLayout,
});

function AdminLayout() {
  return (
    <AdminShell>
      <Outlet />
    </AdminShell>
  );
}

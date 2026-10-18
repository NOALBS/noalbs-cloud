import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { AlertTriangle } from "lucide-react";
import { adminProductsQuery } from "@/api/queries";
import { AddonProduct, NewProductButton, PlanProduct } from "@/components/admin/products";
import { Page } from "@/components/page";
import { Skeleton } from "@/components/ui/skeleton";

export const Route = createFileRoute("/_app/admin/products")({
  component: ProductsPage,
});

function ProductsPage() {
  const products = useQuery(adminProductsQuery);
  const data = products.data;

  return (
    <Page
      title="Products"
      description="Plans and add-ons for sale. Prices are created in Stripe; changing one keeps existing subscribers on their price."
      className="max-w-6xl"
    >
      {data && !data.billing_enabled && (
        <p className="flex items-start gap-2 rounded-xl border border-low/40 bg-low/10 px-3.5 py-2.5 text-sm">
          <AlertTriangle className="mt-0.5 size-4 shrink-0 text-low" />
          Stripe isn’t configured, so nothing can be priced or sold. Plans and add-ons can still be
          edited.
        </p>
      )}
      {!data ? (
        <Skeleton className="h-64" />
      ) : (
        <>
          <section aria-label="Plans" className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <h2 className="text-lg font-semibold">Plans</h2>
              <NewProductButton kind="plan" />
            </div>
            <div className="grid gap-4 lg:grid-cols-2">
              {data.plans.map((plan) => (
                <PlanProduct
                  key={plan.code}
                  plan={plan}
                  currency={data.currency}
                  billingEnabled={data.billing_enabled}
                />
              ))}
            </div>
          </section>
          <section aria-label="Add-ons" className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <h2 className="text-lg font-semibold">Add-ons</h2>
              <NewProductButton kind="addon" />
            </div>
            <div className="grid gap-4 lg:grid-cols-2">
              {data.addons.map((addon) => (
                <AddonProduct
                  key={addon.code}
                  addon={addon}
                  currency={data.currency}
                  billingEnabled={data.billing_enabled}
                />
              ))}
            </div>
          </section>
        </>
      )}
    </Page>
  );
}

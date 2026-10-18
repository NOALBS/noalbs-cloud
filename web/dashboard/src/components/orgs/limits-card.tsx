import type { Organization } from "@/api/types";
import { Section } from "@/components/page";
import { UsageBar } from "@/components/usage-bar";
import { otherLimits, poolUsage } from "@/lib/organizations";

/**
 * Everything the organization may use: the switcher pool (shared by all its
 * clients) and the limits each client or switcher gets. NOALBS sets them.
 */
export function LimitsCard({ organization }: { organization: Organization }) {
  const pool = poolUsage(organization);

  return (
    <Section
      title="Your limits"
      description="Set by NOALBS for your organization. Get in touch to change them."
    >
      <div className="flex flex-col gap-5 px-5 pb-5">
        <div className="flex flex-col gap-2">
          <UsageBar label="Switchers in the pool" used={pool.used} max={pool.max} />
          <p className="text-xs text-muted-foreground">
            Shared by all your clients. A client can be given a share of it.
          </p>
          {pool.over > 0 && (
            <p className="text-sm text-low">
              {pool.over} switcher{pool.over === 1 ? " is" : "s are"} over the pool. They keep
              running, but no new ones can be added until usage is under it.
            </p>
          )}
        </div>
        <dl className="grid grid-cols-2 gap-x-4 gap-y-4 border-t border-divider pt-4 lg:grid-cols-4">
          {otherLimits(organization.limits).map((limit) => (
            <div key={limit.label} className="flex flex-col gap-0.5">
              <dt className="text-xs text-muted-foreground">{limit.label}</dt>
              <dd className="text-sm font-semibold">{limit.value}</dd>
              <dd className="text-xs text-muted-foreground">{limit.hint}</dd>
            </div>
          ))}
        </dl>
      </div>
    </Section>
  );
}

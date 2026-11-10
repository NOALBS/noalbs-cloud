import { plural, t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { AlertTriangle, Check, CreditCard, HeartHandshake, Info } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { addonsQuery, plansQuery, queryKeys, workspacePlanQuery } from "@/api/queries";
import type { BillingInterval, Plan, WorkspacePlan } from "@/api/types";
import { AddonCard } from "@/components/billing/addon-card";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { ManagedPlan } from "@/components/orgs/managed-plan";
import { Page, Section } from "@/components/page";
import { SegmentedControl } from "@/components/segmented-control";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { UsageBar } from "@/components/usage-bar";
import { useBillingChange, useStripeRedirect } from "@/hooks/use-billing";
import { useWorkspace } from "@/hooks/use-workspace";
import {
  describeSubscription,
  FREE_PLAN,
  formatPrice,
  intervalWord,
  missingCardReminder,
  priceFor,
  prorate,
  showsAddon,
  yearlySaving,
} from "@/lib/billing";
import { currentLocale } from "@/lib/locale";
import { cn } from "@/lib/utils";

interface PlanSearch {
  /** Set by Stripe Checkout's return URL. */
  checkout?: "success" | "canceled";
}

export const Route = createFileRoute("/_app/$workspace/settings/plan")({
  validateSearch: (search: Record<string, unknown>): PlanSearch => ({
    checkout:
      search.checkout === "success" || search.checkout === "canceled" ? search.checkout : undefined,
  }),
  component: PlanRoute,
});

/** A client workspace has nothing to buy: its organization pays. */
function PlanRoute() {
  const { workspace } = useWorkspace();
  return workspace.managed_by ? <ManagedPlan workspace={workspace} /> : <PlanPage />;
}

/** How long to wait for Stripe's webhook after a checkout before giving up polling. */
const WEBHOOK_WAIT_MS = 30_000;

function formatDay(date: string | null | undefined): string {
  return date
    ? new Date(date).toLocaleDateString(currentLocale(), {
        day: "numeric",
        month: "long",
        year: "numeric",
      })
    : t`the end of the period`;
}

function PlanPage() {
  const { workspace, me } = useWorkspace();
  const search = Route.useSearch();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  // After a checkout, the plan changes once Stripe's webhook arrives: poll until then.
  const [waitingSince, setWaitingSince] = useState<number | null>(null);
  const current = useQuery({
    ...workspacePlanQuery(workspace.id),
    refetchInterval: () =>
      waitingSince != null && Date.now() - waitingSince < WEBHOOK_WAIT_MS ? 2000 : false,
  });
  const plans = useQuery(plansQuery);
  const addons = useQuery(addonsQuery);
  const portal = useStripeRedirect();
  const resume = useBillingChange(t`Your subscription continues`);
  const [interval, setBillingInterval] = useState<BillingInterval>("month");
  // Show prices for the period the workspace already pays by.
  const paidInterval = current.data?.subscription?.interval;
  useEffect(() => {
    if (paidInterval) setBillingInterval(paidInterval);
  }, [paidInterval]);

  useEffect(() => {
    if (!search.checkout) return;
    if (search.checkout === "success") {
      setWaitingSince(Date.now());
      toast.success(t`Thanks! Your payment went through.`);
    }
    void navigate({ to: ".", search: {}, replace: true });
  }, [search.checkout, navigate]);

  const subscribed = current.data?.subscription != null;
  useEffect(() => {
    if (waitingSince == null || !subscribed) return;
    setWaitingSince(null);
    toast.success(t`Your subscription is active.`);
    void queryClient.invalidateQueries({ queryKey: queryKeys.switchers(workspace.id) });
    void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
  }, [waitingSince, subscribed, queryClient, workspace.id]);

  const sold =
    (plans.data?.some((plan) => plan.prices.length > 0) ?? false) ||
    (addons.data?.some((addon) => addon.prices.length > 0) ?? false);
  const data = current.data;
  const subscription = data?.subscription;
  const currentPrice =
    subscription &&
    priceFor(
      plans.data?.find((plan) => plan.code === data.plan.code)?.prices ?? [],
      subscription.interval,
    );
  const status = subscription ? describeSubscription(subscription) : null;
  const manages = Boolean(data?.can_manage_billing && data.billing_enabled);
  const cardReminder = data?.can_manage_billing ? missingCardReminder(data) : null;
  const shownAddons = (addons.data ?? []).filter((addon) =>
    showsAddon(
      addon,
      data?.addons.find((owned) => owned.code === addon.code),
    ),
  );

  return (
    <Page title={t`Plan`} description={t`What this workspace can use.`}>
      <Section
        title={data ? t`${data.plan.name} plan` : t`Plan`}
        className="[&_h2]:text-primary sm:[&>div:first-child>div]:flex-row sm:[&>div:first-child>div]:items-center sm:[&>div:first-child>div]:gap-3"
        description={
          waitingSince != null ? (
            t`Confirming your payment… this takes a few seconds.`
          ) : status && !status.warning ? (
            <>
              {currentPrice && (
                <>
                  {t`${formatPrice(currentPrice.amount, currentPrice.currency)} per ${intervalWord(subscription.interval)}`}{" "}
                  ·{" "}
                </>
              )}
              {status.text}
            </>
          ) : undefined
        }
        actions={
          manages &&
          subscription && (
            <Button
              variant="outline"
              onClick={() => portal.mutate("portal")}
              disabled={portal.isPending}
            >
              <CreditCard />
              <Trans>Manage billing</Trans>
            </Button>
          )
        }
      >
        {status?.warning && (
          <div className="mx-5 mb-4 flex flex-wrap items-start gap-2 rounded-xl border border-low/40 bg-low/10 px-3.5 py-2.5 text-sm">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-low" />
            <span className="min-w-0 flex-1">{status.text}</span>
            {manages && subscription?.cancel_at_period_end && (
              <Button
                size="sm"
                variant="outline"
                disabled={resume.isPending}
                onClick={() => resume.mutate({ kind: "resume" })}
              >
                <Trans>Resume subscription</Trans>
              </Button>
            )}
          </div>
        )}
        {cardReminder && (
          <div className="mx-5 mb-4 flex items-start gap-2 rounded-xl border border-low/40 bg-low/10 px-3.5 py-2.5 text-sm">
            <AlertTriangle className="mt-0.5 size-4 shrink-0 text-low" />
            <span className="min-w-0 flex-1">{cardReminder}</span>
          </div>
        )}
        {subscription?.held_plan_code && subscription.held_plan_until && (
          <p className="mx-5 mb-4 text-sm text-muted-foreground">
            <Trans>
              Moving to Free on {formatDay(subscription.held_plan_until)}; {data?.plan.name} stays
              until then.
            </Trans>
          </p>
        )}
        <div className="grid gap-5 border-t border-divider px-5 py-4 sm:grid-cols-3">
          {data ? (
            <>
              <UsageBar
                label={t`Switchers`}
                used={data.usage.switchers}
                max={data.plan.max_switchers}
              />
              <UsageBar label={t`Members`} used={data.usage.members} max={data.plan.max_members} />
              <div className="flex flex-col gap-1.5">
                <span className="text-xs text-muted-foreground">
                  <Trans>History</Trans>
                </span>
                <span className="font-display text-sm font-bold tabular">
                  {plural(data.plan.history_days, { one: "# day", other: "# days" })}
                </span>
              </div>
            </>
          ) : (
            <Skeleton className="h-12 sm:col-span-2" />
          )}
        </div>
        {data &&
          data.plan.max_switchers != null &&
          data.usage.switchers > data.plan.max_switchers && (
            <p className="mx-5 mb-5 text-sm text-muted-foreground">
              <Trans>
                The newest switchers beyond {data.plan.max_switchers} are paused: they stay
                connected but don’t switch. Upgrade, or delete switchers you no longer need.
              </Trans>
            </p>
          )}
      </Section>

      {sold && (
        <div className="flex flex-wrap items-center gap-3">
          <SegmentedControl
            label={t`Billing period`}
            value={interval}
            onValueChange={setBillingInterval}
            options={[
              { value: "month", label: t`Monthly` },
              { value: "year", label: t`Yearly` },
            ]}
          />
          {manages && subscription && !subscription.cancel_at_period_end && data && (
            <IntervalSwitch workspacePlan={data} plans={plans.data ?? []} target={interval} />
          )}
        </div>
      )}

      <div className="grid gap-4 md:grid-cols-2">
        {plans.data?.map((plan) => (
          <PlanCard key={plan.code} plan={plan} interval={interval} workspacePlan={data} />
        ))}
      </div>

      {!sold && (
        <div className="rounded-2xl border border-dashed border-muted-foreground/30 bg-card px-5 py-4">
          <div className="mb-2 flex items-center gap-2 text-sm font-medium text-foreground">
            <Info className="size-4 text-muted-foreground" />
            <Trans>Plans are currently unavailable</Trans>
          </div>
          <p className="text-sm text-muted-foreground">
            <Trans>
              We’re not offering paid plans at the moment. If you need more capacity or a custom
              setup, contact us and we’ll work out something that fits.
            </Trans>
          </p>
          <div className="mt-4 flex flex-col gap-3">
            <p className="text-sm text-muted-foreground">
              <Trans>
                If you’d like to support the project and help cover server costs, sponsoring is a
                great way to do it. Sponsorship does not unlock extra capabilities or plan limits.
              </Trans>
            </p>
            <div className="flex justify-start">
              <Button asChild variant="outline" size="sm" className="shrink-0">
                <a href="https://github.com/sponsors/715209" target="_blank" rel="noreferrer">
                  <HeartHandshake className="size-4" />
                  <Trans>Sponsor on GitHub</Trans>
                </a>
              </Button>
            </div>
          </div>
        </div>
      )}

      {data && shownAddons.length > 0 && (
        <section aria-label={t`Add-ons`} className="flex flex-col gap-3">
          <div className="flex flex-col gap-1">
            <h2 className="text-lg font-semibold">
              <Trans>Add-ons</Trans>
            </h2>
            <p className="text-sm text-muted-foreground">
              <Trans>
                Buy on top of any plan, including Free. Adding is charged now for the rest of the
                period; lowering applies when the period ends.
              </Trans>
            </p>
          </div>
          <div className="grid gap-4 md:grid-cols-2">
            {shownAddons.map((addon) => (
              <AddonCard key={addon.code} addon={addon} interval={interval} workspacePlan={data} />
            ))}
          </div>
        </section>
      )}

      {me.staff && (
        <p className="text-sm text-muted-foreground">
          <Trans>
            Staff:{" "}
            <Link
              to="/admin/workspaces/$workspace"
              params={{ workspace: workspace.id }}
              className="font-medium text-primary hover:underline"
            >
              this workspace in Admin
            </Link>{" "}
            (set its plan, see its billing).
          </Trans>
        </p>
      )}
    </Page>
  );
}

/** Moves the whole subscription to the billing period picked in the toggle. */
function IntervalSwitch({
  workspacePlan,
  plans,
  target,
}: {
  workspacePlan: WorkspacePlan;
  plans: Plan[];
  target: BillingInterval;
}) {
  const change = useBillingChange(
    target === "year" ? t`Switched to yearly billing` : t`Switched to monthly billing`,
  );
  const subscription = workspacePlan.subscription;
  if (!subscription || subscription.interval === target) {
    return (
      <span className="text-sm text-muted-foreground">
        {subscription?.interval === "year" ? t`Billed yearly.` : t`Billed monthly.`}
      </span>
    );
  }
  const planPrice = priceFor(
    plans.find((plan) => plan.code === subscription.plan_code)?.prices ?? [],
    target,
  );

  return (
    <ConfirmButton
      variant="outline"
      disabled={change.isPending}
      title={target === "year" ? t`Switch to yearly billing?` : t`Switch to monthly billing?`}
      confirmLabel={target === "year" ? t`Switch to yearly` : t`Switch to monthly`}
      onConfirm={() => change.mutate({ kind: "interval", interval: target })}
      description={
        <>
          <span>
            {target === "year"
              ? t`Your period restarts today. The unused part of the current period is credited, and the new year is charged now.`
              : t`Your period restarts today. The unused part of the current period is credited, and the new month is charged now.`}
            {planPrice &&
              ` ${t`${formatPrice(planPrice.amount, planPrice.currency)} is for the plan.`}`}
          </span>
          <span>
            <Trans>If the card is declined, nothing changes.</Trans>
          </span>
        </>
      }
    >
      {target === "year" ? t`Switch to yearly billing` : t`Switch to monthly billing`}
    </ConfirmButton>
  );
}

function PlanCard({
  plan,
  interval,
  workspacePlan,
}: {
  plan: Plan;
  interval: BillingInterval;
  workspacePlan: WorkspacePlan | undefined;
}) {
  const current = plan.code === workspacePlan?.plan.code;
  const shownInterval = workspacePlan?.subscription?.interval ?? interval;
  const price = priceFor(plan.prices, interval);
  const saving = yearlySaving(plan.prices);
  const perks = [
    plural(plan.max_switchers ?? 0, { one: "# switcher", other: "# switchers" }),
    plural(plan.max_stream_servers, {
      one: "# stream server per switcher",
      other: "# stream servers per switcher",
    }),
    plural(plan.max_members, { one: "# member", other: "# members" }),
    plural(plan.history_days, {
      one: "# day of stream history",
      other: "# days of stream history",
    }),
    t`Automatic LIVE / LOW / BRB switching`,
    t`Chat commands on Twitch, Kick and YouTube`,
    ...(plan.custom_chat_bot ? [t`Your own chat bot account`] : []),
  ];

  return (
    <section
      className={cn(
        "flex min-w-0 flex-col gap-5 rounded-2xl border bg-card p-5 shadow-tile",
        current && "border-primary-soft-border",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-col gap-1">
          <h2 className="text-lg font-semibold">{plan.name}</h2>
          <p className="flex flex-wrap items-baseline gap-x-2 text-sm text-muted-foreground">
            {price ? (
              <>
                <span className="font-display text-2xl font-bold text-foreground tabular">
                  {formatPrice(price.amount, price.currency)}
                </span>
                / {interval === "month" ? t`month` : t`year`}
                {interval === "year" && saving > 0 && (
                  <span className="font-semibold text-muted-foreground">
                    <Trans>save {saving}%</Trans>
                  </span>
                )}
              </>
            ) : plan.code === FREE_PLAN ? (
              <span>
                <Trans>Always free</Trans>
              </span>
            ) : plan.prices.length === 0 ? (
              <span>
                <Trans>Not for sale yet</Trans>
              </span>
            ) : (
              <span>{interval === "month" ? t`Not sold monthly` : t`Not sold yearly`}</span>
            )}
          </p>
        </div>
        {current && (
          <span className="text-xs text-muted-foreground">
            <Trans>Current plan</Trans>
          </span>
        )}
      </div>
      <ul className="flex flex-col gap-2.5 text-sm">
        {perks.map((perk) => (
          <li key={perk} className="flex items-center gap-2.5">
            <Check className="size-4 shrink-0 text-muted-foreground" />
            {perk}
          </li>
        ))}
      </ul>
      {workspacePlan && (
        <PlanAction
          plan={plan}
          workspacePlan={workspacePlan}
          interval={interval}
          subscriptionPrice={priceFor(plan.prices, shownInterval)}
        />
      )}
    </section>
  );
}

/** The button under a plan, and why it may be unavailable. */
function PlanAction({
  plan,
  workspacePlan,
  interval,
  subscriptionPrice,
}: {
  plan: Plan;
  workspacePlan: WorkspacePlan;
  interval: BillingInterval;
  subscriptionPrice: ReturnType<typeof priceFor>;
}) {
  const checkout = useStripeRedirect();
  const change = useBillingChange(t`Plan updated`);
  const subscription = workspacePlan.subscription;
  const current = plan.code === workspacePlan.plan.code;
  const free = plan.code === FREE_PLAN;
  // A paid plan staff haven't priced yet: shown, but nothing to buy.
  const unpriced = !free && plan.prices.length === 0;
  const button = "mt-auto h-11 w-full rounded-xl";

  const note = (text: string) => (
    <div className="mt-auto flex flex-col gap-2">
      <Button className={button} disabled>
        <Trans>Switch to {plan.name}</Trans>
      </Button>
      <p className="text-center text-xs text-muted-foreground">{text}</p>
    </div>
  );

  // Moving to Free is already scheduled: the paid plan can be kept, free of charge.
  if (current && subscription?.held_plan_code === plan.code) {
    return (
      <Button
        className={button}
        variant="outline"
        disabled={change.isPending || !workspacePlan.can_manage_billing}
        onClick={() => change.mutate({ kind: "plan", plan_code: plan.code })}
      >
        <Trans>Keep {plan.name}</Trans>
      </Button>
    );
  }
  if (current) return null;
  if (unpriced) return note(t`Not for sale yet.`);
  if (free && subscription?.held_plan_code) {
    return (
      <p className="mt-auto text-center text-sm text-muted-foreground">
        <Trans>Starts on {formatDay(subscription.held_plan_until)}.</Trans>
      </p>
    );
  }
  if (!workspacePlan.billing_enabled) return note(t`Plan changes open soon.`);
  if (!workspacePlan.can_manage_billing)
    return note(t`Only the workspace owner can change the plan.`);
  if (subscription?.cancel_at_period_end) return note(t`Resume the subscription to change it.`);

  if (!subscription) {
    if (free || !priceFor(plan.prices, interval)) return null;
    return (
      <Button
        className={button}
        disabled={checkout.isPending}
        onClick={() => checkout.mutate({ plan_code: plan.code, interval })}
      >
        {checkout.isPending ? t`Opening checkout…` : t`Upgrade to ${plan.name}`}
      </Button>
    );
  }

  if (free) {
    const keepsAddons = workspacePlan.addons.some((addon) => addon.billed > 0);
    return (
      <ConfirmButton
        className={button}
        variant="outline"
        disabled={change.isPending}
        title={t`Move to Free?`}
        confirmLabel={t`Move to Free`}
        onConfirm={() => change.mutate({ kind: "plan", plan_code: plan.code })}
        description={
          <>
            <span>
              <Trans>
                {workspacePlan.plan.name} stays until {formatDay(subscription.current_period_end)},
                then the workspace moves to Free. Nothing is refunded.
              </Trans>
            </span>
            {keepsAddons ? (
              <span>
                <Trans>Your add-ons keep running and are billed as before.</Trans>
              </span>
            ) : (
              <span>
                <Trans>That ends the subscription.</Trans>
              </span>
            )}
            <span>
              <Trans>
                Switchers beyond Free’s limit pause then; they stay connected but don’t switch.
              </Trans>
            </span>
          </>
        }
      >
        <Trans>Move to Free</Trans>
      </ConfirmButton>
    );
  }

  if (!subscriptionPrice) return note(t`Not sold with your billing period.`);
  return (
    <ConfirmButton
      className={button}
      disabled={change.isPending}
      title={t`Upgrade to ${plan.name}?`}
      confirmLabel={t`Pay and upgrade`}
      onConfirm={() => change.mutate({ kind: "plan", plan_code: plan.code })}
      description={
        <>
          <span>
            <Trans>
              You’ll be charged about{" "}
              <strong>
                {formatPrice(
                  prorate(
                    subscriptionPrice.amount,
                    1,
                    subscription.current_period_start,
                    subscription.current_period_end,
                  ),
                  subscriptionPrice.currency,
                )}
              </strong>{" "}
              now for the rest of this period, then{" "}
              {formatPrice(subscriptionPrice.amount, subscriptionPrice.currency)} per{" "}
              {intervalWord(subscription.interval)}, together with your add-ons.
            </Trans>
          </span>
          <span>
            <Trans>If the card is declined, nothing changes.</Trans>
          </span>
        </>
      }
    >
      <Trans>Upgrade to {plan.name}</Trans>
    </ConfirmButton>
  );
}

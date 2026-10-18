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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { UsageBar } from "@/components/usage-bar";
import { useBillingChange, useStripeRedirect } from "@/hooks/use-billing";
import { useWorkspace } from "@/hooks/use-workspace";
import {
  describeSubscription,
  FREE_PLAN,
  formatPrice,
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
    : "the end of the period";
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
  const resume = useBillingChange("Your subscription continues");
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
      toast.success("Thanks! Your payment went through.");
    }
    void navigate({ to: ".", search: {}, replace: true });
  }, [search.checkout, navigate]);

  const subscribed = current.data?.subscription != null;
  useEffect(() => {
    if (waitingSince == null || !subscribed) return;
    setWaitingSince(null);
    toast.success("Your subscription is active.");
    void queryClient.invalidateQueries({ queryKey: queryKeys.switchers(workspace.id) });
    void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces });
  }, [waitingSince, subscribed, queryClient, workspace.id]);

  const sold =
    (plans.data?.some((plan) => plan.prices.length > 0) ?? false) ||
    (addons.data?.some((addon) => addon.prices.length > 0) ?? false);
  const data = current.data;
  const subscription = data?.subscription;
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
    <Page
      title="Plan"
      description="What this workspace can use."
      actions={
        manages &&
        subscription && (
          <Button
            variant="outline"
            onClick={() => portal.mutate("portal")}
            disabled={portal.isPending}
          >
            <CreditCard />
            Manage billing
          </Button>
        )
      }
    >
      <Section
        title={data ? `${data.plan.name} plan` : "Plan"}
        description={
          waitingSince != null
            ? "Confirming your payment… this takes a few seconds."
            : status && !status.warning
              ? status.text
              : undefined
        }
        actions={data && <Badge className="uppercase">{data.plan.code}</Badge>}
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
                Resume subscription
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
            Moving to Free on {formatDay(subscription.held_plan_until)}; {data?.plan.name} stays
            until then.
          </p>
        )}
        <div className="grid gap-5 px-5 pb-5 sm:grid-cols-2">
          {data ? (
            <>
              <UsageBar
                label="Switchers"
                used={data.usage.switchers}
                max={data.plan.max_switchers}
              />
              <UsageBar label="Members" used={data.usage.members} max={data.plan.max_members} />
            </>
          ) : (
            <Skeleton className="h-12 sm:col-span-2" />
          )}
        </div>
        {data &&
          data.plan.max_switchers != null &&
          data.usage.switchers > data.plan.max_switchers && (
            <p className="mx-5 mb-5 text-sm text-muted-foreground">
              The newest switchers beyond {data.plan.max_switchers} are paused: they stay connected
              but don’t switch. Upgrade, or delete switchers you no longer need.
            </p>
          )}
      </Section>

      {sold && (
        <div className="flex flex-wrap items-center gap-3">
          <fieldset
            aria-label="Billing period"
            className="flex rounded-full border bg-card p-1 text-sm font-medium"
          >
            {(["month", "year"] as const).map((option) => (
              <button
                key={option}
                type="button"
                aria-pressed={interval === option}
                onClick={() => setBillingInterval(option)}
                className={cn(
                  "min-h-8 rounded-full px-4 text-muted-foreground",
                  interval === option && "bg-primary-soft text-primary-soft-foreground",
                )}
              >
                {option === "month" ? "Monthly" : "Yearly"}
              </button>
            ))}
          </fieldset>
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
            Plans are currently unavailable
          </div>
          <p className="text-sm text-muted-foreground">
            We’re not offering paid plans at the moment. If you need more capacity or a custom
            setup, contact us and we’ll work out something that fits.
          </p>
          <div className="mt-4 flex flex-col gap-3">
            <p className="text-sm text-muted-foreground">
              If you’d like to support the project and help cover server costs, sponsoring is a
              great way to do it. Sponsorship does not unlock extra capabilities or plan limits.
            </p>
            <div className="flex justify-start">
              <Button asChild variant="outline" size="sm" className="shrink-0">
                <a href="https://github.com/sponsors/715209" target="_blank" rel="noreferrer">
                  <HeartHandshake className="size-4" />
                  Sponsor on GitHub
                </a>
              </Button>
            </div>
          </div>
        </div>
      )}

      {data && shownAddons.length > 0 && (
        <section aria-label="Add-ons" className="flex flex-col gap-3">
          <div className="flex flex-col gap-1">
            <h2 className="text-lg font-semibold">Add-ons</h2>
            <p className="text-sm text-muted-foreground">
              Buy on top of any plan, including Free. Adding is charged now for the rest of the
              period; lowering applies when the period ends.
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
          Staff:{" "}
          <Link
            to="/admin/workspaces/$workspace"
            params={{ workspace: workspace.id }}
            className="font-medium text-primary hover:underline"
          >
            this workspace in Admin
          </Link>{" "}
          (set its plan, see its billing).
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
    `Switched to ${target === "year" ? "yearly" : "monthly"} billing`,
  );
  const subscription = workspacePlan.subscription;
  if (!subscription || subscription.interval === target) {
    return (
      <span className="text-sm text-muted-foreground">
        Billed {subscription?.interval === "year" ? "yearly" : "monthly"}.
      </span>
    );
  }
  const planPrice = priceFor(
    plans.find((plan) => plan.code === subscription.plan_code)?.prices ?? [],
    target,
  );
  const word = target === "year" ? "yearly" : "monthly";

  return (
    <ConfirmButton
      variant="outline"
      disabled={change.isPending}
      title={`Switch to ${word} billing?`}
      confirmLabel={`Switch to ${word}`}
      onConfirm={() => change.mutate({ kind: "interval", interval: target })}
      description={
        <>
          <span>
            Your period restarts today. The unused part of the current period is credited, and the
            new {target} is charged now
            {planPrice
              ? ` (${formatPrice(planPrice.amount, planPrice.currency)} for the plan)`
              : ""}
            .
          </span>
          <span>If the card is declined, nothing changes.</span>
        </>
      }
    >
      Switch to {word} billing
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
    plan.max_switchers === 1 ? "1 switcher" : `${plan.max_switchers ?? 0} switchers`,
    `${plan.max_stream_servers} stream servers per switcher`,
    `${plan.max_members} members`,
    `${plan.history_days} days of stream history`,
    "Automatic LIVE / LOW / BRB switching",
    "Chat commands on Twitch, Kick and YouTube",
    ...(plan.custom_chat_bot ? ["Your own chat bot account"] : []),
  ];

  return (
    <section
      className={cn(
        "flex flex-col gap-5 rounded-2xl border p-5.5",
        current ? "border-primary-soft-border bg-primary-soft" : "bg-card",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex flex-col gap-1">
          <h2 className="text-lg font-semibold">{plan.name}</h2>
          <p className="flex flex-wrap items-baseline gap-x-2 text-sm text-muted-foreground">
            {price ? (
              <>
                <span className="font-mono text-2xl font-semibold text-foreground tabular">
                  {formatPrice(price.amount, price.currency)}
                </span>
                / {interval === "month" ? "month" : "year"}
                {interval === "year" && saving > 0 && (
                  <span className="font-semibold text-live">save {saving}%</span>
                )}
              </>
            ) : plan.code === FREE_PLAN ? (
              <span>Always free</span>
            ) : plan.prices.length === 0 ? (
              <span>Not for sale yet</span>
            ) : (
              <span>Not sold {interval === "month" ? "monthly" : "yearly"}</span>
            )}
          </p>
        </div>
        {current && <Badge>Current plan</Badge>}
      </div>
      <ul className="flex flex-col gap-2.5 text-sm">
        {perks.map((perk) => (
          <li key={perk} className="flex items-center gap-2.5">
            <Check className="size-4 text-live" />
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
  const change = useBillingChange(`Plan updated`);
  const subscription = workspacePlan.subscription;
  const current = plan.code === workspacePlan.plan.code;
  const free = plan.code === FREE_PLAN;
  // A paid plan staff haven't priced yet: shown, but nothing to buy.
  const unpriced = !free && plan.prices.length === 0;
  const button = "mt-auto h-11 w-full rounded-xl";

  const note = (text: string) => (
    <div className="mt-auto flex flex-col gap-2">
      <Button className={button} disabled>
        Switch to {plan.name}
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
        Keep {plan.name}
      </Button>
    );
  }
  if (current) return null;
  if (unpriced) return note("Not for sale yet.");
  if (free && subscription?.held_plan_code) {
    return (
      <p className="mt-auto text-center text-sm text-muted-foreground">
        Starts on {formatDay(subscription.held_plan_until)}.
      </p>
    );
  }
  if (!workspacePlan.billing_enabled) return note("Plan changes open soon.");
  if (!workspacePlan.can_manage_billing)
    return note("Only the workspace owner can change the plan.");
  if (subscription?.cancel_at_period_end) return note("Resume the subscription to change it.");

  if (!subscription) {
    if (free || !priceFor(plan.prices, interval)) return null;
    return (
      <Button
        className={button}
        disabled={checkout.isPending}
        onClick={() => checkout.mutate({ plan_code: plan.code, interval })}
      >
        {checkout.isPending ? "Opening checkout…" : `Upgrade to ${plan.name}`}
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
        title="Move to Free?"
        confirmLabel="Move to Free"
        onConfirm={() => change.mutate({ kind: "plan", plan_code: plan.code })}
        description={
          <>
            <span>
              {workspacePlan.plan.name} stays until {formatDay(subscription.current_period_end)},
              then the workspace moves to Free. Nothing is refunded.
            </span>
            {keepsAddons ? (
              <span>Your add-ons keep running and are billed as before.</span>
            ) : (
              <span>That ends the subscription.</span>
            )}
            <span>
              Switchers beyond Free’s limit pause then; they stay connected but don’t switch.
            </span>
          </>
        }
      >
        Move to Free
      </ConfirmButton>
    );
  }

  if (!subscriptionPrice) return note(`Not sold with your billing period.`);
  return (
    <ConfirmButton
      className={button}
      disabled={change.isPending}
      title={`Upgrade to ${plan.name}?`}
      confirmLabel="Pay and upgrade"
      onConfirm={() => change.mutate({ kind: "plan", plan_code: plan.code })}
      description={
        <>
          <span>
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
            {subscription.interval}, together with your add-ons.
          </span>
          <span>If the card is declined, nothing changes.</span>
        </>
      }
    >
      Upgrade to {plan.name}
    </ConfirmButton>
  );
}

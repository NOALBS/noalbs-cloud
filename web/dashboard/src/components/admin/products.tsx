import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, Link2, Pencil, Plus, Trash2 } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type {
  AddonVisibility,
  AdminAddon,
  AdminPlan,
  AdminPrice,
  AdminProducts,
  BillingInterval,
} from "@/api/types";
import { ConfirmButton } from "@/components/billing/confirm-button";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { codeFromName, fromCents, toCents, validCode } from "@/lib/admin";
import { ADDON_STATUS_LABELS, formatPrice } from "@/lib/billing";
import { errorMessage } from "@/lib/errors";

type Kind = "plan" | "addon";
type ProductChange = () => Promise<AdminProducts>;

/** Runs a change that answers with the products page, and updates the cache. */
function useProductChange(success: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (change: ProductChange) => change(),
    onSuccess: async (products) => {
      queryClient.setQueryData(queryKeys.adminProducts, products);
      // What's for sale shows on every workspace's Plan page.
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.plans }),
        queryClient.invalidateQueries({ queryKey: queryKeys.addons }),
        queryClient.invalidateQueries({ queryKey: queryKeys.adminOverview }),
      ]);
      toast.success(success);
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
}

const STATUSES: AddonVisibility[] = ["hidden", "coming_soon", "unavailable", "available"];

const STATUS_HINTS: Record<AddonVisibility, string> = {
  hidden: "Not listed. Workspaces that already have it still see it and can lower it.",
  coming_soon: "Listed, but nobody can buy it yet.",
  unavailable: "Listed as currently unavailable: nobody can buy more, those who have it keep it.",
  available: "For sale, as soon as it has a price.",
};

const INTERVALS: { value: BillingInterval; label: string }[] = [
  { value: "month", label: "Monthly" },
  { value: "year", label: "Yearly" },
];

function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

function NumberInput({
  value,
  onChange,
  label,
}: {
  value: number;
  onChange: (value: number) => void;
  label: string;
}) {
  return (
    <Input
      type="number"
      min={1}
      inputMode="numeric"
      aria-label={label}
      value={Number.isNaN(value) ? "" : value}
      onChange={(event) => onChange(Number.parseInt(event.target.value, 10))}
    />
  );
}

function ToggleRow({
  label,
  hint,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  hint: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-4 rounded-xl border px-3.5 py-3">
      <div className="flex flex-col">
        <span className="text-sm font-medium">{label}</span>
        <span className="text-xs text-muted-foreground">{hint}</span>
      </div>
      <Switch checked={checked} onCheckedChange={onChange} disabled={disabled} aria-label={label} />
    </div>
  );
}

/** Creates a plan (no `plan`) or edits one. */
export function PlanDialog({ plan, children }: { plan?: AdminPlan; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [form, setForm] = useState(() => initialPlan(plan));
  const change = useProductChange(plan ? `${plan.name} saved` : "Plan created");
  const isFree = plan?.code === "free";
  const set = (patch: Partial<typeof form>) => setForm((current) => ({ ...current, ...patch }));

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const body = {
      name: form.name,
      description: form.description,
      is_active: form.is_active,
      position: form.position,
      max_switchers: form.max_switchers,
      max_stream_servers: form.max_stream_servers,
      max_members: form.max_members,
      history_days: form.history_days,
      custom_chat_bot: form.custom_chat_bot,
    };
    change.mutate(
      () =>
        plan
          ? unwrap(api.PUT("/admin/plans/{code}", { params: { path: { code: plan.code } }, body }))
          : unwrap(api.POST("/admin/plans", { body: { ...body, code: form.code } })),
      { onSuccess: () => setOpen(false) },
    );
  };
  const valid =
    form.name.trim() !== "" &&
    (plan != null || validCode(form.code)) &&
    [form.max_switchers, form.max_stream_servers, form.max_members, form.history_days].every(
      (n) => n >= 1,
    );

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setForm(initialPlan(plan));
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>{plan ? `Edit ${plan.name}` : "New plan"}</DialogTitle>
            <DialogDescription>
              {plan
                ? "Limits apply to every workspace on this plan right away."
                : "Set its prices after creating it."}
            </DialogDescription>
          </DialogHeader>
          <Field label="Name">
            <Input
              value={form.name}
              onChange={(event) =>
                set({
                  name: event.target.value,
                  ...(plan ? {} : { code: codeFromName(event.target.value) }),
                })
              }
              maxLength={60}
            />
          </Field>
          {!plan && (
            <Field label="Code" hint="Used in URLs and billing; can't be changed later.">
              <Input
                value={form.code}
                onChange={(event) => set({ code: event.target.value })}
                className="font-mono"
              />
            </Field>
          )}
          <Field label="Description">
            <Input
              value={form.description}
              onChange={(event) => set({ description: event.target.value })}
            />
          </Field>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <Field label="Switchers">
              <NumberInput
                label="Switchers"
                value={form.max_switchers}
                onChange={(max_switchers) => set({ max_switchers })}
              />
            </Field>
            <Field label="Servers each">
              <NumberInput
                label="Stream servers per switcher"
                value={form.max_stream_servers}
                onChange={(max_stream_servers) => set({ max_stream_servers })}
              />
            </Field>
            <Field label="Members">
              <NumberInput
                label="Members"
                value={form.max_members}
                onChange={(max_members) => set({ max_members })}
              />
            </Field>
            <Field label="History days">
              <NumberInput
                label="History days"
                value={form.history_days}
                onChange={(history_days) => set({ history_days })}
              />
            </Field>
          </div>
          <Field label="Order" hint="Lower comes first on the Plan page.">
            <Input
              type="number"
              value={form.position}
              onChange={(event) => set({ position: Number.parseInt(event.target.value, 10) || 0 })}
            />
          </Field>
          <ToggleRow
            label="Own chat bot"
            hint="Workspaces may connect their own bot account instead of the shared NOALBS bot."
            checked={form.custom_chat_bot}
            onChange={(custom_chat_bot) => set({ custom_chat_bot })}
          />
          <ToggleRow
            label="Offered"
            hint={
              isFree
                ? "Free is where workspaces land without a subscription."
                : "Hidden plans keep the workspaces already on them."
            }
            checked={form.is_active}
            onChange={(is_active) => set({ is_active })}
            disabled={isFree}
          />
          <DialogFooter>
            <Button type="submit" disabled={!valid || change.isPending}>
              {plan ? "Save" : "Create plan"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function initialPlan(plan?: AdminPlan) {
  return {
    code: plan?.code ?? "",
    name: plan?.name ?? "",
    description: plan?.description ?? "",
    is_active: plan?.is_active ?? true,
    position: plan?.position ?? 10,
    max_switchers: plan?.max_switchers ?? 1,
    max_stream_servers: plan?.max_stream_servers ?? 3,
    max_members: plan?.max_members ?? 3,
    history_days: plan?.history_days ?? 7,
    custom_chat_bot: plan?.custom_chat_bot ?? false,
  };
}

/** Creates an add-on (no `addon`) or edits one. */
export function AddonDialog({ addon, children }: { addon?: AdminAddon; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [form, setForm] = useState(() => initialAddon(addon));
  const change = useProductChange(addon ? `${addon.name} saved` : "Add-on created");
  const set = (patch: Partial<typeof form>) => setForm((current) => ({ ...current, ...patch }));

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const body = {
      name: form.name,
      description: form.description,
      unit: form.unit,
      max_quantity: form.max_quantity,
      status: form.status,
      position: form.position,
    };
    change.mutate(
      () =>
        addon
          ? unwrap(
              api.PUT("/admin/addons/{code}", { params: { path: { code: addon.code } }, body }),
            )
          : unwrap(api.POST("/admin/addons", { body: { ...body, code: form.code } })),
      { onSuccess: () => setOpen(false) },
    );
  };
  const valid =
    form.name.trim() !== "" &&
    form.unit.trim() !== "" &&
    (addon != null || validCode(form.code)) &&
    form.max_quantity >= 1;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setForm(initialAddon(addon));
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>{addon ? `Edit ${addon.name}` : "New add-on"}</DialogTitle>
            <DialogDescription>
              {addon
                ? "Sold per unit on top of any plan."
                : "Sold per unit on top of any plan. Set its prices after creating it."}
            </DialogDescription>
          </DialogHeader>
          <Field label="Name">
            <Input
              value={form.name}
              onChange={(event) =>
                set({
                  name: event.target.value,
                  ...(addon ? {} : { code: codeFromName(event.target.value) }),
                })
              }
              maxLength={60}
            />
          </Field>
          {!addon && (
            <Field label="Code" hint="Features check it by this code; can't be changed later.">
              <Input
                value={form.code}
                onChange={(event) => set({ code: event.target.value })}
                className="font-mono"
              />
            </Field>
          )}
          <Field label="Description" hint="Shown on the Plan page and in Checkout.">
            <Input
              value={form.description}
              onChange={(event) => set({ description: event.target.value })}
            />
          </Field>
          <div className="grid grid-cols-3 gap-3">
            <Field label="Unit">
              <Input
                value={form.unit}
                onChange={(event) => set({ unit: event.target.value })}
                placeholder="slot"
                maxLength={20}
              />
            </Field>
            <Field label="Max per workspace">
              <NumberInput
                label="Max per workspace"
                value={form.max_quantity}
                onChange={(max_quantity) => set({ max_quantity })}
              />
            </Field>
            <Field label="Order">
              <Input
                type="number"
                value={form.position}
                onChange={(event) =>
                  set({ position: Number.parseInt(event.target.value, 10) || 0 })
                }
              />
            </Field>
          </div>
          <Field label="On the Plan page" hint={STATUS_HINTS[form.status]}>
            <Select
              value={form.status}
              onValueChange={(status) => set({ status: status as AddonVisibility })}
            >
              <SelectTrigger aria-label="On the Plan page" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {STATUSES.map((status) => (
                  <SelectItem key={status} value={status}>
                    {ADDON_STATUS_LABELS[status]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={!valid || change.isPending}>
              {addon ? "Save" : "Create add-on"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function initialAddon(addon?: AdminAddon) {
  return {
    code: addon?.code ?? "",
    name: addon?.name ?? "",
    description: addon?.description ?? "",
    unit: addon?.unit ?? "slot",
    max_quantity: addon?.max_quantity ?? 5,
    status: addon?.status ?? ("hidden" as AddonVisibility),
    position: addon?.position ?? 10,
  };
}

/** Sets a new price (created in Stripe) for one billing period. */
function PriceDialog({
  kind,
  code,
  name,
  interval,
  current,
  currency,
  unit,
}: {
  kind: Kind;
  code: string;
  name: string;
  interval: BillingInterval;
  current: AdminPrice | undefined;
  currency: string | null | undefined;
  unit?: string;
}) {
  const [open, setOpen] = useState(false);
  const [amount, setAmount] = useState(current ? fromCents(current.amount) : "");
  const [cur, setCur] = useState(current?.currency ?? currency ?? "eur");
  const change = useProductChange(`${name} price set`);
  const cents = toCents(amount);
  const period = interval === "month" ? "month" : "year";

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (cents == null) return;
    change.mutate(
      () =>
        unwrap(
          api.PUT("/admin/products/{kind}/{code}/prices/{interval}", {
            params: { path: { kind, code, interval } },
            body: { amount: cents, currency: cur.trim().toLowerCase() },
          }),
        ),
      { onSuccess: () => setOpen(false) },
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) {
          setAmount(current ? fromCents(current.amount) : "");
          setCur(current?.currency ?? currency ?? "eur");
        }
      }}
    >
      <DialogTrigger asChild>
        <Button size="sm" variant="outline">
          {current ? "Change price" : "Set price"}
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>
              {name}, {interval === "month" ? "monthly" : "yearly"}
            </DialogTitle>
            <DialogDescription>
              A new price is created in Stripe for new subscriptions.
              {current && " People already paying keep their current price."}
            </DialogDescription>
          </DialogHeader>
          <div className="grid grid-cols-[1fr_6rem] gap-3">
            <Field label={unit ? `Per ${unit} per ${period}` : `Per ${period}`}>
              <Input
                value={amount}
                onChange={(event) => setAmount(event.target.value)}
                inputMode="decimal"
                placeholder="9.00"
                autoFocus
              />
            </Field>
            <Field label="Currency">
              <Input
                value={cur}
                onChange={(event) => setCur(event.target.value)}
                maxLength={3}
                className="font-mono uppercase"
                disabled={currency != null}
              />
            </Field>
          </div>
          {currency != null && (
            <p className="text-xs text-muted-foreground">
              All prices use {currency.toUpperCase()}: one subscription can’t mix currencies.
            </p>
          )}
          <DialogFooter>
            <Button type="submit" disabled={cents == null || change.isPending}>
              {cents == null ? "Set price" : `Set to ${formatPrice(cents, cur || "eur")}`}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Sells at a price already made in the Stripe Dashboard. */
function LinkPriceDialog({
  kind,
  code,
  name,
  interval,
}: {
  kind: Kind;
  code: string;
  name: string;
  interval: BillingInterval;
}) {
  const [open, setOpen] = useState(false);
  const [priceId, setPriceId] = useState("");
  const change = useProductChange(`${name} linked to the Stripe price`);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    change.mutate(
      () =>
        unwrap(
          api.POST("/admin/products/{kind}/{code}/prices/{interval}/link", {
            params: { path: { kind, code, interval } },
            body: { stripe_price_id: priceId.trim() },
          }),
        ),
      { onSuccess: () => setOpen(false) },
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) setPriceId("");
      }}
    >
      <DialogTrigger asChild>
        <Button size="icon-sm" variant="ghost" aria-label="Link an existing Stripe price">
          <Link2 />
        </Button>
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>Link a Stripe price</DialogTitle>
            <DialogDescription>
              Sell {name} {interval === "month" ? "monthly" : "yearly"} at a price you made in the
              Stripe Dashboard. It must be a flat-rate recurring price billed every {interval}.
            </DialogDescription>
          </DialogHeader>
          <Field label="Price ID">
            <Input
              value={priceId}
              onChange={(event) => setPriceId(event.target.value)}
              placeholder="price_…"
              className="font-mono"
              autoFocus
            />
          </Field>
          <DialogFooter>
            <Button
              type="submit"
              disabled={!priceId.trim().startsWith("price_") || change.isPending}
            >
              Link price
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function PriceRow({
  kind,
  code,
  name,
  interval,
  label,
  price,
  currency,
  unit,
  billingEnabled,
}: {
  kind: Kind;
  code: string;
  name: string;
  interval: BillingInterval;
  label: string;
  price: AdminPrice | undefined;
  currency: string | null | undefined;
  unit?: string;
  billingEnabled: boolean;
}) {
  const stop = useProductChange(`${name} is no longer sold ${label.toLowerCase()}`);
  return (
    <div className="flex flex-wrap items-center justify-between gap-2 border-t border-divider py-2.5 first:border-t-0">
      <div className="flex min-w-0 flex-col">
        <span className="text-xs tracking-wider text-muted-foreground uppercase">{label}</span>
        {price ? (
          <span className="font-mono text-sm font-semibold tabular">
            {formatPrice(price.amount, price.currency)}
            {unit && <span className="font-sans font-normal text-muted-foreground"> / {unit}</span>}
          </span>
        ) : (
          <span className="text-sm text-muted-foreground">Not sold</span>
        )}
      </div>
      {billingEnabled && (
        <div className="flex items-center gap-1">
          <PriceDialog
            kind={kind}
            code={code}
            name={name}
            interval={interval}
            current={price}
            currency={currency}
            unit={unit}
          />
          <LinkPriceDialog kind={kind} code={code} name={name} interval={interval} />
          {price && (
            <ConfirmButton
              variant="ghost"
              className="h-8 px-2 text-xs"
              title={`Stop selling ${name} ${label.toLowerCase()}?`}
              confirmLabel="Stop selling"
              onConfirm={() =>
                stop.mutate(() =>
                  unwrap(
                    api.DELETE("/admin/products/{kind}/{code}/prices/{interval}", {
                      params: { path: { kind, code, interval } },
                    }),
                  ),
                )
              }
              description={
                <span>
                  New subscriptions can’t choose it anymore. People already paying keep it until
                  they cancel.
                </span>
              }
            >
              Stop
            </ConfirmButton>
          )}
        </div>
      )}
    </div>
  );
}

function StripeLink({ href }: { href: string | null | undefined }) {
  if (!href) return null;
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline"
    >
      Stripe
      <ExternalLink className="size-3" />
    </a>
  );
}

/** Removes a plan or add-on nothing depends on; otherwise says why it can't go. */
function RemoveButton({
  kind,
  code,
  name,
  blockedBy,
}: {
  kind: Kind;
  code: string;
  name: string;
  blockedBy: string | null | undefined;
}) {
  const remove = useProductChange(`${name} removed`);
  if (blockedBy) {
    return (
      <span title={`Can't remove: ${blockedBy}`}>
        <Button
          size="icon-sm"
          variant="ghost"
          disabled
          aria-label={`Can't remove ${name}: ${blockedBy}`}
        >
          <Trash2 />
        </Button>
      </span>
    );
  }
  return (
    <ConfirmButton
      variant="ghost"
      className="size-8"
      ariaLabel={`Remove ${name}`}
      disabled={remove.isPending}
      title={`Remove ${name}?`}
      confirmLabel="Remove"
      onConfirm={() =>
        remove.mutate(() =>
          kind === "plan"
            ? unwrap(api.DELETE("/admin/plans/{code}", { params: { path: { code } } }))
            : unwrap(api.DELETE("/admin/addons/{code}", { params: { path: { code } } })),
        )
      }
      description={
        <span>
          Nobody uses it, so it can go. Its product and prices are archived in Stripe. This can't be
          undone, but you can create it again.
        </span>
      }
    >
      <Trash2 />
    </ConfirmButton>
  );
}

function ProductFrame({
  name,
  code,
  badges,
  description,
  stripeUrl,
  edit,
  stats,
  children,
}: {
  name: string;
  code: string;
  badges: ReactNode;
  description: string;
  stripeUrl: string | null | undefined;
  edit: ReactNode;
  stats: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3 rounded-2xl border bg-card p-5">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="text-lg font-semibold">{name}</h3>
            <code className="rounded bg-muted px-1.5 text-xs text-muted-foreground">{code}</code>
            {badges}
          </div>
          {description && <p className="text-sm text-muted-foreground">{description}</p>}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <StripeLink href={stripeUrl} />
          {edit}
        </div>
      </div>
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">{stats}</div>
      <div className="flex flex-col rounded-xl border px-3.5">{children}</div>
    </section>
  );
}

function archivedNote(count: number) {
  if (count === 0) return null;
  return (
    <span>
      {count} earlier {count === 1 ? "price" : "prices"}, kept by existing subscribers
    </span>
  );
}

/** "1 switcher", "5 switchers". */
function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

export function PlanProduct({
  plan,
  currency,
  billingEnabled,
}: {
  plan: AdminPlan;
  currency: string | null | undefined;
  billingEnabled: boolean;
}) {
  const free = plan.code === "free";
  return (
    <ProductFrame
      name={plan.name}
      code={plan.code}
      description={plan.description}
      stripeUrl={plan.stripe_product_url}
      badges={!plan.is_active && <Badge variant="secondary">Hidden</Badge>}
      edit={
        <>
          <RemoveButton
            kind="plan"
            code={plan.code}
            name={plan.name}
            blockedBy={plan.removal_blocked_by}
          />
          <PlanDialog plan={plan}>
            <Button size="icon-sm" variant="ghost" aria-label={`Edit ${plan.name}`}>
              <Pencil />
            </Button>
          </PlanDialog>
        </>
      }
      stats={
        <>
          <span>
            {plural(plan.max_switchers, "switcher")} ·{" "}
            {plural(plan.max_stream_servers, "stream server")} each ·{" "}
            {plural(plan.max_members, "member")} · {plural(plan.history_days, "day")} of history
            {plan.custom_chat_bot && " · own chat bot"}
          </span>
          <span>
            {plan.workspaces} {plan.workspaces === 1 ? "workspace" : "workspaces"} on it
            {!free && `, ${plan.subscribers} paying`}
          </span>
          {archivedNote(plan.archived_prices)}
        </>
      }
    >
      {free ? (
        <p className="py-2.5 text-sm text-muted-foreground">No price: it’s the free plan.</p>
      ) : (
        INTERVALS.map((interval) => (
          <PriceRow
            key={interval.value}
            kind="plan"
            code={plan.code}
            name={plan.name}
            interval={interval.value}
            label={interval.label}
            price={plan.prices.find((price) => price.interval === interval.value)}
            currency={currency}
            billingEnabled={billingEnabled}
          />
        ))
      )}
    </ProductFrame>
  );
}

export function AddonProduct({
  addon,
  currency,
  billingEnabled,
}: {
  addon: AdminAddon;
  currency: string | null | undefined;
  billingEnabled: boolean;
}) {
  return (
    <ProductFrame
      name={addon.name}
      code={addon.code}
      description={addon.description}
      stripeUrl={addon.stripe_product_url}
      badges={
        <Badge
          variant={addon.status === "available" ? "default" : "secondary"}
          className={addon.status === "available" ? "bg-live/15 text-live" : undefined}
        >
          {ADDON_STATUS_LABELS[addon.status]}
        </Badge>
      }
      edit={
        <>
          <RemoveButton
            kind="addon"
            code={addon.code}
            name={addon.name}
            blockedBy={addon.removal_blocked_by}
          />
          <AddonDialog addon={addon}>
            <Button size="icon-sm" variant="ghost" aria-label={`Edit ${addon.name}`}>
              <Pencil />
            </Button>
          </AddonDialog>
        </>
      }
      stats={
        <>
          <span>
            Up to {addon.max_quantity} {addon.unit}s per workspace
          </span>
          <span>{addon.subscribers} paying</span>
          {archivedNote(addon.archived_prices)}
          {addon.status === "available" && addon.prices.length === 0 && (
            <span className="font-medium text-low">
              No price yet, so the Plan page shows it as coming soon
            </span>
          )}
        </>
      }
    >
      {INTERVALS.map((interval) => (
        <PriceRow
          key={interval.value}
          kind="addon"
          code={addon.code}
          name={addon.name}
          interval={interval.value}
          label={interval.label}
          price={addon.prices.find((price) => price.interval === interval.value)}
          currency={currency}
          unit={addon.unit}
          billingEnabled={billingEnabled}
        />
      ))}
    </ProductFrame>
  );
}

export function NewProductButton({ kind }: { kind: Kind }) {
  const button = (
    <Button size="sm" variant="outline">
      <Plus />
      {kind === "plan" ? "New plan" : "New add-on"}
    </Button>
  );
  return kind === "plan" ? <PlanDialog>{button}</PlanDialog> : <AddonDialog>{button}</AddonDialog>;
}

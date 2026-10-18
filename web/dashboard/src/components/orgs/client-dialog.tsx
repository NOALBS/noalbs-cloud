import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, type ReactNode, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { Client, Organization } from "@/api/types";
import { SlugField, useSlugStatus } from "@/components/slug-field";
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
import { errorMessage } from "@/lib/errors";
import {
  belowUsage,
  DEFAULT_ALLOCATION,
  digitsOnly,
  MAX_ALLOCATION,
  parseAllocation,
} from "@/lib/organizations";

export function AllocationField({
  value,
  onChange,
  invalid,
  pool,
  inUse = 0,
}: {
  value: string;
  onChange: (value: string) => void;
  invalid: boolean;
  pool: number;
  /** The switchers the client has now: the allocation can't go below them. */
  inUse?: number;
}) {
  const parsed = parseAllocation(value);
  const tooLow = parsed !== undefined && belowUsage(parsed, inUse);
  return (
    <div className="flex flex-col gap-2">
      <Label htmlFor="client-allocation">Switchers</Label>
      <Input
        id="client-allocation"
        value={value}
        onChange={(event) => onChange(digitsOnly(event.target.value))}
        inputMode="numeric"
        maxLength={String(MAX_ALLOCATION).length}
        placeholder={`No cap (the pool of ${pool})`}
        aria-invalid={invalid || tooLow}
        className="h-10"
      />
      <p
        className={invalid || tooLow ? "text-xs text-destructive" : "text-xs text-muted-foreground"}
      >
        {invalid
          ? `A whole number from 0 to ${MAX_ALLOCATION}, or empty.`
          : tooLow
            ? `${inUse} in use: delete some first, or keep at least that many.`
            : "The most switchers this client can have. Leave empty to let the pool decide."}
      </p>
    </div>
  );
}

/** Creates a client workspace: no owner, no plan; the organization runs it. */
export function NewClientDialog({
  organization,
  children,
}: {
  organization: Organization;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [customSlug, setCustomSlug] = useState<string | null>(null);
  const [allocation, setAllocation] = useState(String(DEFAULT_ALLOCATION));
  const slug = useSlugStatus(name, customSlug);
  const parsed = parseAllocation(allocation);
  const queryClient = useQueryClient();

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/organizations/{organization_id}/clients", {
          params: { path: { organization_id: organization.id } },
          body: {
            name: name.trim(),
            url_slug: slug.slug,
            switcher_allocation: parsed ?? null,
          },
        }),
      ),
    onSuccess: async (client) => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.clients(organization.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.organizations }),
      ]);
      toast.success(`${client.name} was added`);
      setOpen(false);
      setName("");
      setCustomSlug(null);
      setAllocation(String(DEFAULT_ALLOCATION));
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate();
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>New client</DialogTitle>
            <DialogDescription>
              A workspace for one of your customers. You run it from here; invite the customer when
              they should get in.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-2">
            <Label htmlFor="client-name">Name</Label>
            <Input
              id="client-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              maxLength={60}
              placeholder="Nomad Live"
              autoFocus
              className="h-10"
            />
          </div>
          <SlugField status={slug} onChange={setCustomSlug} />
          <AllocationField
            value={allocation}
            onChange={setAllocation}
            invalid={parsed === undefined}
            pool={organization.limits.max_switchers}
          />
          {create.error && <p className="text-sm text-destructive">{errorMessage(create.error)}</p>}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={!name.trim() || !slug.ready || parsed === undefined || create.isPending}
            >
              {create.isPending ? "Creating…" : "Create client"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Renames a client and changes how many switchers it may have. */
export function EditClientDialog({
  organization,
  client,
  children,
}: {
  organization: Organization;
  client: Client;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(client.name);
  const [allocation, setAllocation] = useState(client.switcher_allocation?.toString() ?? "");
  const parsed = parseAllocation(allocation);
  const queryClient = useQueryClient();

  const save = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/organizations/{organization_id}/clients/{workspace_id}", {
          params: { path: { organization_id: organization.id, workspace_id: client.id } },
          body: { name: name.trim(), switcher_allocation: parsed ?? null },
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.clients(organization.id) }),
        queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
      ]);
      toast.success("Saved");
      setOpen(false);
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) {
          setName(client.name);
          setAllocation(client.switcher_allocation?.toString() ?? "");
          save.reset();
        }
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>Edit {client.name}</DialogTitle>
            <DialogDescription>
              The allocation can’t go below the switchers the client has. A limit never stops a
              running switcher.
            </DialogDescription>
          </DialogHeader>
          <div className="flex flex-col gap-2">
            <Label htmlFor="client-name">Name</Label>
            <Input
              id="client-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              maxLength={60}
              className="h-10"
            />
          </div>
          <AllocationField
            value={allocation}
            onChange={setAllocation}
            invalid={parsed === undefined}
            pool={organization.limits.max_switchers}
            inUse={client.switchers}
          />
          {save.error && <p className="text-sm text-destructive">{errorMessage(save.error)}</p>}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={
                !name.trim() ||
                parsed === undefined ||
                belowUsage(parsed, client.switchers) ||
                save.isPending
              }
            >
              {save.isPending ? "Saving…" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

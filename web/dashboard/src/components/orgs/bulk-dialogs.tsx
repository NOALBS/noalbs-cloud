import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Download } from "lucide-react";
import { type FormEvent, type ReactNode, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type {
  BulkCreateResult,
  BulkError,
  BulkInvitesResult,
  BulkResult,
  Organization,
} from "@/api/types";
import { CopyField } from "@/components/copy-field";
import { AllocationField } from "@/components/orgs/client-dialog";
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
import { Label } from "@/components/ui/label";
import { errorMessage } from "@/lib/errors";
import {
  BULK_MAX,
  bulkOutcome,
  customerInviteUrl,
  DEFAULT_ALLOCATION,
  invitesCsv,
  parseAllocation,
  parseNames,
} from "@/lib/organizations";

/** Everything a bulk action may have changed. */
function useRefreshClients(organization: Organization) {
  const queryClient = useQueryClient();
  return () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.clients(organization.id) }),
      queryClient.invalidateQueries({ queryKey: queryKeys.organizations }),
      queryClient.invalidateQueries({ queryKey: queryKeys.workspaces }),
    ]);
}

/** The items of a bulk action that failed, with why. */
function Failures({ items }: { items: { name: string; error?: BulkError | null }[] }) {
  const failed = items.filter((item) => item.error);
  if (failed.length === 0) return null;
  return (
    <ul className="flex max-h-40 flex-col gap-1 overflow-y-auto rounded-xl border border-destructive/40 bg-destructive/10 p-3 text-sm">
      {failed.map((item, index) => (
        // The same name can fail more than once, so the index is part of the key.
        // biome-ignore lint/suspicious/noArrayIndexKey: a static list of results
        <li key={`${item.name}-${index}`}>
          <span className="font-semibold">{item.name || "A client"}</span>: {item.error?.message}
        </li>
      ))}
    </ul>
  );
}

const TEXTAREA =
  "min-h-40 w-full rounded-md border border-input bg-transparent px-3 py-2 font-mono text-sm outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50";

/** Creates many clients from a pasted list of names, one per line. */
export function ImportClientsDialog({
  organization,
  children,
}: {
  organization: Organization;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [allocation, setAllocation] = useState(String(DEFAULT_ALLOCATION));
  const [result, setResult] = useState<BulkCreateResult | null>(null);
  const refresh = useRefreshClients(organization);
  const { names, tooMany } = parseNames(text);
  const parsed = parseAllocation(allocation);

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/organizations/{organization_id}/clients/bulk-create", {
          params: { path: { organization_id: organization.id } },
          body: { names, switcher_allocation: parsed ?? null },
        }),
      ),
    onSuccess: async (created) => {
      setResult(created);
      await refresh();
    },
  });

  const reset = () => {
    setText("");
    setResult(null);
    create.reset();
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    create.mutate();
  };
  const outcome = result ? bulkOutcome(result.items) : null;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) reset();
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        {result && outcome ? (
          <div className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>
                {outcome.ok} client{outcome.ok === 1 ? "" : "s"} created
              </DialogTitle>
              <DialogDescription>
                {outcome.failed > 0
                  ? `${outcome.failed} could not be created.`
                  : "Select them in the list to make their invite links."}
              </DialogDescription>
            </DialogHeader>
            <Failures items={result.items} />
            <DialogFooter>
              <Button onClick={() => setOpen(false)}>Done</Button>
            </DialogFooter>
          </div>
        ) : (
          <form onSubmit={submit} className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>Import clients</DialogTitle>
              <DialogDescription>
                One client per line, up to {BULK_MAX}. Each URL follows its name (a number is added
                when it’s taken).
              </DialogDescription>
            </DialogHeader>
            <div className="flex flex-col gap-2">
              <Label htmlFor="import-names">Names</Label>
              <textarea
                id="import-names"
                value={text}
                onChange={(event) => setText(event.target.value)}
                placeholder={"Nomad Live\nBike Crew\nKate IRL"}
                className={TEXTAREA}
                spellCheck={false}
              />
              <p className={tooMany ? "text-xs text-destructive" : "text-xs text-muted-foreground"}>
                {tooMany
                  ? `Only the first ${BULK_MAX} are imported: do the rest in another go.`
                  : `${names.length} client${names.length === 1 ? "" : "s"}`}
              </p>
            </div>
            <AllocationField
              value={allocation}
              onChange={setAllocation}
              invalid={parsed === undefined}
              pool={organization.limits.max_switchers}
            />
            {create.error && (
              <p className="text-sm text-destructive">{errorMessage(create.error)}</p>
            )}
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button
                type="submit"
                disabled={names.length === 0 || parsed === undefined || create.isPending}
              >
                {create.isPending
                  ? "Creating…"
                  : `Create ${names.length} client${names.length === 1 ? "" : "s"}`}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Makes a customer invite link for each selected client; shown once. */
export function BulkInvitesDialog({
  organization,
  ids,
  children,
}: {
  organization: Organization;
  ids: string[];
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [result, setResult] = useState<BulkInvitesResult | null>(null);
  const refresh = useRefreshClients(organization);

  const create = useMutation({
    mutationFn: () =>
      unwrap(
        api.POST("/organizations/{organization_id}/clients/bulk-invites", {
          params: { path: { organization_id: organization.id } },
          body: { workspace_ids: ids, expires_in_days: 7 },
        }),
      ),
    onSuccess: async (made) => {
      setResult(made);
      await refresh();
    },
  });

  const links = result?.items.filter((item) => item.token) ?? [];
  const origin = window.location.origin;
  const copyAll = async () => {
    const lines = links.map(
      (item) => `${item.name}: ${customerInviteUrl(origin, item.token ?? "")}`,
    );
    await navigator.clipboard.writeText(lines.join("\n"));
    toast.success("Links copied");
  };
  const download = () => {
    const blob = new Blob([invitesCsv(result?.items ?? [], origin)], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${organization.url_slug}-invites.csv`;
    anchor.click();
    URL.revokeObjectURL(url);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) {
          setResult(null);
          create.reset();
        }
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>
            Invite links for {ids.length} client{ids.length === 1 ? "" : "s"}
          </DialogTitle>
          <DialogDescription>
            Each customer signs in with Twitch, Kick or YouTube and gets in as an admin. The links
            expire after 7 days and are shown only now: copy or download them before closing.
          </DialogDescription>
        </DialogHeader>

        {!result ? (
          <>
            {create.error && (
              <p className="text-sm text-destructive">{errorMessage(create.error)}</p>
            )}
            <DialogFooter>
              <Button variant="outline" onClick={() => setOpen(false)}>
                Cancel
              </Button>
              <Button onClick={() => create.mutate()} disabled={create.isPending}>
                {create.isPending ? "Making links…" : "Make the links"}
              </Button>
            </DialogFooter>
          </>
        ) : (
          <div className="flex flex-col gap-3">
            <ul className="flex max-h-72 flex-col gap-2 overflow-y-auto">
              {links.map((item) => (
                <li key={item.workspace_id} className="flex flex-col gap-1">
                  <span className="text-sm font-semibold">{item.name}</span>
                  <CopyField
                    value={customerInviteUrl(origin, item.token ?? "")}
                    label={`Invite link for ${item.name}`}
                    secret
                  />
                </li>
              ))}
            </ul>
            <Failures items={result.items} />
            <DialogFooter>
              {links.length > 0 && (
                <>
                  <Button variant="outline" onClick={() => void copyAll()}>
                    Copy all
                  </Button>
                  <Button variant="outline" onClick={download}>
                    <Download />
                    Download CSV
                  </Button>
                </>
              )}
              <Button onClick={() => setOpen(false)}>Done</Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** Sets the switcher allocation of the selected clients. */
export function BulkAllocationDialog({
  organization,
  ids,
  children,
}: {
  organization: Organization;
  ids: string[];
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [allocation, setAllocation] = useState(String(DEFAULT_ALLOCATION));
  const [result, setResult] = useState<BulkResult | null>(null);
  const refresh = useRefreshClients(organization);
  const parsed = parseAllocation(allocation);

  const update = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/organizations/{organization_id}/clients/bulk-allocation", {
          params: { path: { organization_id: organization.id } },
          body: { workspace_ids: ids, switcher_allocation: parsed ?? null },
        }),
      ),
    onSuccess: async (done) => {
      setResult(done);
      await refresh();
      if (bulkOutcome(done.items).failed === 0) {
        toast.success("Allocation set");
        setOpen(false);
      }
    },
  });
  const outcome = result ? bulkOutcome(result.items) : null;

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) {
          setResult(null);
          update.reset();
        }
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            update.mutate();
          }}
          className="flex flex-col gap-4"
        >
          <DialogHeader>
            <DialogTitle>
              Allocation for {ids.length} client{ids.length === 1 ? "" : "s"}
            </DialogTitle>
            <DialogDescription>
              The same number of switchers for each. One can’t go below the switchers it has.
            </DialogDescription>
          </DialogHeader>
          <AllocationField
            value={allocation}
            onChange={setAllocation}
            invalid={parsed === undefined}
            pool={organization.limits.max_switchers}
          />
          {outcome && outcome.failed > 0 && (
            <>
              <p className="text-sm">
                {outcome.ok} changed, {outcome.failed} not:
              </p>
              <Failures items={result?.items ?? []} />
            </>
          )}
          {update.error && <p className="text-sm text-destructive">{errorMessage(update.error)}</p>}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setOpen(false)}>
              {outcome ? "Close" : "Cancel"}
            </Button>
            <Button type="submit" disabled={parsed === undefined || update.isPending}>
              {update.isPending ? "Saving…" : "Set allocation"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

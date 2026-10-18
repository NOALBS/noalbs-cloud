import { useMutation, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { AdminNotice } from "@/api/types";
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
import { Switch } from "@/components/ui/switch";
import { errorMessage } from "@/lib/errors";
import { fromLocalInput, SEVERITIES, toLocalInput } from "@/lib/notices";
import { cn } from "@/lib/utils";

const MAX_MESSAGE = 500;

/** Posts a notice, or changes `notice`. The trigger is `children`. */
export function NoticeDialog({ notice, children }: { notice?: AdminNotice; children: ReactNode }) {
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [message, setMessage] = useState(notice?.message ?? "");
  const [severity, setSeverity] = useState<string>(notice?.severity ?? "info");
  // A notice that already started keeps its start; a new one starts now unless set.
  const [startsAt, setStartsAt] = useState(
    notice && new Date(notice.starts_at) <= new Date() ? toLocalInput(notice.starts_at) : "",
  );
  const [endsAt, setEndsAt] = useState(toLocalInput(notice?.ends_at ?? null));
  const [dismissible, setDismissible] = useState(notice?.dismissible ?? true);

  const save = useMutation({
    mutationFn: () => {
      const body = {
        message: message.trim(),
        severity,
        starts_at: fromLocalInput(startsAt),
        ends_at: fromLocalInput(endsAt),
        dismissible: dismissible && severity !== "critical",
      };
      return notice
        ? unwrap(
            api.PUT("/admin/notices/{notice_id}", {
              params: { path: { notice_id: notice.id } },
              body,
            }),
          )
        : unwrap(api.POST("/admin/notices", { body }));
    },
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.adminNotices }),
        queryClient.invalidateQueries({ queryKey: queryKeys.notices }),
      ]);
      toast.success(notice ? "Notice updated" : "Notice posted");
      setOpen(false);
      if (!notice) {
        setMessage("");
        setStartsAt("");
        setEndsAt("");
      }
    },
  });

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (next) save.reset();
      }}
    >
      <DialogTrigger asChild>{children}</DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{notice ? "Edit notice" : "New notice"}</DialogTitle>
          <DialogDescription>
            Shown at the top of the dashboard to everyone who is signed in. Plain text only.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-2">
            <Label htmlFor="notice-message">Message</Label>
            <textarea
              id="notice-message"
              value={message}
              onChange={(event) => setMessage(event.target.value)}
              maxLength={MAX_MESSAGE}
              rows={3}
              className="w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
            />
            <span className="text-xs text-muted-foreground">
              {message.length}/{MAX_MESSAGE}
            </span>
          </div>

          <fieldset className="flex flex-col gap-2">
            <legend className="mb-2 text-sm leading-none font-medium">Severity</legend>
            <div className="flex gap-1.5">
              {SEVERITIES.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  aria-pressed={severity === option.value}
                  onClick={() => setSeverity(option.value)}
                  className={cn(
                    "rounded-full border px-3.5 py-1.5 text-sm font-medium",
                    severity === option.value
                      ? "border-primary bg-primary-soft text-primary-soft-foreground"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {option.label}
                </button>
              ))}
            </div>
          </fieldset>

          <div className="grid gap-4 sm:grid-cols-2">
            <div className="flex flex-col gap-2">
              <Label htmlFor="notice-starts">Starts (empty = now)</Label>
              <Input
                id="notice-starts"
                type="datetime-local"
                value={startsAt}
                onChange={(event) => setStartsAt(event.target.value)}
              />
            </div>
            <div className="flex flex-col gap-2">
              <Label htmlFor="notice-ends">Ends (empty = until ended)</Label>
              <Input
                id="notice-ends"
                type="datetime-local"
                value={endsAt}
                onChange={(event) => setEndsAt(event.target.value)}
              />
            </div>
          </div>

          <div className="flex items-center justify-between gap-3">
            <Label htmlFor="notice-dismissible" className="flex flex-col items-start gap-0.5">
              Users can hide it
              {severity === "critical" && (
                <span className="text-xs font-normal text-muted-foreground">
                  Critical notices can’t be hidden.
                </span>
              )}
            </Label>
            <Switch
              id="notice-dismissible"
              checked={dismissible && severity !== "critical"}
              disabled={severity === "critical"}
              onCheckedChange={setDismissible}
            />
          </div>

          {save.isError && <p className="text-sm text-destructive">{errorMessage(save.error)}</p>}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button onClick={() => save.mutate()} disabled={!message.trim() || save.isPending}>
            {notice ? "Save" : "Post notice"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

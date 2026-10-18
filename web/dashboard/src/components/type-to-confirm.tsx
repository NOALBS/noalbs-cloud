import { type ReactNode, useId, useState } from "react";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { errorMessage } from "@/lib/errors";

/**
 * For things that can't be undone (deleting a workspace or an account, handing it over): the
 * button only works once `phrase` is typed. The dialog stays open on failure
 * and shows why.
 */
export function TypeToConfirm({
  trigger,
  title,
  description,
  phrase,
  confirmLabel,
  pendingLabel = "Deleting…",
  disabled = false,
  onConfirm,
}: {
  trigger: string;
  title: string;
  description: ReactNode;
  /** What to type, e.g. the workspace's URL name. */
  phrase: string;
  confirmLabel: string;
  pendingLabel?: string;
  /** Keeps the trigger button disabled, e.g. until something is picked. */
  disabled?: boolean;
  onConfirm: () => Promise<unknown>;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [typed, setTyped] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>(null);

  const confirm = async () => {
    setPending(true);
    setError(null);
    try {
      await onConfirm();
      setOpen(false);
    } catch (failure) {
      setError(failure);
    } finally {
      setPending(false);
    }
  };

  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) {
          setTyped("");
          setError(null);
        }
      }}
    >
      <AlertDialogTrigger asChild>
        <Button
          variant="outline"
          className="border-destructive/50 text-destructive-foreground"
          disabled={disabled}
        >
          {trigger}
        </Button>
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{title}</AlertDialogTitle>
          <AlertDialogDescription asChild>
            <div className="flex flex-col gap-2">{description}</div>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <div className="flex flex-col gap-2">
          <Label htmlFor={id}>
            Type <span className="font-mono">{phrase}</span> to confirm
          </Label>
          <Input
            id={id}
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
          {error != null && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
        </div>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>Cancel</AlertDialogCancel>
          <Button
            variant="destructive"
            disabled={typed.trim() !== phrase || pending}
            onClick={() => void confirm()}
          >
            {pending ? pendingLabel : confirmLabel}
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

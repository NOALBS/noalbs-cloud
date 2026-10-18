import { useNavigate } from "@tanstack/react-router";
import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { grantErrorMessage } from "@/lib/chat";
import { platformLabel } from "@/lib/platforms";

/** The search params the API adds when a chat grant comes back. */
export interface GrantSearch {
  chat_connected?: string;
  chat_error?: string;
}

export function validateGrantSearch(search: Record<string, unknown>): GrantSearch {
  return {
    chat_connected: typeof search.chat_connected === "string" ? search.chat_connected : undefined,
    chat_error: typeof search.chat_error === "string" ? search.chat_error : undefined,
  };
}

/** Says how a chat grant went, then tidies the URL. */
export function useGrantOutcome(search: GrantSearch, connected: (platform: string) => string) {
  const navigate = useNavigate();
  const shown = useRef(false);
  const label = useRef(connected);
  label.current = connected;

  useEffect(() => {
    if (shown.current || (!search.chat_connected && !search.chat_error)) return;
    shown.current = true;
    if (search.chat_connected) {
      toast.success(label.current(platformLabel(search.chat_connected)));
    } else if (search.chat_error) {
      toast.error(grantErrorMessage(search.chat_error));
    }
    void navigate({ to: ".", search: {}, replace: true });
  }, [search.chat_connected, search.chat_error, navigate]);
}

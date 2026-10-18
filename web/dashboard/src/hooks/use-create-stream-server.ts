import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import type { ServerDetailsInput, StreamServerKind } from "@/api/types";

export interface NewStreamServer {
  name: string;
  kind: StreamServerKind;
  stats_url: string;
  details: ServerDetailsInput;
}

/** Adds a stream server to a switcher and refreshes what shows it. */
export function useCreateStreamServer(path: { workspace_id: string; switcher_id: string }) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (body: NewStreamServer) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/stream-servers", {
          params: { path },
          body,
        }),
      ),
    onSuccess: async (server) => {
      await queryClient.invalidateQueries({
        queryKey: queryKeys.streamServers(path.workspace_id, path.switcher_id),
      });
      await queryClient.invalidateQueries({ queryKey: queryKeys.switchers(path.workspace_id) });
      toast.success(`Added ${server.name}`);
    },
  });
}

import { t } from "@lingui/core/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { apiWithTimeout, unwrap } from "@/api/client";
import { queryKeys } from "@/api/queries";
import { errorMessage } from "@/lib/errors";

export type ConnectObsAction = "start" | "stop" | "restart";

const OBS_COMMAND_TIMEOUT_MS = 40_000;
const connectObsApi = apiWithTimeout(OBS_COMMAND_TIMEOUT_MS);

export function useConnectObsCommand(path: { workspace_id: string; switcher_id: string }) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (action: ConnectObsAction) =>
      unwrap(
        connectObsApi.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/connect/obs", {
          params: { path },
          body: { action },
        }),
      ),
    onSuccess: async (_, action) => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: queryKeys.switcher(path.workspace_id, path.switcher_id),
        }),
        queryClient.invalidateQueries({
          queryKey: queryKeys.switchers(path.workspace_id),
        }),
      ]);
      toast.success(
        {
          start: t`OBS started`,
          stop: t`OBS stopped`,
          restart: t`OBS restarted`,
        }[action],
      );
    },
    onError: (error) => toast.error(errorMessage(error)),
  });
}

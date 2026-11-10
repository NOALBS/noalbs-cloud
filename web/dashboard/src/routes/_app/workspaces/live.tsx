import { t } from "@lingui/core/macro";
import { useQuery } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useCallback, useMemo } from "react";
import { myLiveQuery } from "@/api/queries";
import { LiveList } from "@/components/operator/live-list";
import { normalizeMyLive } from "@/components/operator/model";
import { errorMessage } from "@/lib/errors";
import { parseLiveSearch } from "@/lib/organizations";

export const Route = createFileRoute("/_app/workspaces/live")({
  validateSearch: parseLiveSearch,
  component: LivePage,
});

function LivePage() {
  const search = Route.useSearch();
  const { q = "", page = 1, show } = search;
  const navigate = useNavigate({ from: Route.fullPath });
  const live = useQuery({
    ...myLiveQuery({ q, page, filter: show }),
    refetchIntervalInBackground: false,
  });
  const data = useMemo(() => (live.data ? normalizeMyLive(live.data) : undefined), [live.data]);
  const onSearch = useCallback(
    (value: string) =>
      void navigate({
        search: (previous) => ({ ...previous, q: value || undefined, page: undefined }),
        replace: true,
      }),
    [navigate],
  );

  return (
    <section className="flex w-full min-w-0 flex-col px-4 py-6 md:px-8" aria-label={t`Live`}>
      <LiveList
        data={data}
        pending={live.isPending}
        error={live.isError ? errorMessage(live.error) : undefined}
        updatedAt={live.dataUpdatedAt}
        scope="workspaces"
        search={search}
        onSearch={onSearch}
        onOpen={(pin) =>
          void navigate({
            search: (previous) => ({
              ...previous,
              open: pin?.switcher,
              open_workspace: pin?.workspace,
            }),
          })
        }
        onShow={(value) =>
          void navigate({ search: (previous) => ({ ...previous, show: value, page: undefined }) })
        }
        onPage={(next) => void navigate({ search: (previous) => ({ ...previous, page: next }) })}
      />
    </section>
  );
}

import { t } from "@lingui/core/macro";
import { Trans } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import { Plus, X } from "lucide-react";
import { type FormEvent, useState } from "react";
import { toast } from "sonner";
import { api, unwrap } from "@/api/client";
import { chatQuery, queryKeys } from "@/api/queries";
import type { ChatCommand, ChatRole, ChatSettings } from "@/api/types";
import { BotSection, ChannelsSection, IgnoredSection } from "@/components/chat/chat-links";
import { Section } from "@/components/page";
import { ReadOnlyNotice } from "@/components/settings";
import { IndexedPage } from "@/components/switcher-page";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { useGrantOutcome, validateGrantSearch } from "@/hooks/use-grant-outcome";
import { useSwitcher } from "@/hooks/use-switcher";
import { errorMessage } from "@/lib/errors";
import { cn } from "@/lib/utils";

export const Route = createFileRoute("/_app/$workspace/s/$switcher/chat")({
  validateSearch: validateGrantSearch,
  component: ChatPage,
});

const roleLabels = (): Record<ChatRole, string> => ({
  public: t`Everyone`,
  subscriber: t`Subscribers`,
  vip: t`VIPs`,
  moderator: t`Moderators`,
  admin: t`Admins`,
});

const LANGUAGE_NAMES: Record<string, string> = {
  en: "English",
  da: "Dansk",
  de: "Deutsch",
  es: "Español",
  fr: "Français",
  it: "Italiano",
  ja: "日本語",
  ko: "한국어",
  nb: "Norsk bokmål",
  nl: "Nederlands",
  pl: "Polski",
  "pt-br": "Português (Brasil)",
  ru: "Русский",
  sv: "Svenska",
  tr: "Türkçe",
  uk: "Українська",
  "zh-cn": "简体中文",
  "zh-tw": "繁體中文",
};

type Filter = "all" | ChatRole;
const FILTERS: Filter[] = ["all", "admin", "moderator", "vip", "subscriber", "public"];
type Path = { workspace_id: string; switcher_id: string };

function ChatPage() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const chat = useQuery(chatQuery(workspace.id, switcher.id));
  useGrantOutcome(Route.useSearch(), (platform) => t`${platform} connected`);
  const [filter, setFilter] = useState<Filter>("all");

  const commands = chat.data?.commands.filter(
    (command) => filter === "all" || command.role === filter,
  );
  const prefix = chat.data?.settings.command_prefix ?? "!";
  // No command is for subscribers or VIPs by default: show those only once one is.
  const filters = FILTERS.filter(
    (option) =>
      (option !== "vip" && option !== "subscriber") ||
      option === filter ||
      chat.data?.commands.some((command) => command.role === option),
  );

  return (
    <IndexedPage
      title={t`Chat`}
      description={t`What viewers and mods can do from chat, and who answers them.`}
      lead={!canManage && <ReadOnlyNotice />}
      sections={[
        {
          id: "channels",
          title: t`Channels`,
          count: chat.data?.channels.length,
          content: <ChannelsSection channels={chat.data?.channels} />,
        },
        { id: "bot", title: t`Bot`, content: <BotSection /> },
        {
          id: "settings",
          title: t`Settings`,
          content: (
            <div className="flex flex-col gap-4">
              {chat.data ? (
                <ChatSettingsSection
                  key={JSON.stringify(chat.data.settings)}
                  settings={chat.data.settings}
                  languages={chat.data.languages}
                  canManage={canManage}
                  path={path}
                />
              ) : (
                <Skeleton className="h-40 rounded-2xl" />
              )}
              <IgnoredSection ignored={chat.data?.ignored} />
            </div>
          ),
        },
        {
          id: "commands",
          title: t`Commands`,
          count: chat.data?.commands.length,
          content: (
            <Section
              title={t`Commands`}
              description={t`Turn commands on or off, choose who may use them and add shorter names. Roles build on each other (Everyone → Subscribers → VIPs → Moderators → Admins): a command for Subscribers also works for VIPs, moderators and admins. Admins are the channel owner and every member of this workspace, on the accounts linked to their NOALBS sign-in.`}
              actions={
                <div
                  role="radiogroup"
                  aria-label={t`Show commands for`}
                  className="flex flex-wrap gap-1.5"
                >
                  {filters.map((option) => (
                    <label
                      key={option}
                      className="flex h-8 cursor-pointer items-center rounded-md px-3 text-xs font-semibold text-muted-foreground has-checked:border-primary has-checked:bg-primary-soft has-checked:text-foreground has-focus-visible:ring-2 has-focus-visible:ring-ring"
                    >
                      <input
                        type="radio"
                        name="command-filter"
                        className="sr-only"
                        checked={filter === option}
                        onChange={() => setFilter(option)}
                      />
                      {option === "all"
                        ? t`All ${chat.data?.commands.length ?? ""}`
                        : roleLabels()[option]}
                    </label>
                  ))}
                </div>
              }
            >
              <div className="overflow-x-auto">
                <table className="w-full min-w-[680px] text-left">
                  <thead className="border-t border-divider text-xs text-muted-foreground">
                    <tr>
                      <th className="px-5 py-3 font-medium">
                        <Trans>Command & aliases</Trans>
                      </th>
                      <th className="px-5 py-3 font-medium">
                        <Trans>Description</Trans>
                      </th>
                      <th className="px-5 py-3 font-medium">
                        <Trans>Who can use it</Trans>
                      </th>
                      <th className="px-5 py-3 font-medium">
                        <Trans>Enabled</Trans>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {commands?.map((command) => (
                      <CommandRow
                        key={command.command}
                        command={command}
                        prefix={prefix}
                        canManage={canManage}
                        path={path}
                      />
                    ))}
                  </tbody>
                </table>
              </div>
            </Section>
          ),
        },
      ]}
    />
  );
}

function ChatSettingsSection({
  settings,
  languages,
  canManage,
  path,
}: {
  settings: ChatSettings;
  languages: string[];
  canManage: boolean;
  path: Path;
}) {
  const queryClient = useQueryClient();
  const [form, setForm] = useState(settings);
  const dirty = JSON.stringify(form) !== JSON.stringify(settings);

  const save = useMutation({
    mutationFn: () =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/chat/settings", {
          params: { path },
          body: form,
        }),
      ),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: queryKeys.chat(path.workspace_id, path.switcher_id),
        }),
        queryClient.invalidateQueries({
          queryKey: queryKeys.switcher(path.workspace_id, path.switcher_id),
        }),
      ]);
      toast.success(t`Saved`);
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate();
  };

  return (
    <Section title={t`Settings`}>
      <form onSubmit={submit} className="flex flex-col">
        <div className="flex flex-col divide-y divide-divider border-t border-divider">
          <div className="grid items-center gap-2 px-5 py-3 sm:grid-cols-[1fr_auto]">
            <Label htmlFor="chat-prefix">
              <Trans>Command prefix</Trans>
            </Label>
            <p className="text-xs text-muted-foreground sm:col-start-1 sm:row-start-2">{t`Commands start with this character.`}</p>
            <Input
              id="chat-prefix"
              value={form.command_prefix}
              onChange={(event) => setForm({ ...form, command_prefix: event.target.value })}
              maxLength={3}
              disabled={!canManage}
              className="w-24 font-mono sm:col-start-2 sm:row-span-2 sm:row-start-1"
            />
          </div>
          {languages.length > 1 && (
            <div className="grid items-center gap-2 px-5 py-3 sm:grid-cols-[1fr_auto]">
              <Label htmlFor="chat-language">
                <Trans>Bot language</Trans>
              </Label>
              <Select
                value={form.language}
                onValueChange={(language) => setForm({ ...form, language })}
                disabled={!canManage}
              >
                <SelectTrigger id="chat-language" className="w-56">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {languages.map((code) => (
                    <SelectItem key={code} value={code}>
                      {LANGUAGE_NAMES[code] ?? code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground sm:col-span-2">{t`Bot replies use ${LANGUAGE_NAMES[form.language] ?? form.language}.`}</p>
            </div>
          )}
          <div className="grid items-center gap-2 px-5 py-3 sm:grid-cols-[1fr_auto]">
            <Label htmlFor="chat-reply-mode">
              <Trans>Reply mode</Trans>
            </Label>
            <Select
              value={form.threaded_replies ? "reply" : "message"}
              onValueChange={(mode) => setForm({ ...form, threaded_replies: mode === "reply" })}
              disabled={!canManage}
            >
              <SelectTrigger id="chat-reply-mode" className="w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="reply">
                  <Trans>Reply</Trans>
                </SelectItem>
                <SelectItem value="message">
                  <Trans>Normal message</Trans>
                </SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground sm:col-span-2">
              {form.threaded_replies
                ? t`Replies are threaded under the original message.`
                : t`Replies are posted as normal chat messages.`}
            </p>
          </div>
        </div>
        {(
          [
            [
              "public_commands_enabled",
              t`Public commands`,
              t`Let everyone in chat use commands like !bitrate.`,
            ],
            [
              "mod_commands_enabled",
              t`Moderator commands`,
              t`Let your moderators run moderator commands.`,
            ],
          ] as const
        ).map(([key, title, description]) => (
          <label
            key={key}
            htmlFor={`chat-${key}`}
            className="flex min-h-16 cursor-pointer items-center gap-4 border-t border-divider px-5 py-3"
          >
            <span className="flex flex-1 flex-col gap-0.5">
              <span className="text-sm font-semibold">{title}</span>
              <span className="text-sm text-muted-foreground">{description}</span>
            </span>
            <Switch
              id={`chat-${key}`}
              checked={form[key]}
              onCheckedChange={(checked) => setForm({ ...form, [key]: checked })}
              disabled={!canManage}
            />
          </label>
        ))}
        {canManage && dirty && (
          <div className="flex items-center justify-end gap-3 border-t border-divider px-5 py-3">
            {save.error && (
              <span className="flex-1 text-sm text-destructive">{errorMessage(save.error)}</span>
            )}
            <Button type="button" variant="ghost" onClick={() => setForm(settings)}>
              <Trans>Discard</Trans>
            </Button>
            <Button type="submit" disabled={save.isPending}>
              {save.isPending ? t`Saving…` : t`Save`}
            </Button>
          </div>
        )}
      </form>
    </Section>
  );
}

function CommandRow({
  command,
  prefix,
  canManage,
  path,
}: {
  command: ChatCommand;
  prefix: string;
  canManage: boolean;
  path: Path;
}) {
  const queryClient = useQueryClient();
  const [aliasInput, setAliasInput] = useState<string | null>(null);
  const refresh = () =>
    queryClient.invalidateQueries({
      queryKey: queryKeys.chat(path.workspace_id, path.switcher_id),
    });

  const setCommand = useMutation({
    mutationFn: (change: { enabled: boolean; role: ChatRole }) =>
      unwrap(
        api.PUT("/workspaces/{workspace_id}/switchers/{switcher_id}/chat/commands/{command}", {
          params: { path: { ...path, command: command.command } },
          body: {
            enabled: change.enabled,
            role: change.role === command.default_role ? null : change.role,
          },
        }),
      ),
    onSuccess: refresh,
    onError: (error) => toast.error(errorMessage(error)),
  });

  const addAlias = useMutation({
    mutationFn: (alias: string) =>
      unwrap(
        api.POST("/workspaces/{workspace_id}/switchers/{switcher_id}/chat/aliases", {
          params: { path },
          body: { alias, command: command.command },
        }),
      ),
    onSuccess: async () => {
      setAliasInput(null);
      await refresh();
    },
  });

  const removeAlias = useMutation({
    mutationFn: (alias: string) =>
      unwrap(
        api.DELETE("/workspaces/{workspace_id}/switchers/{switcher_id}/chat/aliases/{alias}", {
          params: { path: { ...path, alias } },
        }),
      ),
    onSuccess: refresh,
    onError: (error) => toast.error(errorMessage(error)),
  });

  const submitAlias = (event: FormEvent) => {
    event.preventDefault();
    if (aliasInput?.trim()) addAlias.mutate(aliasInput);
  };

  return (
    <tr className={cn("border-t border-divider align-top", !command.enabled && "opacity-60")}>
      <td className="w-48 px-5 py-3">
        <p className="font-mono text-sm font-semibold">
          {prefix}
          {command.command}
        </p>
        <div className="flex flex-wrap items-center gap-1.5">
          {command.aliases.map((alias) => (
            <span
              key={alias}
              className="flex items-center gap-1 font-mono text-xs text-muted-foreground"
            >
              {prefix}
              {alias}
              {canManage && (
                <button
                  type="button"
                  onClick={() => removeAlias.mutate(alias)}
                  aria-label={t`Remove alias ${alias}`}
                  className="rounded p-0.5 hover:bg-primary/20"
                >
                  <X className="size-3" />
                </button>
              )}
            </span>
          ))}
          {canManage &&
            (aliasInput === null ? (
              <button
                type="button"
                onClick={() => setAliasInput("")}
                className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs text-muted-foreground hover:bg-surface-2 hover:text-foreground"
              >
                <Plus className="size-3" />
                <Trans>Alias</Trans>
              </button>
            ) : (
              <form onSubmit={submitAlias} className="flex items-center gap-1.5">
                <Input
                  value={aliasInput}
                  onChange={(event) => setAliasInput(event.target.value)}
                  onKeyDown={(event) => event.key === "Escape" && setAliasInput(null)}
                  placeholder={t`alias`}
                  aria-label={t`New alias for ${command.command}`}
                  autoFocus
                  className="h-7 w-28 font-mono text-xs"
                />
                <Button type="submit" size="xs" disabled={addAlias.isPending}>
                  <Trans>Add</Trans>
                </Button>
                {addAlias.error && (
                  <span className="text-xs text-destructive">{errorMessage(addAlias.error)}</span>
                )}
              </form>
            ))}
        </div>
      </td>
      <td className="px-5 py-3 text-sm text-muted-foreground">{command.description}</td>
      <td className="w-48 px-5 py-3">
        <Select
          value={command.role}
          onValueChange={(role) =>
            setCommand.mutate({ enabled: command.enabled, role: role as ChatRole })
          }
          disabled={!canManage || setCommand.isPending}
        >
          <SelectTrigger className="h-9 w-full" aria-label={t`Who can use ${command.command}`}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {(["public", "subscriber", "vip", "moderator", "admin"] as const).map((role) => (
              <SelectItem key={role} value={role}>
                {roleLabels()[role]}
                {role === command.default_role && t` (default)`}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="mt-1 text-xs text-muted-foreground">{t`${roleLabels()[command.role]} and higher roles can use this command.`}</p>
      </td>
      <td className="px-5 py-3">
        <Switch
          checked={command.enabled}
          onCheckedChange={(enabled) => setCommand.mutate({ enabled, role: command.role })}
          disabled={!canManage || setCommand.isPending}
          aria-label={t`Enable ${command.command}`}
        />
      </td>
    </tr>
  );
}

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
import { SwitcherPage } from "@/components/switcher-page";
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

const ROLE_LABELS: Record<ChatRole, string> = {
  public: "Everyone",
  subscriber: "Subscribers",
  vip: "VIPs",
  moderator: "Moderators",
  admin: "Admins",
};

const LANGUAGE_NAMES: Record<string, string> = {
  en: "English",
  de: "Deutsch",
  es: "Español",
  fr: "Français",
  it: "Italiano",
  nl: "Nederlands",
  pl: "Polski",
  "pt-br": "Português (Brasil)",
  sv: "Svenska",
  tr: "Türkçe",
  "zh-tw": "中文（台灣）",
};

/**
 * The languages the bot has messages for (`messages.rs` in the chat service). The API accepts
 * more, but they would all be answered in English, so the picker waits until there's a choice.
 * Add a code here once its messages are written.
 */
const BOT_LANGUAGES = ["en"];

type Filter = "all" | ChatRole;
const FILTERS: Filter[] = ["all", "admin", "moderator", "vip", "subscriber", "public"];
type Path = { workspace_id: string; switcher_id: string };

function ChatPage() {
  const { workspace, switcher, canManage, path } = useSwitcher();
  const chat = useQuery(chatQuery(workspace.id, switcher.id));
  useGrantOutcome(Route.useSearch(), (platform) => `${platform} connected`);
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
    <SwitcherPage
      title="Chat & commands"
      description="What viewers and mods can do from chat."
      backToSettings
      className="max-w-5xl"
    >
      {!canManage && <ReadOnlyNotice />}

      <ChannelsSection channels={chat.data?.channels} />
      <BotSection />

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

      <Section
        title="Commands"
        description="Turn commands on or off, choose who may use them and add shorter names. Roles build on each other (Everyone → Subscribers → VIPs → Moderators → Admins): a command for Subscribers also works for VIPs, moderators and admins."
        actions={
          <div role="radiogroup" aria-label="Show commands for" className="flex flex-wrap gap-1.5">
            {filters.map((option) => (
              <label
                key={option}
                className="flex h-8 cursor-pointer items-center rounded-full border px-3 text-xs font-semibold text-muted-foreground has-checked:border-primary has-checked:bg-primary-soft has-checked:text-foreground has-focus-visible:ring-2 has-focus-visible:ring-ring"
              >
                <input
                  type="radio"
                  name="command-filter"
                  className="sr-only"
                  checked={filter === option}
                  onChange={() => setFilter(option)}
                />
                {option === "all" ? `All ${chat.data?.commands.length ?? ""}` : ROLE_LABELS[option]}
              </label>
            ))}
          </div>
        }
      >
        <ul>
          {commands?.map((command) => (
            <CommandRow
              key={command.command}
              command={command}
              prefix={prefix}
              canManage={canManage}
              path={path}
            />
          ))}
        </ul>
      </Section>

      <IgnoredSection ignored={chat.data?.ignored} />
    </SwitcherPage>
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
  const botLanguages = languages.filter((code) => BOT_LANGUAGES.includes(code));
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
      toast.success("Saved");
    },
  });

  const submit = (event: FormEvent) => {
    event.preventDefault();
    save.mutate();
  };

  return (
    <Section title="Settings">
      <form onSubmit={submit} className="flex flex-col">
        <div className="grid gap-4 px-5 pb-5 sm:grid-cols-2 lg:grid-cols-3">
          <div className="flex flex-col gap-2">
            <Label htmlFor="chat-prefix">Command prefix</Label>
            <Input
              id="chat-prefix"
              value={form.command_prefix}
              onChange={(event) => setForm({ ...form, command_prefix: event.target.value })}
              maxLength={3}
              disabled={!canManage}
              className="w-24 font-mono"
            />
          </div>
          {botLanguages.length > 1 && (
            <div className="flex flex-col gap-2">
              <Label htmlFor="chat-language">Bot language</Label>
              <Select
                value={form.language}
                onValueChange={(language) => setForm({ ...form, language })}
                disabled={!canManage}
              >
                <SelectTrigger id="chat-language" className="w-56">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {botLanguages.map((code) => (
                    <SelectItem key={code} value={code}>
                      {LANGUAGE_NAMES[code] ?? code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="flex flex-col gap-2">
            <Label htmlFor="chat-reply-mode">Reply mode</Label>
            <Select
              value={form.threaded_replies ? "reply" : "message"}
              onValueChange={(mode) => setForm({ ...form, threaded_replies: mode === "reply" })}
              disabled={!canManage}
            >
              <SelectTrigger id="chat-reply-mode" className="w-56">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="reply">Reply</SelectItem>
                <SelectItem value="message">Normal message</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
        {(
          [
            [
              "public_commands_enabled",
              "Public commands",
              "Let everyone in chat use commands like !bitrate.",
            ],
            [
              "mod_commands_enabled",
              "Moderator commands",
              "Let your moderators run moderator commands.",
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
              Discard
            </Button>
            <Button type="submit" disabled={save.isPending}>
              {save.isPending ? "Saving…" : "Save"}
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
    <li
      className={cn(
        "grid gap-3 border-t border-divider px-5 py-3 md:grid-cols-[10rem_minmax(0,1fr)_13rem_3rem] md:items-center",
        !command.enabled && "opacity-60",
      )}
    >
      <span className="flex flex-col">
        <span className="font-mono text-sm font-semibold">
          {prefix}
          {command.command}
        </span>
        <span className="text-xs text-muted-foreground md:hidden">{command.description}</span>
      </span>

      <div className="flex min-w-0 flex-col gap-1.5">
        <span className="hidden text-sm text-muted-foreground md:block">{command.description}</span>
        <div className="flex flex-wrap items-center gap-1.5">
          {command.aliases.map((alias) => (
            <span
              key={alias}
              className="flex items-center gap-1 rounded-md bg-primary-soft py-0.5 pr-1 pl-2 font-mono text-xs text-primary-soft-foreground"
            >
              {prefix}
              {alias}
              {canManage && (
                <button
                  type="button"
                  onClick={() => removeAlias.mutate(alias)}
                  aria-label={`Remove alias ${alias}`}
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
                Alias
              </button>
            ) : (
              <form onSubmit={submitAlias} className="flex items-center gap-1.5">
                <Input
                  value={aliasInput}
                  onChange={(event) => setAliasInput(event.target.value)}
                  onKeyDown={(event) => event.key === "Escape" && setAliasInput(null)}
                  placeholder="alias"
                  aria-label={`New alias for ${command.command}`}
                  autoFocus
                  className="h-7 w-28 font-mono text-xs"
                />
                <Button type="submit" size="xs" disabled={addAlias.isPending}>
                  Add
                </Button>
                {addAlias.error && (
                  <span className="text-xs text-destructive">{errorMessage(addAlias.error)}</span>
                )}
              </form>
            ))}
        </div>
      </div>

      <Select
        value={command.role}
        onValueChange={(role) =>
          setCommand.mutate({ enabled: command.enabled, role: role as ChatRole })
        }
        disabled={!canManage || setCommand.isPending}
      >
        <SelectTrigger className="h-9 w-full" aria-label={`Who can use ${command.command}`}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {(["public", "subscriber", "vip", "moderator", "admin"] as const).map((role) => (
            <SelectItem key={role} value={role}>
              {ROLE_LABELS[role]}
              {role === command.default_role && " (default)"}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <Switch
        checked={command.enabled}
        onCheckedChange={(enabled) => setCommand.mutate({ enabled, role: command.role })}
        disabled={!canManage || setCommand.isPending}
        aria-label={`Enable ${command.command}`}
      />
    </li>
  );
}

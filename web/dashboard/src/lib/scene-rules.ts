import type { SceneRule, StreamServer } from "@/api/types";

type Servers = Pick<StreamServer, "id" | "name" | "enabled">[];
type Rule = Pick<SceneRule, "id" | "enabled" | "server_ids">;

/** "S1 + S3": the rule's servers in their priority order. */
export function ruleLabel(rule: Pick<SceneRule, "server_ids">, servers: Servers): string {
  return servers
    .filter((server) => rule.server_ids.includes(server.id))
    .map((server) => server.name)
    .join(" + ");
}

const sameSet = (a: string[], b: string[]) =>
  a.length === b.length && a.every((id) => b.includes(id));

/**
 * The enabled rule above `rules[index]` that always matches first: all of its
 * servers are among this one's, so whenever this one could apply, it does.
 */
export function shadowedBy<T extends Rule>(rules: T[], index: number): T | undefined {
  const rule = rules[index];
  if (!rule) return undefined;
  return rules
    .slice(0, index)
    .find((above) => above.enabled && above.server_ids.every((id) => rule.server_ids.includes(id)));
}

/** Another rule with exactly these servers, in any order. */
export function findDuplicate<T extends Rule>(
  rules: T[],
  serverIds: string[],
  exceptId?: string,
): T | undefined {
  return rules.find((rule) => rule.id !== exceptId && sameSet(rule.server_ids, serverIds));
}

/** The rule's servers that are turned off: while any is, the rule never applies. */
export function disabledServers(rule: Pick<SceneRule, "server_ids">, servers: Servers): Servers {
  return servers.filter((server) => !server.enabled && rule.server_ids.includes(server.id));
}

/** The rules a server is in, which go away with it. */
export function rulesWithServer<T extends Rule>(rules: T[], serverId: string): T[] {
  return rules.filter((rule) => rule.server_ids.includes(serverId));
}

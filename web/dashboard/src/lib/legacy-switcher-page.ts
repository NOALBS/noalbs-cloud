export function legacySwitcherPage(segment: "scenes" | "servers" | "triggers" | "obs", hash = "") {
  const anchor = hash.replace(/^#/, "");
  if (segment !== "obs")
    return {
      to: "/$workspace/s/$switcher/switching" as const,
      hash: segment === "triggers" ? "rules" : segment,
    };
  return {
    to: ["belabox", "moblin", "connect"].includes(anchor)
      ? ("/$workspace/s/$switcher/devices" as const)
      : ("/$workspace/s/$switcher/settings" as const),
    hash: anchor,
  };
}

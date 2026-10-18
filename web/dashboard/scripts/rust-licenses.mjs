// Writes public/rust-licenses.txt: the crates linked into the backend services and their
// licenses. Committed (the dashboard image is built without the Rust workspace), so run it
// when Cargo.lock changes: `node web/dashboard/scripts/rust-licenses.mjs` from the repo root.
// Warns about licenses that can't be combined with our AGPL-3.0 (GPL-2.0-only, SSPL, CDDL, EPL).
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const dashboard = join(dirname(fileURLToPath(import.meta.url)), "..");
const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--locked"], {
    cwd: join(dashboard, "..", ".."),
    encoding: "utf8",
    maxBuffer: 256 * 1024 * 1024,
  }),
);

const byId = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]));
const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
const workspace = new Set(metadata.workspace_members);

// What ends up in the services: normal dependencies only (not dev or build ones).
const seen = new Set(workspace);
const queue = [...workspace];
while (queue.length > 0) {
  const node = nodes.get(queue.pop());
  for (const dep of node?.deps ?? []) {
    if (dep.dep_kinds.some((kind) => kind.kind === null) && !seen.has(dep.pkg)) {
      seen.add(dep.pkg);
      queue.push(dep.pkg);
    }
  }
}

const crates = [...seen]
  .filter((id) => !workspace.has(id))
  .map((id) => byId.get(id))
  .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

const unclear = crates.filter((pkg) => !pkg.license);
// `GPL-2.0` without `-or-later`/`+` is the old SPDX spelling of GPL-2.0-only.
const incompatible = crates.filter((pkg) =>
  /\b(GPL-2\.0(-only|(?![-+]))|SSPL|CDDL|EPL)/.test(pkg.license ?? ""),
);
for (const pkg of unclear) console.warn(`no license field: ${pkg.name} ${pkg.version}`);
for (const pkg of incompatible) console.warn(`check: ${pkg.name} ${pkg.version} is ${pkg.license}`);

const lines = [
  "NOALBS Cloud: Rust crates used by the backend services, with their",
  "licenses as the crates declare them. Generated from Cargo.lock.",
  "",
  ...crates.map(
    (pkg) =>
      `${pkg.name} ${pkg.version} | ${pkg.license ?? "see repository"}${pkg.repository ? ` | ${pkg.repository}` : ""}`,
  ),
  "",
];
writeFileSync(join(dashboard, "public", "rust-licenses.txt"), lines.join("\n"));
console.log(`rust-licenses.txt: ${crates.length} crates`);

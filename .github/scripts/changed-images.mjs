// Decides which images the release workflow builds: those whose inputs changed
// since the commit production runs (the `sha-<commit>` tag in the manifests
// repo's overlays), so a dashboard change doesn't restart obs-link and drop
// every browser source. Comparing against what's deployed rather than the
// previous commit means a cancelled or failed run is caught up by the next one.
// Anything unclear (no tag, overlays disagree, a commit we don't have or that
// isn't an ancestor) rebuilds.
//
// Env: MANIFESTS_DIR (checkout of the manifests repo), REBUILD_ALL ("true"),
// GITHUB_OUTPUT (else prints). Needs the full git history (fetch-depth: 0).
//
// Outputs: `matrix` (JSON [{service, dockerfile, context}]) and `services`
// (space-separated names).

import { execFileSync } from "node:child_process";
import { appendFileSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";

const IMAGE = "ghcr.io/noalbs/noalbs-cloud";
const RUST_SERVICES = ["api", "obs-link", "chat", "db-migrator"];

const git = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();
const gitOk = (...args) => {
  try {
    execFileSync("git", args, { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
};

// What each image is built from: its own crate and the workspace crates it
// (transitively) depends on, outside dev-dependencies, plus the lockfile.
function rustInputs() {
  const root = git("rev-parse", "--show-toplevel");
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    }),
  );
  const crates = new Map(
    metadata.packages.map((p) => [
      p.name,
      {
        dir: relative(root, dirname(p.manifest_path)),
        deps: p.dependencies
          .filter((d) => d.path && d.kind !== "dev")
          .map((d) => d.name),
      },
    ]),
  );

  const inputs = {};
  for (const service of RUST_SERVICES) {
    const seen = new Set();
    const visit = (name) => {
      if (seen.has(name)) return;
      seen.add(name);
      for (const dep of crates.get(name).deps) visit(dep);
    };
    visit(service);
    inputs[service] = [
      "Cargo.toml",
      "Cargo.lock",
      ".dockerignore",
      ...[...seen].map((name) => crates.get(name).dir),
      // Left out of the image by .dockerignore.
      ":(exclude,glob)**/tests/**",
      ":(exclude,glob)**/*.md",
    ];
  }
  return inputs;
}

function images() {
  const rust = rustInputs();
  return [
    ...RUST_SERVICES.map((service) => ({
      service,
      dockerfile: `services/${service}/Dockerfile`,
      context: ".",
      paths: rust[service],
    })),
    {
      service: "dashboard",
      dockerfile: "web/dashboard/Dockerfile",
      context: "web/dashboard",
      paths: ["web/dashboard"],
    },
  ];
}

// The tag each overlay sets for an image, e.g. `sha-fbe82bb`.
function deployedTags(service) {
  const overlays = join(process.env.MANIFESTS_DIR, "apps/noalbs/overlays");
  const name = `${IMAGE}/${service}`;
  return readdirSync(overlays).map((overlay) => {
    const text = readFileSync(
      join(overlays, overlay, "kustomization.yaml"),
      "utf8",
    );
    const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const match = text.match(
      new RegExp(`-\\s*name:\\s*${escaped}\\s*\\n\\s*newTag:\\s*(\\S+)`),
    );
    return match?.[1];
  });
}

function reasonToBuild({ service, paths }) {
  if (process.env.REBUILD_ALL === "true") return "rebuild all requested";

  const tags = [...new Set(deployedTags(service))];
  if (tags.length !== 1 || !tags[0])
    return `overlays disagree or lack a tag (${tags.join(", ") || "none"})`;
  const sha = tags[0].match(/^sha-([0-9a-f]+)$/)?.[1];
  if (!sha) return `deployed tag ${tags[0]} isn't a commit`;
  if (!gitOk("rev-parse", "--verify", "--quiet", `${sha}^{commit}`))
    return `deployed commit ${sha} not found`;
  if (!gitOk("merge-base", "--is-ancestor", sha, "HEAD"))
    return `deployed commit ${sha} isn't an ancestor of HEAD`;

  const changed = git("diff", "--name-only", sha, "HEAD", "--", ...paths);
  if (!changed) return null;
  const files = changed.split("\n");
  const shown = files.slice(0, 5).join(", ");
  return `changed since ${sha}: ${shown}${files.length > 5 ? `, … (${files.length})` : ""}`;
}

const build = [];
for (const image of images()) {
  const reason = reasonToBuild(image);
  console.log(`${image.service}: ${reason ? `build (${reason})` : "unchanged"}`);
  if (reason) build.push(image);
}

const matrix = JSON.stringify(
  build.map(({ service, dockerfile, context }) => ({
    service,
    dockerfile,
    context,
  })),
);
const services = build.map((i) => i.service).join(" ");
const output = `matrix=${matrix}\nservices=${services}\n`;
if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, output);
else process.stdout.write(output);

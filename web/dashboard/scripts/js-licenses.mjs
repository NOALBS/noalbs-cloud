// Writes public/third-party-licenses.txt: the license text of every production dependency,
// as the MIT, ISC, BSD and Apache licenses ask when their code is shipped to browsers.
// Runs before every build (and by hand: `pnpm gen:licenses`); the file isn't committed.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const LICENSE_FILE = /^(licen[cs]e|copying|notice)(\.|-|$)/i;

const listed = JSON.parse(
  execFileSync("pnpm", ["licenses", "list", "--prod", "--json"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  }),
);

const packages = [];
for (const [license, entries] of Object.entries(listed)) {
  for (const entry of entries) {
    entry.paths.forEach((path, index) => {
      const files = readdirSync(path).filter((name) => LICENSE_FILE.test(name));
      packages.push({
        name: entry.name,
        version: entry.versions[index] ?? entry.versions[0],
        license,
        homepage: entry.homepage,
        text: files.map((name) => readFileSync(join(path, name), "utf8").trim()).join("\n\n"),
      });
    });
  }
}
packages.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

const rule = "=".repeat(78);
const out = ["NOALBS Cloud: third-party software in the dashboard and the OBS browser source.\n"];
for (const pkg of packages) {
  out.push(
    `${rule}\n${pkg.name} ${pkg.version} (${pkg.license})${pkg.homepage ? `\n${pkg.homepage}` : ""}\n${rule}\n`,
    pkg.text || `Licensed under ${pkg.license}; the package ships no license text.`,
    "",
  );
}

writeFileSync(join(root, "public", "third-party-licenses.txt"), `${out.join("\n")}\n`);
console.log(`third-party-licenses.txt: ${packages.length} packages`);

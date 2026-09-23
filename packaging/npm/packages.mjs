// Builds the npm packages from release archives:
//   node packaging/npm/packages.mjs VERSION ARCHIVE_DIR OUT_DIR
// One package per platform holds that platform's default executable; the
// `snomed-ecl-engine` package holds a launcher and lists them all as optional
// dependencies, so npm installs only the one that matches the machine.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const [version, archives, out] = process.argv.slice(2);
if (!version || !archives || !out) {
  throw new Error("Usage: packages.mjs VERSION ARCHIVE_DIR OUT_DIR");
}
const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..", "..");

const PLATFORMS = [
  { target: "x86_64-unknown-linux-gnu", os: "linux", cpu: "x64", libc: "glibc" },
  { target: "aarch64-unknown-linux-gnu", os: "linux", cpu: "arm64", libc: "glibc" },
  { target: "x86_64-apple-darwin", os: "darwin", cpu: "x64" },
  { target: "aarch64-apple-darwin", os: "darwin", cpu: "arm64" },
  { target: "x86_64-pc-windows-msvc", os: "win32", cpu: "x64" },
];

const common = {
  version,
  license: "MIT",
  homepage: "https://github.com/EddieDavison92/snomed-ecl-engine",
  repository: { type: "git", url: "git+https://github.com/EddieDavison92/snomed-ecl-engine.git" },
};

fs.rmSync(out, { recursive: true, force: true });
const optional = {};
for (const platform of PLATFORMS) {
  const name = `snomed-ecl-engine-${platform.os}-${platform.cpu}`;
  const windows = platform.os === "win32";
  const stem = `snomed-ecl-engine-v${version}-${platform.target}-default`;
  const archive = path.join(archives, stem + (windows ? ".zip" : ".tar.gz"));
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "snomed-ecl-npm-"));
  if (windows) {
    execFileSync("unzip", ["-q", archive, "-d", scratch]);
  } else {
    execFileSync("tar", ["-xzf", archive, "-C", scratch]);
  }
  const executable = "snomed-ecl-engine" + (windows ? ".exe" : "");
  const dir = path.join(out, name);
  fs.mkdirSync(path.join(dir, "bin"), { recursive: true });
  fs.copyFileSync(path.join(scratch, stem, executable), path.join(dir, "bin", executable));
  fs.chmodSync(path.join(dir, "bin", executable), 0o755);
  fs.copyFileSync(path.join(root, "LICENSE"), path.join(dir, "LICENSE"));
  fs.rmSync(scratch, { recursive: true, force: true });
  const manifest = {
    name,
    ...common,
    description: `The snomed-ecl-engine executable for ${platform.os} ${platform.cpu}.`,
    os: [platform.os],
    cpu: [platform.cpu],
    ...(platform.libc ? { libc: [platform.libc] } : {}),
    files: ["bin/"],
  };
  fs.writeFileSync(path.join(dir, "package.json"), JSON.stringify(manifest, null, 2) + "\n");
  optional[name] = version;
}

const wrapper = path.join(out, "snomed-ecl-engine");
fs.mkdirSync(path.join(wrapper, "bin"), { recursive: true });
fs.copyFileSync(path.join(here, "bin", "snomed-ecl-engine.js"), path.join(wrapper, "bin", "snomed-ecl-engine.js"));
fs.chmodSync(path.join(wrapper, "bin", "snomed-ecl-engine.js"), 0o755);
fs.copyFileSync(path.join(root, "LICENSE"), path.join(wrapper, "LICENSE"));
fs.copyFileSync(path.join(here, "README.md"), path.join(wrapper, "README.md"));
const manifest = {
  name: "snomed-ecl-engine",
  ...common,
  description: "Evaluate SNOMED CT ECL against a local index, without a terminology server.",
  keywords: ["snomed", "snomed-ct", "ecl", "terminology", "rf2"],
  bin: { "snomed-ecl-engine": "bin/snomed-ecl-engine.js" },
  files: ["bin/"],
  engines: { node: ">=18" },
  optionalDependencies: optional,
};
fs.writeFileSync(path.join(wrapper, "package.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`Wrote ${Object.keys(optional).length + 1} packages to ${out}`);

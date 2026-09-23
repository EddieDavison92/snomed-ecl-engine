#!/usr/bin/env node
// Runs the executable from the platform package npm installed alongside this one.
"use strict";

const { spawnSync } = require("node:child_process");

const PACKAGES = {
  "linux x64": "snomed-ecl-engine-linux-x64",
  "linux arm64": "snomed-ecl-engine-linux-arm64",
  "darwin x64": "snomed-ecl-engine-darwin-x64",
  "darwin arm64": "snomed-ecl-engine-darwin-arm64",
  "win32 x64": "snomed-ecl-engine-win32-x64",
};

const name = PACKAGES[`${process.platform} ${process.arch}`];
if (!name) {
  console.error(`snomed-ecl-engine has no build for ${process.platform} ${process.arch}.`);
  process.exit(1);
}

let executable;
try {
  const suffix = process.platform === "win32" ? ".exe" : "";
  executable = require.resolve(`${name}/bin/snomed-ecl-engine${suffix}`);
} catch {
  console.error(`The ${name} package is missing. Reinstall without --omit=optional.`);
  process.exit(1);
}

const result = spawnSync(executable, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);

#!/usr/bin/env node
// Cuts a FlipOn release and publishes it to our own update server (api.korol.app).
//
//   pnpm release patch|minor|major|1.2.3 [--notes "…"] [--no-build] [--no-upload] [--no-git]
//
// 1. bumps the version in package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json
// 2. takes the release notes from CHANGELOG.md ("## <version>") or --notes
// 3. builds the signed Windows installer (the updater refuses unsigned builds)
// 4. uploads installer + signature + notes, and checks the server got the same bytes
// 5. commits the version bump and tags it v<version>
//
// Needs, from the environment or an untracked .env.release file:
//   FLIPON_ADMIN_SECRET   the backend's ADMIN_SECRET (publishing is admin-only)
//   FLIPON_SIGNING_KEY    path to the updater private key (default ~/.tauri/flipon.key)
//   FLIPON_SIGNING_KEY_PASSWORD   its password, if it has one
//   FLIPON_RELEASE_API    default https://api.korol.app

import { execSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const APP = "flipon";
const PLATFORM = "windows-x86_64";
const FILE_NAME = "FlipOn-Setup.exe";

function fail(message) {
  console.error(`\n✗ ${message}`);
  process.exit(1);
}

function loadEnvFile() {
  const file = join(root, ".env.release");
  if (!existsSync(file)) return;
  for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
    const m = /^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$/.exec(line);
    if (m && !process.env[m[1]]) process.env[m[1]] = m[2].replace(/^["']|["']$/g, "");
  }
}

function parseArgs(argv) {
  const opts = { bump: null, notes: null, build: true, upload: true, git: true };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--notes") opts.notes = argv[++i] ?? "";
    else if (a === "--no-build") opts.build = false;
    else if (a === "--no-upload") opts.upload = false;
    else if (a === "--no-git") opts.git = false;
    else if (!a.startsWith("--") && !opts.bump) opts.bump = a;
    else fail(`Unknown argument: ${a}`);
  }
  if (!opts.bump) fail("Usage: pnpm release patch|minor|major|1.2.3 [--notes \"…\"] [--no-build] [--no-upload] [--no-git]");
  return opts;
}

function nextVersion(current, bump) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(current);
  if (!m) fail(`Current version "${current}" is not x.y.z`);
  let [major, minor, patch] = m.slice(1).map(Number);
  if (bump === "major") return `${major + 1}.0.0`;
  if (bump === "minor") return `${major}.${minor + 1}.0`;
  if (bump === "patch") return `${major}.${minor}.${patch + 1}`;
  if (/^\d+\.\d+\.\d+$/.test(bump)) return bump;
  fail(`"${bump}" is not patch, minor, major or x.y.z`);
}

function notesFromChangelog(version) {
  const file = join(root, "CHANGELOG.md");
  if (!existsSync(file)) return null;
  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  const start = lines.findIndex((l) => new RegExp(`^##\\s+v?${version.replace(/\./g, "\\.")}\\b`).test(l));
  if (start === -1) return null;
  const end = lines.findIndex((l, i) => i > start && /^##\s/.test(l));
  return lines.slice(start + 1, end === -1 ? undefined : end).join("\n").trim() || null;
}

function setVersions(version) {
  const pkgPath = join(root, "package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  pkg.version = version;
  writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");

  const confPath = join(root, "src-tauri", "tauri.conf.json");
  const conf = JSON.parse(readFileSync(confPath, "utf8"));
  conf.version = version;
  writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");

  const cargoPath = join(root, "src-tauri", "Cargo.toml");
  const cargo = readFileSync(cargoPath, "utf8");
  // only the [package] version — the first `version =` in the file
  const updated = cargo.replace(/^version\s*=\s*"[^"]*"/m, `version = "${version}"`);
  if (updated === cargo && !cargo.includes(`version = "${version}"`)) fail("Could not find the version in Cargo.toml");
  writeFileSync(cargoPath, updated);
}

function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex");
}

async function main() {
  loadEnvFile();
  const opts = parseArgs(process.argv.slice(2));
  const current = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  const version = nextVersion(current, opts.bump);
  const notes = opts.notes ?? notesFromChangelog(version);
  if (notes == null) fail(`Add a "## ${version}" section to CHANGELOG.md, or pass --notes "…"`);

  const api = (process.env.FLIPON_RELEASE_API || "https://api.korol.app").replace(/\/$/, "");
  const secret = process.env.FLIPON_ADMIN_SECRET;
  if (opts.upload && !secret) fail("FLIPON_ADMIN_SECRET is not set (environment or .env.release)");

  console.log(`FlipOn ${current} → ${version}\n`);
  if (version !== current) setVersions(version);

  const bundleDir = join(root, "src-tauri", "target", "release", "bundle", "nsis");
  const installer = join(bundleDir, `FlipOn_${version}_x64-setup.exe`);
  const signatureFile = `${installer}.sig`;

  if (opts.build) {
    const keyPath = process.env.FLIPON_SIGNING_KEY || join(homedir(), ".tauri", "flipon.key");
    if (!existsSync(keyPath)) fail(`Updater signing key not found at ${keyPath}`);
    console.log("Building the signed installer…");
    const result = spawnSync("pnpm", ["tauri", "build", "--bundles", "nsis"], {
      cwd: root,
      stdio: "inherit",
      shell: true,
      env: {
        ...process.env,
        TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyPath, "utf8").trim(),
        TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.FLIPON_SIGNING_KEY_PASSWORD ?? "",
      },
    });
    if (result.status !== 0) fail("Build failed");
  }
  if (!existsSync(installer)) fail(`Installer not found: ${installer}`);
  if (!existsSync(signatureFile)) fail(`Signature not found: ${signatureFile} (is the signing key set?)`);

  const data = readFileSync(installer);
  const signature = readFileSync(signatureFile, "utf8").trim();
  const hash = sha256(data);

  // keep a copy with a stable name, e.g. for winget or a manual mirror
  const outDir = join(root, "release-artifacts", version);
  mkdirSync(outDir, { recursive: true });
  copyFileSync(installer, join(outDir, FILE_NAME));
  writeFileSync(join(outDir, `${FILE_NAME}.sig`), signature + "\n");
  writeFileSync(join(outDir, "SHA256SUMS.txt"), `${hash}  ${FILE_NAME}\n`);
  console.log(`\n${FILE_NAME}  ${(data.length / 1048576).toFixed(2)} MB  sha256 ${hash}`);

  if (opts.upload) {
    console.log(`Uploading to ${api}…`);
    const res = await fetch(`${api}/apps/${APP}/releases/${version}/${PLATFORM}`, {
      method: "PUT",
      headers: {
        Authorization: `Bearer ${secret}`,
        "Content-Type": "application/octet-stream",
        "X-File-Name": FILE_NAME,
        "X-Signature": Buffer.from(signature, "utf8").toString("base64"),
        "X-Notes": Buffer.from(notes, "utf8").toString("base64"),
      },
      body: data,
    });
    const body = await res.text();
    if (!res.ok) fail(`Upload failed (HTTP ${res.status}): ${body}`);
    const published = JSON.parse(body);
    if (published.sha256 !== hash) fail(`Server stored different bytes (${published.sha256})`);
    console.log(`Published ${published.version}: ${published.url}`);
    // The landing page links the VirusTotal report of exactly this file (by its hash); it shows a
    // result once the file has been scanned there, so upload it once per release.
    console.log(`
VirusTotal: upload ${join(outDir, FILE_NAME)} at https://www.virustotal.com/gui/home/upload`);
    console.log(`  the site links https://www.virustotal.com/gui/file/${hash}`);
  }

  if (opts.git && version !== current) {
    const files = ["package.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock", "src-tauri/tauri.conf.json", "CHANGELOG.md"]
      .filter((f) => existsSync(join(root, f)));
    execSync(`git add ${files.join(" ")}`, { cwd: root, stdio: "inherit" });
    execSync(`git commit -q -m "Release v${version}"`, { cwd: root, stdio: "inherit" });
    execSync(`git tag v${version}`, { cwd: root, stdio: "inherit" });
    console.log(`Committed and tagged v${version} (push it when you have a remote).`);
  }

  console.log(`\n✓ FlipOn ${version} is out. Installed copies pick it up within a few hours.`);
}

main().catch((e) => fail(e?.stack || String(e)));

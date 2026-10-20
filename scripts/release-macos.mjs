#!/usr/bin/env node
// Builds the macOS version of the CURRENT release and publishes it to api.korol.app.
// Runs on a Mac (CI: see codemagic.yaml); the Windows release (`pnpm release`) has already bumped
// the version, written the changelog and tagged the commit.
//
//   node scripts/release-macos.mjs [--no-build] [--no-upload]
//
// Builds one universal app (Intel + Apple Silicon) and uploads, for darwin-aarch64 and darwin-x86_64:
//   * FlipOn.dmg          kind "installer"  what people download from the site
//   * FlipOn.app.tar.gz   kind "update"     what the auto-updater installs (signed with the updater key)
//
// Environment:
//   FLIPON_ADMIN_SECRET                the backend's ADMIN_SECRET
//   TAURI_SIGNING_PRIVATE_KEY          the updater private key itself (not a path)
//   TAURI_SIGNING_PRIVATE_KEY_PASSWORD its password, if it has one
//   FLIPON_RELEASE_API                 default https://api.korol.app

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const APP = "flipon";
const PLATFORMS = ["darwin-aarch64", "darwin-x86_64"];

function fail(message) {
  console.error(`\n✗ ${message}`);
  process.exit(1);
}

const args = process.argv.slice(2);
const doBuild = !args.includes("--no-build");
const doUpload = !args.includes("--no-upload");
for (const a of args) if (!["--no-build", "--no-upload"].includes(a)) fail(`Unknown argument: ${a}`);

const version = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8")).version;

function notesFromChangelog() {
  const file = join(root, "CHANGELOG.md");
  if (!existsSync(file)) return "";
  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  const start = lines.findIndex((l) => new RegExp(`^##\\s+v?${version.replace(/\./g, "\\.")}\\b`).test(l));
  if (start === -1) return "";
  const end = lines.findIndex((l, i) => i > start && /^##\s/.test(l));
  return lines.slice(start + 1, end === -1 ? undefined : end).join("\n").trim();
}

function find(dir, predicate) {
  if (!existsSync(dir)) return null;
  const name = readdirSync(dir).find(predicate);
  return name ? join(dir, name) : null;
}

if (process.platform !== "darwin" && doBuild) fail("macOS builds only work on a Mac. Use --no-build to upload existing files.");

if (doBuild) {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY) fail("TAURI_SIGNING_PRIVATE_KEY is not set (the updater key, as text)");
  console.log(`Building FlipOn ${version} for macOS (universal)…`);
  const r = spawnSync("pnpm", ["tauri", "build", "--target", "universal-apple-darwin", "--bundles", "app,dmg"], {
    cwd: root,
    stdio: "inherit",
    env: {
      ...process.env,
      // a pasted secret often carries a trailing newline
      TAURI_SIGNING_PRIVATE_KEY: process.env.TAURI_SIGNING_PRIVATE_KEY.trim(),
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "",
    },
  });
  if (r.status !== 0) fail("Build failed");
}

const bundle = join(root, "src-tauri", "target", "universal-apple-darwin", "release", "bundle");
const dmg = find(join(bundle, "dmg"), (n) => n.endsWith(".dmg"));
const archive = find(join(bundle, "macos"), (n) => n.endsWith(".app.tar.gz"));
if (!dmg) fail(`No .dmg found in ${join(bundle, "dmg")}`);
if (!archive) fail(`No .app.tar.gz found in ${join(bundle, "macos")}`);
if (!existsSync(`${archive}.sig`)) fail(`Missing ${archive}.sig (is the signing key set?)`);

const files = [
  { kind: "installer", fileName: "FlipOn.dmg", path: dmg, signature: "-" }, // the updater never reads this
  { kind: "update", fileName: "FlipOn.app.tar.gz", path: archive, signature: readFileSync(`${archive}.sig`, "utf8").trim() },
];
for (const f of files) {
  f.data = readFileSync(f.path);
  f.sha256 = createHash("sha256").update(f.data).digest("hex");
  console.log(`${f.fileName}  ${(f.data.length / 1048576).toFixed(2)} MB  sha256 ${f.sha256}`);
}

if (!doUpload) process.exit(0);

const api = (process.env.FLIPON_RELEASE_API || "https://api.korol.app").replace(/\/$/, "");
const secret = process.env.FLIPON_ADMIN_SECRET;
if (!secret) fail("FLIPON_ADMIN_SECRET is not set");
const notes = notesFromChangelog();

for (const platform of PLATFORMS) {
  for (const f of files) {
    const res = await fetch(`${api}/apps/${APP}/releases/${version}/${platform}`, {
      method: "PUT",
      headers: {
        Authorization: `Bearer ${secret}`,
        "Content-Type": "application/octet-stream",
        "X-File-Name": f.fileName,
        "X-Kind": f.kind,
        "X-Signature": Buffer.from(f.signature, "utf8").toString("base64"),
        "X-Notes": Buffer.from(notes, "utf8").toString("base64"),
      },
      body: f.data,
    });
    const body = await res.text();
    if (!res.ok) fail(`Upload of ${f.fileName} for ${platform} failed (HTTP ${res.status}): ${body}`);
    if (JSON.parse(body).sha256 !== f.sha256) fail(`Server stored different bytes for ${f.fileName} (${platform})`);
    console.log(`Published ${f.fileName} for ${platform}`);
  }
}
console.log(`\n✓ FlipOn ${version} for macOS is out.`);
console.log(`  VirusTotal: https://www.virustotal.com/gui/file/${files[0].sha256}`);

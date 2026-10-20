// Dev-only stand-in for the Rust backend so the UI can be viewed in a normal browser
// (`pnpm dev`, then open http://localhost:1420/?w=popover or ?w=settings).
import type { Account, Settings, Snapshot } from "./api";

const H = 3_600_000;
const D = 24 * H;
const now = Date.now();

const win = (key: string, label: string, utilization: number, resetIn: number | null, windowMs: number, trend: Account["windows"][number]["trend"] = null) => ({
  key,
  label,
  utilization,
  resetsAtMs: resetIn == null ? null : now + resetIn,
  windowMs,
  trend,
});

const accounts: Account[] = [
  {
    id: "a", email: "work@example.com", alias: "Work", plan: "Team", status: "week_full", statusResetsAtMs: now + D + 6 * H,
    windows: [win("five_hour", "Session", 20, 2 * H, 5 * H), win("seven_day", "Weekly", 97, D + 6 * H, 7 * D, { dir: "flat", delta: 0, minutes: 20 })], today: null,
  },
  {
    id: "b", email: "side.project@example.com", alias: null, plan: "Pro", status: "ready", statusResetsAtMs: null,
    windows: [
      win("five_hour", "Session", 8, 2 * H + 5 * 60_000, 5 * H, { dir: "up", delta: 5, minutes: 20 }),
      win("seven_day", "Weekly", 22, 5 * D + 14 * H, 7 * D, { dir: "flat", delta: 0, minutes: 20 }),
    ],
    today: null,
  },
  {
    id: "c", email: "me@example.com", alias: "Personal", plan: "Pro", status: "active", statusResetsAtMs: null,
    windows: [
      win("five_hour", "Session", 41, 3 * H + 40 * 60_000, 5 * H, { dir: "up", delta: 6, minutes: 20 }),
      win("seven_day", "Weekly", 63, 4 * D + 2 * H, 7 * D, { dir: "flat", delta: 0, minutes: 20 }),
    ],
    today: { cost: 2.4, turns: 12, lines: 540, models: [["Opus", 2.1], ["Sonnet", 0.3]] },
  },
];

const settings: Settings = {
  autoSwitch: true, notifyOnSwitch: true, switchThreshold: 90, warnEnabled: true, warnThreshold: 80,
  strategy: "balanced", roundRobinStep: 40, trayMode: "presets", trayPreset: "bar",
  trayModules: [
    { id: "name", enabled: true }, { id: "5h", enabled: true }, { id: "7d", enabled: true },
    { id: "cost", enabled: false }, { id: "reset", enabled: false },
  ],
  trayBasedOn: "highest", startOnLogin: false, cliPath: "", autoUpdate: true, autoUpdateChosen: false,
};

let snap: Snapshot = {
  accounts, activeId: "c", settings,
  cli: { found: false, version: null, path: null },
  login: { running: false, adding: false, needsCode: false, url: null, targetId: null },
  hold: { accountId: "c", untilMs: now + H + 20 * 60_000 },
  unknownLogin: null, refreshing: false, error: null,
  app: { version: "0.1.0", platform: "windows", dev: false },
  update: { status: "ready", version: "0.1.1", notes: "Faster switching", progress: null, error: null, checkedAtMs: now },
  nowMs: now,
};

const listeners = new Set<(s: Snapshot) => void>();
const push = () => listeners.forEach((l) => l({ ...snap }));

export function subscribeMock(fn: (s: Snapshot) => void) {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

export async function mockInvoke(name: string, args?: Record<string, unknown>): Promise<unknown> {
  console.info("[mock]", name, args);
  switch (name) {
    case "get_snapshot": return { ...snap };
    case "update_settings": snap = { ...snap, settings: args!.settings as Settings }; push(); return;
    case "reorder_account": {
      const list = [...snap.accounts];
      const i = list.findIndex((a) => a.id === args!.id);
      const j = i + (args!.delta as number);
      if (i >= 0 && j >= 0 && j < list.length) [list[i], list[j]] = [list[j], list[i]];
      snap = { ...snap, accounts: list }; push(); return;
    }
    case "add_account": snap = { ...snap, login: { running: true, adding: false, needsCode: true, url: "https://example.com", targetId: null } }; push(); return;
    case "cancel_login": snap = { ...snap, login: { running: false, adding: false, needsCode: false, url: null, targetId: null } }; push(); return;
    case "submit_login_code": snap = { ...snap, login: { ...snap.login, needsCode: false } }; push(); return;
    case "detect_cli": return snap.cli;
    default: return;
  }
}

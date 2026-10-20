import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/** Outside Tauri (plain browser during UI work) a small in-memory mock stands in for the backend. */
async function invoke<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return tauriInvoke<T>(name, args);
  const { mockInvoke } = await import("./mock");
  return mockInvoke(name, args) as T;
}

export type Strategy = "headroom" | "order" | "balanced" | "roundrobin";
export type TrayPreset = "bar" | "percent" | "ring" | "dot" | "face" | "history";
export type BasedOn = "highest" | "5h" | "7d";

export interface TrayModule {
  id: "name" | "5h" | "7d" | "cost" | "reset";
  enabled: boolean;
}

export interface Settings {
  autoSwitch: boolean;
  notifyOnSwitch: boolean;
  switchThreshold: number;
  warnEnabled: boolean;
  warnThreshold: number;
  strategy: Strategy;
  roundRobinStep: number;
  trayMode: "presets" | "custom";
  trayPreset: TrayPreset;
  trayModules: TrayModule[];
  trayBasedOn: BasedOn;
  startOnLogin: boolean;
  cliPath: string;
  autoUpdate: boolean;
  autoUpdateChosen: boolean;
}

export interface Trend {
  dir: "up" | "flat";
  delta: number;
  minutes: number;
}

export type WindowKey = "five_hour" | "seven_day" | "seven_day_opus" | "seven_day_sonnet";

export interface UsageWindow {
  key: WindowKey | string;
  label: string;
  utilization: number;
  resetsAtMs: number | null;
  windowMs: number;
  trend: Trend | null;
}

export interface Today {
  cost: number;
  turns: number;
  lines: number;
  models: [string, number][];
}

export type AccountStatus = "active" | "ready" | "needs_login" | "no_backup" | "week_full" | "five_hour_full";

export interface Account {
  id: string;
  email: string;
  alias: string | null;
  plan: string;
  status: AccountStatus;
  statusResetsAtMs: number | null;
  windows: UsageWindow[];
  today: Today | null;
}

export interface Snapshot {
  accounts: Account[];
  activeId: string | null;
  settings: Settings;
  cli: { found: boolean; version: string | null; path: string | null };
  login: { running: boolean; adding: boolean; needsCode: boolean; url: string | null; targetId: string | null };
  hold: { accountId: string; untilMs: number | null } | null;
  unknownLogin: string | null;
  refreshing: boolean;
  error: { id: number; message: string } | null;
  app: { version: string; platform: string; dev: boolean };
  update: {
    status: "idle" | "checking" | "up_to_date" | "downloading" | "ready" | "installing" | "error";
    version: string | null;
    notes: string | null;
    progress: number | null;
    error: string | null;
    checkedAtMs: number | null;
  };
  nowMs: number;
}

export const cmd = {
  addAccount: () => invoke<void>("add_account"),
  reloginAccount: (id: string) => invoke<void>("relogin_account", { id }),
  cancelLogin: () => invoke<void>("cancel_login"),
  submitLoginCode: (code: string) => invoke<void>("submit_login_code", { code }),
  /** `hours: null` = "No timer" (pinned until the next manual switch). */
  switchAccount: (id: string, hours: number | null) => invoke<void>("switch_account", { id, hours }),
  renameAccount: (id: string, alias: string) => invoke<void>("rename_account", { id, alias }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  reorderAccount: (id: string, delta: number) => invoke<void>("reorder_account", { id, delta }),
  refreshUsage: () => invoke<void>("refresh_usage"),
  updateSettings: (settings: Settings) => invoke<void>("update_settings", { settings }),
  detectCli: () => invoke<Snapshot["cli"]>("detect_cli"),
  saveUnknownLogin: () => invoke<void>("save_unknown_login"),
  releaseHold: () => invoke<void>("release_hold"),
  dismissError: () => invoke<void>("dismiss_error"),
  openSettings: () => invoke<void>("open_settings"),
  hidePopover: () => invoke<void>("hide_popover"),
  resizePopover: (height: number) => invoke<void>("resize_popover", { height }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  checkUpdates: () => invoke<void>("check_updates"),
  installUpdate: () => invoke<void>("install_update"),
};

/** Live view of the backend state: fetched once, then pushed on every change. */
export function useSnapshot(): Snapshot | null {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    const pull = () => invoke<Snapshot>("get_snapshot").then((s) => !cancelled && setSnap(s));
    pull();
    if (inTauri) {
      listen<Snapshot>("flipon://state", (e) => setSnap(e.payload)).then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    } else {
      import("./mock").then(({ subscribeMock }) => {
        if (!cancelled) unlisten = subscribeMock(setSnap);
      });
    }
    window.addEventListener("focus", pull);
    return () => {
      cancelled = true;
      unlisten?.();
      window.removeEventListener("focus", pull);
    };
  }, []);
  return snap;
}

/** Re-render periodically so countdowns stay current between backend pushes. */
export function useNow(intervalMs = 20_000): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}

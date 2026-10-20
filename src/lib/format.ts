import type { Account, Settings, Strategy, UsageWindow } from "./api";

/** "1d 6h", "2h 05m", "12m" */
export function countdown(ms: number): string {
  if (ms <= 0) return "now";
  const totalMin = Math.ceil(ms / 60_000);
  const d = Math.floor(totalMin / 1440);
  const h = Math.floor((totalMin % 1440) / 60);
  const m = totalMin % 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  return `${m}m`;
}

export type Tone = "ok" | "warn" | "critical";

/** Bar/figure color: critical at the switch threshold, amber from 60%. */
export function toneFor(pct: number, switchThreshold: number): Tone {
  if (pct >= switchThreshold) return "critical";
  if (pct >= 60) return "warn";
  return "ok";
}

export function elapsedPercent(w: UsageWindow, now: number): number | null {
  if (w.resetsAtMs == null) return null;
  const remaining = w.resetsAtMs - now;
  return Math.min(100, Math.max(0, (1 - remaining / w.windowMs) * 100));
}

export function accountTitle(a: Pick<Account, "alias" | "email">): string {
  return a.alias ?? a.email;
}

export const STRATEGY_NAMES: Record<Strategy, string> = {
  headroom: "most headroom",
  order: "account order",
  balanced: "balanced usage",
  roundrobin: "round-robin",
};

/** Same rule as the design: a maxed-out week makes the 5-hour row moot. */
export function visibleWindows(a: Account, settings: Settings): UsageWindow[] {
  const week = a.windows.find((w) => w.key === "seven_day");
  const weekFull = week != null && week.utilization >= settings.switchThreshold;
  return a.windows.filter((w) => !(weekFull && w.key === "five_hour"));
}

export function trendTitle(t: NonNullable<UsageWindow["trend"]>): string {
  if (t.dir === "flat") return "flat — barely moving";
  const rounded = Math.max(5, Math.round(t.minutes / 5) * 5);
  return `↗ +${Math.round(t.delta)}% in the last ${rounded}m`;
}

export function money(n: number): string {
  return `$${n.toFixed(2)}`;
}

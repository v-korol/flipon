import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowsRotate,
  faCalendarWeek,
  faChartSimple,
  faCheck,
  faChevronDown,
  faChevronUp,
  faCircleCheck,
  faCircleInfo,
  faClock,
  faCoins,
  faCommentDots,
  faDesktop,
  faGauge,
  faGripVertical,
  faIcons,
  faListOl,
  faMagnifyingGlass,
  faRightLeft,
  faRotate,
  faScaleUnbalancedFlip,
  faUser,
  faWandMagicSparkles,
  type IconDefinition,
} from "@fortawesome/free-solid-svg-icons";
import {
  cmd,
  useSnapshot,
  type Account,
  type BasedOn,
  type Settings,
  type Snapshot,
  type Strategy,
  type TrayModule,
  type TrayPreset,
} from "@/lib/api";
import { accountTitle } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";

type Tab = "switching" | "tray" | "system" | "about";

const TABS: { id: Tab; label: string; icon: IconDefinition; title: string; desc: string }[] = [
  { id: "switching", label: "Switching", icon: faRightLeft, title: "Switching accounts", desc: "How FlipOn decides when to move you to a different account." },
  { id: "tray", label: "Tray icon", icon: faIcons, title: "Tray icon", desc: "What the tray icon looks like and which usage it tracks." },
  { id: "system", label: "System", icon: faDesktop, title: "System", desc: "" },
  { id: "about", label: "About", icon: faCircleInfo, title: "About", desc: "" },
];

type SaveState = "idle" | "saving" | "saved" | { error: string };

// ---------------------------------------------------------------------------------- shared bits

function Card({ children }: { children: ReactNode }) {
  return <div className="rounded-xl border border-border bg-card px-4">{children}</div>;
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 border-t border-border py-3.5 first:border-t-0">
      <div>
        <div className="text-[13px] font-medium">{label}</div>
        {hint && <div className="mt-0.5 max-w-[320px] text-xs text-muted-foreground">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

function SliderRow({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="border-t border-border py-3.5 first:border-t-0">
      <div className="flex items-center justify-between gap-4">
        <span className="text-[13px] font-medium">{label}</span>
        <span className="font-mono text-[13px]">{value}%</span>
      </div>
      <Slider className="mt-2.5" min={min} max={max} step={1} value={[value]} onValueChange={([v]) => onChange(v)} />
    </div>
  );
}

function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <ToggleGroup type="single" value={value} onValueChange={(v) => v && onChange(v as T)}>
      {options.map((o) => (
        <ToggleGroupItem key={o.value} value={o.value}>
          {o.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

// ---------------------------------------------------------------------------------- switching tab

const STRATEGIES: { id: Strategy; icon: IconDefinition; title: string; desc: string; best: string }[] = [
  {
    id: "headroom",
    icon: faChartSimple,
    title: "Most headroom",
    desc: "Stays on the current account until it reaches the switch limit, then moves to the one with the most usage left.",
    best: "One account at a time, always the freshest next.",
  },
  {
    id: "order",
    icon: faListOl,
    title: "My account order",
    desc: "Always uses the top account in your list that still has room. Moves down when it fills up and jumps back as soon as it resets.",
    best: "A main account with backups.",
  },
  {
    id: "balanced",
    icon: faScaleUnbalancedFlip,
    title: "Balanced usage",
    desc: "Uses first the account whose weekly limit resets soonest, so leftover quota isn't lost at the reset. If tied, the account with a session already running wins.",
    best: "Getting the most out of several plans.",
  },
  {
    id: "roundrobin",
    icon: faRotate,
    title: "Round-robin by threshold",
    desc: "Uses each account up to a set percent, then moves to the next in your list and loops around, with a higher cap on every lap.",
    best: "Spreading usage evenly across accounts.",
  },
];

function OrderList({ accounts }: { accounts: Account[] }) {
  return (
    <div className="overflow-hidden rounded-[10px] border border-border">
      {accounts.map((a, i) => (
        <div key={a.id} className="flex items-center gap-2.5 border-t border-border px-3 py-2 text-[13px] first:border-t-0">
          <span className="w-3.5 text-right font-mono text-xs text-muted-foreground">{i + 1}</span>
          <span className="min-w-0 flex-1 truncate">
            {a.alias ? (
              <>
                {a.alias} <span className="font-normal text-muted-foreground">· {a.email}</span>
              </>
            ) : (
              a.email
            )}
          </span>
          <Badge>{a.plan}</Badge>
          <Button variant="ghost" size="icon-sm" disabled={i === 0} onClick={() => cmd.reorderAccount(a.id, -1)} aria-label="Move up">
            <FontAwesomeIcon icon={faChevronUp} />
          </Button>
          <Button variant="ghost" size="icon-sm" disabled={i === accounts.length - 1} onClick={() => cmd.reorderAccount(a.id, 1)} aria-label="Move down">
            <FontAwesomeIcon icon={faChevronDown} />
          </Button>
        </div>
      ))}
    </div>
  );
}

function SwitchingTab({ s, set, accounts }: { s: Settings; set: (p: Partial<Settings>) => void; accounts: Account[] }) {
  return (
    <div className="flex flex-col gap-4">
      <Card>
        <Row label="Switch accounts automatically" hint="Moves to the next account before this one hits its usage limit.">
          <Switch checked={s.autoSwitch} onCheckedChange={(v) => set({ autoSwitch: v })} />
        </Row>
        <Row label="Notify me when it switches">
          <Switch checked={s.notifyOnSwitch} onCheckedChange={(v) => set({ notifyOnSwitch: v })} />
        </Row>
        <SliderRow label="Switch once usage reaches" value={s.switchThreshold} min={50} max={100} onChange={(v) => set({ switchThreshold: v })} />
      </Card>

      <Card>
        <Row label="Warn me before I hit the limit" hint="Sends a notification at this usage level — works even with auto-switch off.">
          <Switch checked={s.warnEnabled} onCheckedChange={(v) => set({ warnEnabled: v })} />
        </Row>
        <SliderRow label="Notify at" value={s.warnThreshold} min={50} max={100} onChange={(v) => set({ warnThreshold: v })} />
      </Card>

      <div>
        <div className="mb-2 mt-1 text-[13px] font-medium">Strategy</div>
        <div className="flex flex-col gap-2">
          {STRATEGIES.map((st) => {
            const selected = s.strategy === st.id;
            return (
              <button
                key={st.id}
                onClick={() => set({ strategy: st.id })}
                className={cn(
                  "flex w-full items-start gap-3 rounded-[10px] border px-3.5 py-3 text-left",
                  selected ? "border-accent bg-[color-mix(in_oklab,var(--accent)_7%,var(--background))]" : "border-border bg-background"
                )}
              >
                <div
                  className={cn(
                    "flex size-[30px] shrink-0 items-center justify-center rounded-lg text-[13px]",
                    selected ? "bg-accent text-accent-foreground" : "bg-muted text-muted-foreground"
                  )}
                >
                  <FontAwesomeIcon icon={st.icon} />
                </div>
                <div className="min-w-0 flex-1">
                  <div className="text-[13px] font-semibold">{st.title}</div>
                  <div className="mt-0.5 text-xs leading-[1.45] text-muted-foreground">{st.desc}</div>
                  <div className="mt-1 text-[11px] leading-[1.4] text-muted-foreground/80">
                    <span className="font-medium text-muted-foreground">Best for:</span> {st.best}
                  </div>
                </div>
                <FontAwesomeIcon icon={faCircleCheck} className={cn("shrink-0 text-sm text-accent", selected ? "opacity-100" : "opacity-0")} />
              </button>
            );
          })}
        </div>
      </div>

      {s.strategy === "roundrobin" && (
        <Card>
          <SliderRow label="Use each account up to, per turn" value={s.roundRobinStep} min={10} max={90} onChange={(v) => set({ roundRobinStep: v })} />
        </Card>
      )}

      {s.strategy === "order" && accounts.length > 0 && (
        <div>
          <div className="mb-2 text-[13px] font-medium">Account order</div>
          <OrderList accounts={accounts} />
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------------- tray tab

const PRESETS: { id: TrayPreset; label: string; sparkle?: boolean }[] = [
  { id: "bar", label: "Progress bar" },
  { id: "percent", label: "Percentage" },
  { id: "ring", label: "Ring" },
  { id: "dot", label: "Traffic-light dot" },
  { id: "face", label: "Mood face", sparkle: true },
  { id: "history", label: "Mini history", sparkle: true },
];

const MODULES: Record<TrayModule["id"], { label: string; icon: IconDefinition }> = {
  name: { label: "Account name / alias", icon: faUser },
  "5h": { label: "5-hour bar", icon: faGauge },
  "7d": { label: "Weekly bar", icon: faCalendarWeek },
  cost: { label: "Today's cost", icon: faCoins },
  reset: { label: "Reset countdown", icon: faClock },
};

function toneColor(pct: number, critical: number) {
  return pct >= critical ? "#e5645e" : pct >= 60 ? "#e8a050" : "#fff";
}

function MiniBar({ pct, w = 28, h = 10, color = "#fff" }: { pct: number; w?: number; h?: number; color?: string }) {
  return (
    <div className="relative overflow-hidden rounded-full border border-white/50" style={{ width: w, height: h }}>
      <div className="absolute rounded-full" style={{ inset: "2px auto 2px 2px", width: `calc(${Math.min(100, pct)}% - 4px)`, minWidth: h - 4, background: color }} />
    </div>
  );
}

function PresetPreview({ id, pct, critical }: { id: TrayPreset; pct: number; critical: number }) {
  const c = toneColor(pct, critical);
  switch (id) {
    case "bar":
      return <MiniBar pct={pct} color={c} />;
    case "percent":
      return <span className="font-mono text-[11px] font-semibold" style={{ color: c }}>{Math.round(pct)}%</span>;
    case "ring":
      return (
        <div
          className="size-5 rounded-full"
          style={{ background: `radial-gradient(closest-side,#2b2b2f 60%,transparent 61% 100%),conic-gradient(${c} 0% ${pct}%, rgba(255,255,255,.25) 0)` }}
        />
      );
    case "dot":
      return <div className="size-3 rounded-full" style={{ background: pct >= critical ? "#e5645e" : pct >= 60 ? "#d7a868" : "#7dc79b" }} />;
    case "face":
      return <span className="text-lg leading-none">{pct >= critical ? "😣" : pct >= 80 ? "😟" : pct >= 60 ? "😐" : "🙂"}</span>;
    case "history":
      return (
        <div className="flex h-[18px] items-end gap-0.5">
          {[40, 65, 50, Math.max(10, pct)].map((h, i, a) => (
            <div key={i} className="w-[3px] rounded-[1px]" style={{ height: `${h}%`, background: i === a.length - 1 ? "#fff" : "rgba(255,255,255,.4)" }} />
          ))}
        </div>
      );
  }
}

function ModuleStrip({ modules, preview }: { modules: TrayModule[]; preview: { name: string; five: number; seven: number; cost: string; reset: string; critical: number } }) {
  const on = modules.filter((m) => m.enabled);
  return (
    <div className="mb-2.5 flex h-[34px] items-center justify-center gap-2.5 rounded-lg bg-[#2b2b2f] px-3.5 text-white">
      {on.length === 0 && <span className="text-xs opacity-60">Nothing selected</span>}
      {on.map((m) => {
        switch (m.id) {
          case "name":
            return <span key={m.id} className="text-xs font-semibold">{preview.name}</span>;
          case "5h":
            return <MiniBar key={m.id} pct={preview.five} w={24} h={9} color={toneColor(preview.five, preview.critical)} />;
          case "7d":
            return <MiniBar key={m.id} pct={preview.seven} w={24} h={9} color={toneColor(preview.seven, preview.critical)} />;
          case "cost":
            return <span key={m.id} className="font-mono text-[11px]">{preview.cost}</span>;
          case "reset":
            return <span key={m.id} className="font-mono text-[11px]">{preview.reset}</span>;
        }
      })}
    </div>
  );
}

function TrayTab({ s, set, active }: { s: Settings; set: (p: Partial<Settings>) => void; active: Account | undefined }) {
  const five = active?.windows.find((w) => w.key === "five_hour")?.utilization ?? 41;
  const seven = active?.windows.find((w) => w.key === "seven_day")?.utilization ?? 63;
  const driving = s.trayBasedOn === "5h" ? five : s.trayBasedOn === "7d" ? seven : Math.max(five, seven);
  const moveModule = (i: number, d: number) => {
    const next = [...s.trayModules];
    const j = i + d;
    if (j < 0 || j >= next.length) return;
    [next[i], next[j]] = [next[j], next[i]];
    set({ trayModules: next });
  };
  const toggleModule = (i: number, enabled: boolean) =>
    set({ trayModules: s.trayModules.map((m, k) => (k === i ? { ...m, enabled } : m)) });

  return (
    <div className="flex flex-col gap-4">
      <div>
        <Segmented
          value={s.trayMode}
          options={[
            { value: "presets", label: "Presets" },
            { value: "custom", label: "Custom" },
          ]}
          onChange={(v) => set({ trayMode: v })}
        />
      </div>

      {s.trayMode === "presets" ? (
        <div className="grid grid-cols-3 gap-2.5">
          {PRESETS.map((p) => {
            const selected = s.trayPreset === p.id;
            return (
              <button
                key={p.id}
                onClick={() => set({ trayPreset: p.id })}
                className={cn(
                  "flex flex-col items-center gap-2 rounded-[10px] border p-2.5",
                  selected ? "border-accent bg-[color-mix(in_oklab,var(--accent)_7%,var(--background))]" : "border-border bg-background"
                )}
              >
                <div className="flex h-[34px] w-full items-center justify-center gap-1.5 rounded-md bg-[#2b2b2f]">
                  <PresetPreview id={p.id} pct={driving} critical={s.switchThreshold} />
                </div>
                <span className={cn("text-[11px] font-medium", selected && "text-accent")}>
                  {p.label} {p.sparkle && <FontAwesomeIcon icon={faWandMagicSparkles} className="text-[9px] text-accent" />}
                </span>
              </button>
            );
          })}
        </div>
      ) : (
        <div>
          <p className="mb-2.5 text-xs leading-normal text-muted-foreground">
            Build your own: toggle the blocks you want and use the arrows to reorder them. The preview shows how it'll look in the tray —
            the tray icon is square, so the blocks are stacked to fit.
          </p>
          <ModuleStrip
            modules={s.trayModules}
            preview={{
              name: active ? accountTitle(active) : "Personal",
              five,
              seven,
              cost: active?.today ? `$${active.today.cost.toFixed(2)}` : "$2.40",
              reset: "3h 40m",
              critical: s.switchThreshold,
            }}
          />
          <div className="overflow-hidden rounded-[10px] border border-border">
            {s.trayModules.map((m, i) => (
              <div key={m.id} className={cn("flex items-center gap-2.5 border-t border-border px-3 py-[9px] text-[13px] first:border-t-0", !m.enabled && "opacity-50")}>
                <FontAwesomeIcon icon={faGripVertical} className="shrink-0 text-muted-foreground" />
                <FontAwesomeIcon icon={MODULES[m.id].icon} className="w-3.5 text-muted-foreground" />
                <span className="min-w-0 flex-1">{MODULES[m.id].label}</span>
                <div className="flex gap-0.5">
                  <Button variant="ghost" size="icon-sm" disabled={i === 0} onClick={() => moveModule(i, -1)} aria-label="Move up">
                    <FontAwesomeIcon icon={faChevronUp} />
                  </Button>
                  <Button variant="ghost" size="icon-sm" disabled={i === s.trayModules.length - 1} onClick={() => moveModule(i, 1)} aria-label="Move down">
                    <FontAwesomeIcon icon={faChevronDown} />
                  </Button>
                </div>
                <Switch size="sm" checked={m.enabled} onCheckedChange={(v) => toggleModule(i, v)} />
              </div>
            ))}
          </div>
        </div>
      )}

      <Card>
        <Row label="Based on" hint="Which usage window drives the icon.">
          <Segmented<BasedOn>
            value={s.trayBasedOn}
            options={[
              { value: "highest", label: "Highest" },
              { value: "5h", label: "5h" },
              { value: "7d", label: "7d" },
            ]}
            onChange={(v) => set({ trayBasedOn: v })}
          />
        </Row>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------------- system + about

function SystemTab({ s, set, cli, flush }: { s: Settings; set: (p: Partial<Settings>) => void; cli: Snapshot["cli"]; flush: () => Promise<void> }) {
  const [detecting, setDetecting] = useState(false);
  const detect = async () => {
    setDetecting(true);
    try {
      await flush();
      await cmd.detectCli();
    } finally {
      setDetecting(false);
    }
  };
  return (
    <Card>
      <Row label="Launch FlipOn on computer startup" hint="FlipOn starts in the tray when you sign in to your computer. Not related to your Claude accounts.">
        <Switch checked={s.startOnLogin} onCheckedChange={(v) => set({ startOnLogin: v })} />
      </Row>
      <div className="flex flex-col gap-2 border-t border-border py-3.5">
        <div className="flex items-center justify-between">
          <span className="text-[13px] font-medium">Claude Code CLI location</span>
          {cli.found ? (
            <span className="font-mono text-xs text-success">{cli.version ?? "found"}</span>
          ) : (
            <span className="font-mono text-xs text-warning">not found</span>
          )}
        </div>
        <div className="flex gap-2">
          <Input placeholder="Auto-detect (leave empty)" value={s.cliPath} onChange={(e) => set({ cliPath: e.target.value })} />
          <Button variant="outline" disabled={detecting} onClick={detect}>
            <FontAwesomeIcon icon={faMagnifyingGlass} /> Detect
          </Button>
        </div>
        <p className="text-xs leading-normal text-muted-foreground">
          Only needed if FlipOn can't find <span className="font-mono">claude</span> on its own.
          {cli.path && (
            <>
              {" "}
              Using <span className="break-all font-mono">{cli.path}</span>.
            </>
          )}
        </p>
      </div>
    </Card>
  );
}

function AppMark({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden>
      <rect width="64" height="64" rx="15" fill="#3F3F46" />
      <rect x="8" y="13" width="36" height="17" rx="8.5" fill="#FAFAFA" />
      <circle cx="18" cy="21.5" r="4.5" fill="#8FA3CF" />
      <rect x="26" y="19" width="12" height="5" rx="2.5" fill="#3F3F46" fillOpacity=".35" />
      <rect x="20" y="34" width="36" height="17" rx="8.5" fill="#8FA3CF" />
      <circle cx="30" cy="42.5" r="4.5" fill="#FAFAFA" />
      <rect x="38" y="40" width="12" height="5" rx="2.5" fill="#3F3F46" fillOpacity=".4" />
    </svg>
  );
}

function updateHint(u: Snapshot["update"], current: string): string {
  const v = u.version ?? "";
  switch (u.status) {
    case "checking":
      return "Checking for updates…";
    case "downloading":
      return u.progress != null ? `Downloading version ${v}… ${Math.round(u.progress * 100)}%` : `Downloading version ${v}…`;
    case "ready":
      return `Version ${v} is downloaded and verified.${u.notes ? ` ${u.notes}` : ""}`;
    case "installing":
      return "Installing — FlipOn will restart.";
    case "up_to_date":
      return `You have the latest version (${current}).`;
    case "error":
      return u.error ?? "The last update check failed.";
    default:
      return "Updates come from korol.app and are checked every few hours.";
  }
}

function UpdatesRow({ snap }: { snap: Snapshot }) {
  const u = snap.update;
  const busy = u.status === "checking" || u.status === "downloading" || u.status === "installing";
  return (
    <Row label="Updates" hint={updateHint(u, snap.app.version)}>
      {u.status === "ready" ? (
        <Button onClick={() => cmd.installUpdate()}>Restart to update</Button>
      ) : (
        <Button variant="outline" disabled={busy} onClick={() => cmd.checkUpdates()}>
          <FontAwesomeIcon icon={faArrowsRotate} className={busy ? "spin" : undefined} /> Check now
        </Button>
      )}
    </Row>
  );
}

/** Marks a development build, so it is never confused with the installed app. */
export function DevBadge() {
  return (
    <span className="ml-1.5 rounded bg-destructive px-1.5 py-px font-mono text-[10px] font-semibold text-white align-middle">DEV</span>
  );
}

function AboutTab({ s, set, snap }: { s: Settings; set: (p: Partial<Settings>) => void; snap: Snapshot }) {
  return (
    <>
      <Card>
        <div className="flex items-center justify-between gap-4 py-3.5">
          <div className="flex items-center gap-2.5">
            <AppMark size={40} />
            <div>
              <div className="text-sm font-semibold">FlipOn</div>
              <div className="text-xs text-muted-foreground">
                Version {snap.app.version} · {snap.app.platform}
                {snap.app.dev && <DevBadge />}
              </div>
            </div>
          </div>
          <Button variant="outline" disabled={snap.refreshing} onClick={() => cmd.refreshUsage()}>
            <FontAwesomeIcon icon={faArrowsRotate} className={snap.refreshing ? "spin" : undefined} /> Refresh usage now
          </Button>
        </div>
        <Row label="Install updates automatically" hint="New releases download in the background and install when the app is idle.">
          <Switch checked={s.autoUpdate} onCheckedChange={(v) => set({ autoUpdate: v })} />
        </Row>
        <UpdatesRow snap={snap} />
        <Row label="Feedback" hint="A bug, a missing feature or a question. It opens a short form on flipon.korol.app.">
          <Button variant="outline" onClick={() => cmd.openUrl(`https://flipon.korol.app/feedback?v=${encodeURIComponent(snap.app.version)}`)}>
            <FontAwesomeIcon icon={faCommentDots} /> Send feedback
          </Button>
        </Row>
      </Card>
      <p className="mt-1 text-xs leading-normal text-muted-foreground">
        Credentials are stored locally, with owner-only file permissions, in the app's data folder — they are only ever sent to Anthropic.
        FlipOn is not affiliated with Anthropic.
      </p>
    </>
  );
}

// ---------------------------------------------------------------------------------- window

export function SettingsWindow() {
  const snap = useSnapshot();
  const [tab, setTab] = useState<Tab>("switching");
  const [draft, setDraft] = useState<Settings | null>(null);
  const [save, setSave] = useState<SaveState>("saved");
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const pending = useRef<Settings | null>(null);

  // adopt the backend's settings once; after that this window is the only writer
  useEffect(() => {
    if (snap && !draft) setDraft(snap.settings);
  }, [snap, draft]);

  const flush = useCallback(async () => {
    clearTimeout(timer.current);
    const next = pending.current;
    if (!next) return;
    pending.current = null;
    setSave("saving");
    try {
      await cmd.updateSettings(next);
      setSave("saved");
    } catch (e) {
      setSave({ error: String(e) });
    }
  }, []);

  const set = useCallback(
    (patch: Partial<Settings>) => {
      setDraft((d) => {
        if (!d) return d;
        const next = { ...d, ...patch };
        pending.current = next;
        return next;
      });
      setSave("saving");
      clearTimeout(timer.current);
      timer.current = setTimeout(flush, 350);
    },
    [flush]
  );

  // closing the window inside the debounce must not lose the last change
  useEffect(
    () => () => {
      clearTimeout(timer.current);
      if (pending.current) void cmd.updateSettings(pending.current);
    },
    []
  );

  if (!snap || !draft) return null;
  const info = TABS.find((t) => t.id === tab)!;
  const active = snap.accounts.find((a) => a.id === snap.activeId);

  return (
    <div className="flex h-full bg-background">
      <nav className="flex w-[180px] shrink-0 flex-col gap-0.5 border-r border-border bg-muted px-2.5 py-4">
        <div className="flex items-center gap-2 px-2 pb-4 pt-0.5">
          <AppMark size={22} />
          <span className="text-sm font-semibold tracking-[-0.01em]">FlipOn</span>
        </div>
        {TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={cn(
              "flex h-8 w-full items-center gap-[9px] rounded-lg px-2 text-left text-[13px]",
              tab === t.id ? "bg-background font-medium text-foreground shadow-[0_1px_2px_rgb(0_0_0/0.06)]" : "text-muted-foreground hover:bg-border hover:text-foreground"
            )}
          >
            <FontAwesomeIcon icon={t.icon} className="w-[15px] shrink-0 text-center" />
            {t.label}
          </button>
        ))}
        <div className="mt-auto p-2 font-mono text-[11px] text-muted-foreground">
          v{snap.app.version} · {snap.app.platform}
          {snap.app.dev && <DevBadge />}
        </div>
      </nav>

      <div className="min-w-0 flex-1 overflow-y-auto px-[26px] pb-7 pt-5">
        <div className="mb-[18px] flex items-start justify-between gap-4">
          <div>
            <h1 className="text-[17px] font-semibold tracking-[-0.01em]">{info.title}</h1>
            {info.desc && <p className="mt-[3px] max-w-[420px] text-xs text-muted-foreground">{info.desc}</p>}
          </div>
          <div className="flex shrink-0 items-center gap-1.5 whitespace-nowrap text-xs text-muted-foreground">
            {save === "saving" && (
              <>
                <span className="spinner !size-3" /> Saving
              </>
            )}
            {save === "saved" && (
              <>
                <FontAwesomeIcon icon={faCheck} className="text-success" /> Saved
              </>
            )}
            {typeof save === "object" && <span className="text-destructive">{save.error}</span>}
          </div>
        </div>

        {tab === "switching" && <SwitchingTab s={draft} set={set} accounts={snap.accounts} />}
        {tab === "tray" && <TrayTab s={draft} set={set} active={active} />}
        {tab === "system" && <SystemTab s={draft} set={set} cli={snap.cli} flush={flush} />}
        {tab === "about" && <AboutTab s={draft} set={set} snap={snap} />}
      </div>
    </div>
  );
}

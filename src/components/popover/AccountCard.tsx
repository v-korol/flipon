import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faChevronDown,
  faChevronUp,
  faCoins,
  faEllipsis,
  faKey,
  faPen,
  faRightLeft,
  faTrash,
} from "@fortawesome/free-solid-svg-icons";
import { cmd, type Account, type Settings, type UsageWindow } from "@/lib/api";
import { countdown, elapsedPercent, money, toneFor, visibleWindows } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { TrendIcon, UsageBar } from "@/components/usage-bar";

const COMPACT_NAMES: Record<string, string> = {
  five_hour: "5h",
  seven_day: "wk",
  seven_day_opus: "opus",
  seven_day_sonnet: "sonnet",
};

function resetsIn(w: UsageWindow, now: number) {
  return w.resetsAtMs != null ? ` · resets in ${countdown(w.resetsAtMs - now)}` : "";
}

function Status({ account, now }: { account: Account; now: number }) {
  const left = account.statusResetsAtMs != null ? ` · ${countdown(account.statusResetsAtMs - now)}` : "";
  switch (account.status) {
    case "active":
      return <span className="font-medium text-accent">Active</span>;
    case "ready":
      return <span className="text-muted-foreground">Ready</span>;
    case "needs_login":
      return <span className="text-warning">Needs login</span>;
    case "no_backup":
      return <span className="text-warning">No backup</span>;
    case "week_full":
      return <span className="text-destructive">Week full{left}</span>;
    case "five_hour_full":
      return <span className="text-destructive">5h full{left}</span>;
  }
}

function TodayStat({ today }: { today: NonNullable<Account["today"]> }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="flex min-w-0 items-center gap-[5px] truncate text-[11px] text-muted-foreground">
          <FontAwesomeIcon icon={faCoins} />
          {money(today.cost)} today · {today.turns} {today.turns === 1 ? "turn" : "turns"} · {today.lines} lines
        </span>
      </TooltipTrigger>
      <TooltipContent>
        {today.models.length > 0 ? today.models.map(([m, c]) => `${m}: ${money(c)}`).join(" · ") + " — " : ""}
        {today.turns} turns, {today.lines} lines written today. Cost is estimated at API list prices.
      </TooltipContent>
    </Tooltip>
  );
}

function ActiveLimits({ account, settings, now }: { account: Account; settings: Settings; now: number }) {
  const windows = visibleWindows(account, settings);
  return (
    <div className="flex flex-col gap-2.5 border-t border-border pt-2.5">
      {windows.map((w) => (
        <div key={w.key}>
          <div className="flex items-baseline justify-between text-[13px]">
            <span>
              <b>{w.label}</b>
              <span className="font-normal text-muted-foreground">{resetsIn(w, now)}</span>
            </span>
            <span className="font-mono">
              {Math.round(w.utilization)}%
              <TrendIcon trend={w.trend} />
            </span>
          </div>
          <div className="mt-1.5">
            <UsageBar large pct={w.utilization} tone={toneFor(w.utilization, settings.switchThreshold)} elapsed={elapsedPercent(w, now)} />
          </div>
        </div>
      ))}
    </div>
  );
}

function CompactLimits({ account, settings, now }: { account: Account; settings: Settings; now: number }) {
  const windows = visibleWindows(account, settings);
  if (windows.length === 0) return null;
  return (
    <div className="flex flex-col gap-[7px]">
      {windows.map((w) => (
        <div key={w.key} className="text-[11px] text-muted-foreground">
          <div className="flex items-baseline justify-between gap-2">
            <span>
              <span className="font-medium text-foreground">{COMPACT_NAMES[w.key] ?? w.label}</span>
              {resetsIn(w, now)}
            </span>
            <span className="shrink-0 font-mono">
              {Math.round(w.utilization)}%
              <TrendIcon trend={w.trend} />
            </span>
          </div>
          <div className="mt-1">
            <UsageBar pct={w.utilization} tone={toneFor(w.utilization, settings.switchThreshold)} />
          </div>
        </div>
      ))}
    </div>
  );
}

export function AccountCard({
  account,
  index,
  count,
  settings,
  now,
  onSwitch,
  onRename,
}: {
  account: Account;
  index: number;
  count: number;
  settings: Settings;
  now: number;
  onSwitch: (a: Account) => void;
  onRename: (a: Account) => void;
}) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  const active = account.status === "active";
  const needsLogin = account.status === "needs_login";

  return (
    <div
      className={cn(
        "group/card flex gap-1.5 rounded-xl border bg-card py-3 pl-1.5 pr-3 shadow-[0_1px_2px_rgb(0_0_0/0.03)]",
        active ? "border-accent" : "border-border"
      )}
    >
      <div className="flex w-6 shrink-0 flex-col items-center">
        <button
          className="flex size-6 items-center justify-center rounded-md hover:bg-muted disabled:opacity-30 disabled:hover:bg-transparent"
          disabled={index === 0}
          onClick={() => cmd.reorderAccount(account.id, -1)}
          aria-label="Move up"
        >
          <FontAwesomeIcon icon={faChevronUp} className="text-[11px]" />
        </button>
        <span className="font-mono text-[11px] leading-4 text-muted-foreground">{index + 1}</span>
        <button
          className="flex size-6 items-center justify-center rounded-md hover:bg-muted disabled:opacity-30 disabled:hover:bg-transparent"
          disabled={index === count - 1}
          onClick={() => cmd.reorderAccount(account.id, 1)}
          aria-label="Move down"
        >
          <FontAwesomeIcon icon={faChevronDown} className="text-[11px]" />
        </button>
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-2.5">
        <div className="relative flex items-center gap-2.5">
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 items-center gap-[7px]">
              {account.alias ? (
                <>
                  <span className="shrink-0 text-sm font-semibold tracking-[-0.01em]">{account.alias}</span>
                  <span className="min-w-0 truncate text-[11px] text-muted-foreground">{account.email}</span>
                </>
              ) : (
                <span className="min-w-0 truncate text-sm font-semibold tracking-[-0.01em]">{account.email}</span>
              )}
              <Badge className="shrink-0">{account.plan}</Badge>
            </div>
            {account.today && (
              <div className="mt-0.5 flex">
                <TodayStat today={account.today} />
              </div>
            )}
            {/* "Ready" is the normal state, so only problems (needs login, limit reached…) get a line */}
            {!active && account.status !== "ready" && (
              <div className="mt-0.5 text-xs">
                <Status account={account} now={now} />
              </div>
            )}
          </div>

          {!active && !confirmRemove && (
            <Button
              variant="accent"
              size="sm"
              disabled={needsLogin}
              className="pointer-events-none absolute right-10 top-1/2 -translate-y-1/2 scale-[0.96] opacity-0 shadow-[0_2px_8px_rgb(0_0_0/0.18),-18px_0_14px_-6px_var(--card)] transition-[opacity,transform] duration-150 group-hover/card:pointer-events-auto group-hover/card:scale-100 group-hover/card:opacity-100"
              onClick={() => onSwitch(account)}
            >
              Switch
            </Button>
          )}

          <DropdownMenu>
            <Tooltip>
              <TooltipTrigger asChild>
                <DropdownMenuTrigger asChild>
                  <Button variant="ghost" size="icon-sm" aria-label="More">
                    <FontAwesomeIcon icon={faEllipsis} />
                  </Button>
                </DropdownMenuTrigger>
              </TooltipTrigger>
              <TooltipContent>More</TooltipContent>
            </Tooltip>
            <DropdownMenuContent side="left" align="start">
              <DropdownMenuLabel>{account.email}</DropdownMenuLabel>
              {!active && (
                <DropdownMenuItem disabled={needsLogin} onSelect={() => onSwitch(account)}>
                  <FontAwesomeIcon icon={faRightLeft} /> Switch to this
                </DropdownMenuItem>
              )}
              <DropdownMenuItem onSelect={() => onRename(account)}>
                <FontAwesomeIcon icon={faPen} /> Rename
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => cmd.reloginAccount(account.id)}>
                <FontAwesomeIcon icon={faKey} /> Log in again
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onSelect={() => setConfirmRemove(true)}>
                <FontAwesomeIcon icon={faTrash} /> Remove account
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {confirmRemove && (
          <div className="flex items-center gap-2 rounded-lg bg-muted px-2.5 py-2 text-xs">
            <span className="flex-1">Remove this account from the app?</span>
            <Button variant="ghost" size="sm" onClick={() => setConfirmRemove(false)}>
              Cancel
            </Button>
            <Button variant="destructive" size="sm" onClick={() => cmd.removeAccount(account.id)}>
              Remove
            </Button>
          </div>
        )}

        {active ? (
          <ActiveLimits account={account} settings={settings} now={now} />
        ) : (
          <CompactLimits account={account} settings={settings} now={now} />
        )}
      </div>
    </div>
  );
}

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faArrowsRotate, faGear, faPlus, faRightLeft } from "@fortawesome/free-solid-svg-icons";
import { cmd, useNow, useSnapshot, type Account } from "@/lib/api";
import { DevBadge } from "@/components/settings/Settings";
import { STRATEGY_NAMES } from "@/lib/format";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { AccountCard } from "./AccountCard";
import { HoldDialog, LoginDialog, RenameDialog } from "./Dialogs";
import { Notices } from "./Notices";

export function Popover() {
  const snap = useSnapshot();
  const now = useNow();
  const rootRef = useRef<HTMLDivElement>(null);
  const [switchTarget, setSwitchTarget] = useState<Account | null>(null);
  const [renameTarget, setRenameTarget] = useState<Account | null>(null);

  // The window hugs its content: report our height whenever it changes.
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const report = () => cmd.resizePopover(Math.ceil(el.getBoundingClientRect().height));
    const ro = new ResizeObserver(report);
    ro.observe(el);
    report();
    return () => ro.disconnect();
  }, [snap != null]);

  // Escape closes the popover — unless it is meant for an open menu or dialog.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (document.querySelector('[role="menu"],[role="dialog"]')) return;
      cmd.hidePopover();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  if (!snap) return null;
  const { accounts, settings, login, cli } = snap;
  const dialogOpen = switchTarget != null || renameTarget != null || login.running;
  const addDisabled = !cli.found || login.running || login.adding;

  return (
    <div
      ref={rootRef}
      className={cn(
        "flex w-full flex-col overflow-hidden rounded-[10px] border border-border bg-background",
        dialogOpen && "min-h-[330px]"
      )}
    >
      <div className="flex items-center gap-1.5 border-b border-border py-3 pl-4 pr-3">
        <div className="min-w-0 flex-1">
          <div className="text-[15px] font-semibold tracking-[-0.01em]">
            Accounts{snap?.app.dev && <DevBadge />}
          </div>
          <div className="text-xs text-muted-foreground">
            {accounts.length === 0 ? "No accounts saved yet" : `${accounts.length} saved · ${STRATEGY_NAMES[settings.strategy]}`}
          </div>
        </div>
        <Button variant="outline" size="sm" disabled={addDisabled} onClick={() => cmd.addAccount()}>
          <FontAwesomeIcon icon={faPlus} /> Add
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-sm" disabled={snap.refreshing} onClick={() => cmd.refreshUsage()} aria-label="Refresh usage">
              <FontAwesomeIcon icon={faArrowsRotate} className={snap.refreshing ? "spin" : undefined} />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Refresh usage</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon-sm" onClick={() => cmd.openSettings()} aria-label="Settings">
              <FontAwesomeIcon icon={faGear} />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Settings</TooltipContent>
        </Tooltip>
      </div>

      <div className="flex max-h-[660px] flex-col gap-2.5 overflow-auto p-3">
        <Notices snap={snap} now={now} />
        {accounts.length === 0 ? (
          <div className="flex flex-col items-center rounded-xl border border-dashed border-border px-5 py-8 text-center">
            <div className="mb-3 flex size-11 items-center justify-center rounded-full bg-muted">
              <FontAwesomeIcon icon={faRightLeft} />
            </div>
            <h3 className="text-[15px] font-semibold">Keep every Claude login</h3>
            <p className="mt-1 max-w-[260px] text-[13px] text-muted-foreground">
              Add each account once. Switching later takes one click and never signs you out.
            </p>
            <Button className="mt-3.5" disabled={addDisabled} onClick={() => cmd.addAccount()}>
              <FontAwesomeIcon icon={faPlus} /> Add account
            </Button>
          </div>
        ) : (
          accounts.map((a, i) => (
            <AccountCard
              key={a.id}
              account={a}
              index={i}
              count={accounts.length}
              settings={settings}
              now={now}
              onSwitch={setSwitchTarget}
              onRename={setRenameTarget}
            />
          ))
        )}
      </div>

      <HoldDialog account={switchTarget} onClose={() => setSwitchTarget(null)} />
      <RenameDialog account={renameTarget} onClose={() => setRenameTarget(null)} />
      <LoginDialog login={login} accounts={accounts} />
    </div>
  );
}

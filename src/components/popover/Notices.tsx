import type { ReactNode } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faDownload, faKey, faThumbtack, faTriangleExclamation, faXmark, type IconDefinition } from "@fortawesome/free-solid-svg-icons";
import { cmd, type Snapshot } from "@/lib/api";
import { countdown } from "@/lib/format";
import { cn } from "@/lib/utils";

function Notice({
  tone = "default",
  icon,
  children,
  onClose,
}: {
  tone?: "default" | "warn" | "critical";
  icon: IconDefinition;
  children: ReactNode;
  onClose?: () => void;
}) {
  return (
    <div
      className={cn(
        "flex items-start gap-2 rounded-[10px] border border-border bg-background px-3 py-2 text-xs leading-[1.4]",
        tone === "warn" && "text-warning",
        tone === "critical" && "text-destructive"
      )}
    >
      <span className="pt-px">
        <FontAwesomeIcon icon={icon} />
      </span>
      <span className={cn("flex-1", tone === "default" && "text-foreground")}>{children}</span>
      {onClose && (
        <button className="opacity-70 hover:opacity-100" onClick={onClose} aria-label="Dismiss">
          <FontAwesomeIcon icon={faXmark} />
        </button>
      )}
    </div>
  );
}

function Action({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button className="ml-1 text-xs font-medium underline underline-offset-2" onClick={onClick}>
      {children}
    </button>
  );
}

/** Banners stacked above the account list, most relevant on top. */
export function Notices({ snap, now }: { snap: Snapshot; now: number }) {
  const hold = snap.hold;
  return (
    <>
      {snap.error && (
        <Notice tone="critical" icon={faTriangleExclamation} onClose={() => cmd.dismissError()}>
          {snap.error.message}
        </Notice>
      )}
      {snap.update.status === "ready" && snap.update.version && (
        <Notice icon={faDownload}>
          Version {snap.update.version} is ready.
          <Action onClick={() => cmd.installUpdate()}>Restart to update</Action>
        </Notice>
      )}
      {hold && snap.activeId === hold.accountId && (
        <Notice icon={faThumbtack}>
          {hold.untilMs != null ? `Pinned for ${countdown(hold.untilMs - now)}.` : "Pinned until you switch again."} Auto-switch
          only leaves it at the limit.
          <Action onClick={() => cmd.releaseHold()}>Release</Action>
        </Notice>
      )}
      {snap.unknownLogin && (
        <Notice tone="warn" icon={faKey}>
          The CLI is logged in as {snap.unknownLogin}, which is not saved yet.
          <Action onClick={() => cmd.saveUnknownLogin()}>Save it</Action>
        </Notice>
      )}
      {!snap.cli.found && (
        <Notice tone="warn" icon={faTriangleExclamation}>
          Claude Code CLI not found. Install it or set its path in Settings.
        </Notice>
      )}
    </>
  );
}

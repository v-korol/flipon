import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faArrowUpRightFromSquare, faClockRotateLeft, faInfinity, faKey } from "@fortawesome/free-solid-svg-icons";
import { cmd, type Account, type Snapshot } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

function DialogIcon({ children, center }: { children: React.ReactNode; center?: boolean }) {
  return (
    <div
      className={
        "mb-3 flex size-10 items-center justify-center rounded-[11px] bg-[color-mix(in_oklab,var(--accent)_14%,var(--muted))] text-[15px] text-accent" +
        (center ? " mx-auto" : "")
      }
    >
      {children}
    </div>
  );
}

/** "Switch for how long?" — the Hold picker. */
export function HoldDialog({ account, onClose }: { account: Account | null; onClose: () => void }) {
  const choose = (hours: number | null) => {
    if (!account) return;
    cmd.switchAccount(account.id, hours);
    onClose();
  };
  return (
    <Dialog open={account != null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogIcon>
          <FontAwesomeIcon icon={faClockRotateLeft} />
        </DialogIcon>
        <DialogTitle className="text-[15px] font-semibold">Switch for how long?</DialogTitle>
        <div className="mt-0.5 truncate text-xs text-muted-foreground">{account?.email}</div>
        <DialogDescription className="mt-2.5 text-[13px] leading-normal text-muted-foreground">
          Auto-switch won't move away during this time unless this account hits its limit.
        </DialogDescription>
        <div className="mt-3.5 flex gap-1.5">
          {[1, 2, 3, 4].map((h) => (
            <button
              key={h}
              onClick={() => choose(h)}
              className="h-[34px] flex-1 rounded-full border border-input bg-background text-[13px] font-medium transition-colors hover:border-accent hover:bg-[color-mix(in_oklab,var(--accent)_6%,var(--background))] hover:text-accent"
            >
              {h} h
            </button>
          ))}
        </div>
        <Button className="mt-2.5 w-full" onClick={() => choose(null)}>
          <FontAwesomeIcon icon={faInfinity} /> No timer
        </Button>
      </DialogContent>
    </Dialog>
  );
}

export function RenameDialog({ account, onClose }: { account: Account | null; onClose: () => void }) {
  const [value, setValue] = useState("");
  useEffect(() => setValue(account?.alias ?? ""), [account]);
  const save = () => {
    if (account) cmd.renameAccount(account.id, value);
    onClose();
  };
  return (
    <Dialog open={account != null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogTitle className="text-[15px] font-semibold">Rename account</DialogTitle>
        <DialogDescription className="mt-0.5 truncate text-xs text-muted-foreground">{account?.email}</DialogDescription>
        <div className="mt-3.5">
          <Input
            autoFocus
            className="font-sans text-[13px]"
            placeholder="Alias, e.g. Work (empty to clear)"
            value={value}
            maxLength={40}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && save()}
          />
        </div>
        <div className="mt-3 flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={save}>Save</Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** How long to wait before offering the code step: the CLI always prints the "paste code" prompt,
 *  but when the browser redirect works it finishes on its own. */
const CODE_STEP_DELAY_MS = 12_000;

export function LoginDialog({ login, accounts }: { login: Snapshot["login"]; accounts: Account[] }) {
  const [manual, setManual] = useState(false);
  const [late, setLate] = useState(false);
  const [code, setCode] = useState("");

  useEffect(() => {
    if (!login.running) {
      setManual(false);
      setLate(false);
      setCode("");
      return;
    }
    const t = setTimeout(() => setLate(true), CODE_STEP_DELAY_MS);
    return () => clearTimeout(t);
  }, [login.running]);

  const stepB = login.running && login.needsCode && (manual || late);
  const target = accounts.find((a) => a.id === login.targetId);

  const submit = () => {
    if (!code.trim()) return;
    cmd.submitLoginCode(code.trim());
    setCode("");
    setManual(false);
    setLate(false);
  };

  return (
    <Dialog open={login.running} onOpenChange={(o) => !o && cmd.cancelLogin()}>
      <DialogContent className="text-center">
        {stepB ? (
          <>
            <DialogIcon center>
              <FontAwesomeIcon icon={faKey} />
            </DialogIcon>
            <DialogTitle className="text-[15px] font-semibold">Paste the code from your browser</DialogTitle>
            <DialogDescription className="mt-1.5 text-[13px] leading-normal text-muted-foreground">
              Claude showed a code after you signed in. Copy it and paste it here to finish.
            </DialogDescription>
            <div className="mt-3.5 text-left">
              <label htmlFor="login-code" className="mb-1.5 block text-xs font-medium">
                Authorization code
              </label>
              <Input
                id="login-code"
                autoFocus
                placeholder="e.g. 7F93-ANQ2"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && submit()}
              />
              <Button className="mt-2.5 w-full" disabled={!code.trim()} onClick={submit}>
                Continue
              </Button>
            </div>
          </>
        ) : (
          <>
            <DialogIcon center>
              <span className="spinner !size-[17px] !border-[color-mix(in_oklab,var(--accent)_24%,transparent)] !border-t-accent" />
            </DialogIcon>
            <DialogTitle className="text-[15px] font-semibold">Waiting for browser login</DialogTitle>
            <DialogDescription className="mt-1.5 text-[13px] leading-normal text-muted-foreground">
              {target ? `Sign in as ${target.email} in your browser. ` : "Finish signing in to Claude in your browser. "}
              Your other accounts stay saved.
            </DialogDescription>
            <Button
              variant="outline"
              size="sm"
              className="mt-4"
              disabled={!login.url}
              onClick={() => {
                if (login.url) cmd.openUrl(login.url);
                setManual(true);
              }}
            >
              <FontAwesomeIcon icon={faArrowUpRightFromSquare} /> Open login page
            </Button>
            {login.needsCode && (
              <div className="mt-2">
                <button className="text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground" onClick={() => setManual(true)}>
                  I have a code
                </button>
              </div>
            )}
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}


import * as React from "react"
import { Dialog as DialogPrimitive } from "radix-ui"
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome"
import { faXmark } from "@fortawesome/free-solid-svg-icons"
import { cn } from "@/lib/utils"

const Dialog = DialogPrimitive.Root
const DialogTitle = DialogPrimitive.Title
const DialogDescription = DialogPrimitive.Description

/** Dialog rendered as an overlay inside the popover window: blurred scrim + centered card. */
function DialogContent({
  className,
  children,
  ...props
}: React.ComponentProps<typeof DialogPrimitive.Content>) {
  return (
    <DialogPrimitive.Portal>
      <DialogPrimitive.Overlay className="fixed inset-0 z-40 rounded-[10px] bg-[var(--overlay)] backdrop-blur-[3px]" />
      <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center p-5">
        <DialogPrimitive.Content
          data-slot="dialog-content"
          className={cn(
            "pointer-events-auto relative w-full rounded-2xl border border-border bg-background px-5 pb-5 pt-[22px] shadow-[var(--shadow-popover)] outline-none",
            className
          )}
          {...props}
        >
          {children}
          <DialogPrimitive.Close
            aria-label="Close"
            className="absolute right-2.5 top-2.5 flex size-[26px] items-center justify-center rounded-full text-[11px] text-muted-foreground hover:bg-muted hover:text-foreground"
          >
            <FontAwesomeIcon icon={faXmark} />
          </DialogPrimitive.Close>
        </DialogPrimitive.Content>
      </div>
    </DialogPrimitive.Portal>
  )
}

export { Dialog, DialogContent, DialogTitle, DialogDescription }

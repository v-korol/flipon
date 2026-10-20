import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

const badgeVariants = cva(
  "inline-flex h-[18px] items-center whitespace-nowrap rounded-md px-1.5 text-[11px] font-medium",
  {
    variants: {
      variant: {
        outline: "border border-border",
        secondary: "bg-[color-mix(in_oklab,var(--accent)_16%,var(--muted))] text-accent",
      },
    },
    defaultVariants: { variant: "outline" },
  }
)

function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span data-slot="badge" className={cn(badgeVariants({ variant }), className)} {...props} />
}

export { Badge, badgeVariants }

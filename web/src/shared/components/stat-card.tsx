import type { ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { Card, CardContent } from '@/shared/components/ui/card'

interface StatCardProps {
  label: string
  value: ReactNode
  hint?: ReactNode
  to?: string
}

/** A single figure with a label, optionally linking to its page. */
export function StatCard({ label, value, hint, to }: StatCardProps) {
  const body = (
    <CardContent className="flex flex-col gap-1">
      <span className="text-sm text-muted-foreground">{label}</span>
      <span className="font-heading text-2xl font-semibold tabular-nums">{value}</span>
      {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
    </CardContent>
  )
  return to
    ? <Link to={to} className="rounded-xl outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"><Card className="h-full transition-colors hover:bg-muted/40">{body}</Card></Link>
    : <Card className="h-full">{body}</Card>
}

/** Formats a unix timestamp in seconds for display; empty for `null`. */
export function formatUnix(seconds: number | null | undefined): string {
  if (!seconds)
    return ''
  return new Date(seconds * 1000).toLocaleString()
}

/** Formats an RFC 3339 timestamp for display. */
export function formatIso(value: string): string {
  const d = new Date(value)
  return Number.isNaN(d.getTime()) ? value : d.toLocaleString()
}

/** Shortens a long secret for display, keeping both ends. */
export function abbreviate(value: string, keep = 6): string {
  return value.length <= keep * 2 + 1 ? value : `${value.slice(0, keep)}…${value.slice(-keep)}`
}

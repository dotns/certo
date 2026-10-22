/** Number of entries per owner name. */
export function countByOwner(items: Array<{ owner?: string }> | undefined): Record<string, number> {
  const counts: Record<string, number> = {}
  for (const item of items ?? []) {
    if (item.owner)
      counts[item.owner] = (counts[item.owner] ?? 0) + 1
  }
  return counts
}

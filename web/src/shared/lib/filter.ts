/** Case-insensitive substring match of `query` against any of `fields`; empty query matches all. */
export function matches(query: string, ...fields: Array<string | number | null | undefined>): boolean {
  const q = query.trim().toLowerCase()
  if (!q)
    return true
  return fields.some(f => f !== null && f !== undefined && String(f).toLowerCase().includes(q))
}

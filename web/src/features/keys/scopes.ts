/**
 * Normalizes the scope list of a new key: lowercase, trimmed, empty entries dropped,
 * exact names covered by a `*.` wildcard removed, duplicates removed. Empty → global.
 */
export function normalizeScopes(scopes: string[]): string[] {
  const cleaned = scopes.map(s => s.trim().toLowerCase().replace(/\.$/, '')).filter(Boolean)
  const wildcards = new Set(cleaned.filter(s => s.startsWith('*.')))
  const result: string[] = []
  for (const s of cleaned) {
    const covered = !s.startsWith('*.') && wildcards.has(`*.${s}`)
    if (!covered && !result.includes(s))
      result.push(s)
  }
  return result.length > 0 ? result : ['*']
}

/** Whether a scope list grants access to every domain. */
export function isGlobalScope(scope: string[]): boolean {
  return scope.includes('*')
}

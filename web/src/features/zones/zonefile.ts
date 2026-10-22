import type { RrSet, RrSetChange } from './api'
import { DEFAULT_TTL, isManagedType, relativeName } from './rrsets'

/** Why a line was skipped: a `zones.import.reason.*` key and its interpolation values. */
export interface SkipReason {
  key: string
  params?: Record<string, string>
}

export interface SkippedLine {
  line: number
  text: string
  reason: SkipReason
}

export interface ParsedZoneFile {
  changes: RrSetChange[]
  skipped: SkippedLine[]
}

/** Zone-file text for the rrsets of `zone`, relative names, one record per line. */
export function exportZoneFile(zone: string, rrsets: RrSet[]): string {
  const lines = [`$ORIGIN ${zone}.`, `$TTL ${DEFAULT_TTL}`, '']
  for (const rr of rrsets) {
    const name = relativeName(rr.name, zone)
    for (const r of rr.records)
      lines.push(`${name}\t${rr.ttl}\tIN\t${rr.type}\t${r}`)
  }
  return `${lines.join('\n')}\n`
}

/** Splits a line into tokens, keeping quoted strings (with their quotes) together. */
export function tokenize(line: string): string[] {
  const tokens: string[] = []
  let current = ''
  let quoted = false
  for (let i = 0; i < line.length; i++) {
    const c = line[i]
    if (c === '"') {
      quoted = !quoted
      current += c
    }
    else if (c === '\\' && quoted && i + 1 < line.length) {
      current += c + line[++i]
    }
    else if (!quoted && c === ';') {
      break
    }
    else if (!quoted && /\s/.test(c ?? '')) {
      if (current)
        tokens.push(current)
      current = ''
    }
    else {
      current += c
    }
  }
  if (current)
    tokens.push(current)
  return tokens
}

/** Joins parenthesised multi-line records into single logical lines, keeping line numbers. */
function logicalLines(text: string): Array<{ line: number, text: string }> {
  const out: Array<{ line: number, text: string }> = []
  let buffer = ''
  let start = 0
  let depth = 0
  text.split(/\r?\n/).forEach((raw, i) => {
    const stripped = raw.replace(/;.*$/, '')
    if (depth === 0) {
      start = i + 1
      buffer = ''
    }
    buffer += (depth > 0 ? ' ' : '') + stripped
    depth += (stripped.match(/\(/g) ?? []).length - (stripped.match(/\)/g) ?? []).length
    if (depth <= 0) {
      depth = 0
      out.push({ line: start, text: buffer.replace(/[()]/g, ' ') })
    }
  })
  if (depth > 0)
    out.push({ line: start, text: buffer.replace(/[()]/g, ' ') })
  return out
}

/** Owner name relative to `zone` (`@` for the apex), or `null` when outside the zone. */
function ownerName(token: string, origin: string, zone: string): string | null {
  const lower = token.toLowerCase()
  if (lower === '@')
    return origin === zone ? '@' : relativeName(`${origin}.`, zone)
  const absolute = lower.endsWith('.') ? lower.slice(0, -1) : (origin ? `${lower}.${origin}` : lower)
  if (absolute !== zone && !absolute.endsWith(`.${zone}`))
    return null
  return relativeName(`${absolute}.`, zone)
}

/**
 * Parses zone-file lines into REPLACE changes for `zone`. Records of the same name and
 * type form one change; unknown types, SOA lines and names outside the zone are listed
 * in `skipped`.
 */
export function parseZoneFile(text: string, zone: string): ParsedZoneFile {
  const changes = new Map<string, RrSetChange>()
  const skipped: SkippedLine[] = []
  let origin = zone
  let defaultTtl: number | undefined
  let lastOwner = '@'

  for (const { line, text: raw } of logicalLines(text)) {
    const tokens = tokenize(raw)
    if (tokens.length === 0)
      continue
    const skip = (key: string, params?: Record<string, string>) => skipped.push({ line, text: raw.trim(), reason: { key, params } })
    const first = tokens[0] ?? ''
    if (first.toUpperCase() === '$ORIGIN') {
      origin = (tokens[1] ?? '').toLowerCase().replace(/\.$/, '')
      continue
    }
    if (first.toUpperCase() === '$TTL') {
      defaultTtl = Number.parseInt(tokens[1] ?? '', 10) || undefined
      continue
    }
    if (first.startsWith('$')) {
      skip('directive', { name: first })
      continue
    }
    let rest = tokens
    let owner: string | null
    if (/^\s/.test(raw)) {
      owner = lastOwner
    }
    else {
      owner = ownerName(first, origin, zone)
      rest = tokens.slice(1)
      if (owner === null) {
        skip('outside', { name: first, zone })
        continue
      }
      lastOwner = owner
    }
    let ttl = defaultTtl
    if (/^\d+$/.test(rest[0] ?? '')) {
      ttl = Number.parseInt(rest[0] ?? '', 10)
      rest = rest.slice(1)
    }
    if ((rest[0] ?? '').toUpperCase() === 'IN')
      rest = rest.slice(1)
    if (/^\d+$/.test(rest[0] ?? '')) {
      ttl = Number.parseInt(rest[0] ?? '', 10)
      rest = rest.slice(1)
    }
    const type = (rest[0] ?? '').toUpperCase()
    const rdata = rest.slice(1).join(' ')
    if (!type) {
      skip('noType')
      continue
    }
    if (type === 'SOA') {
      skip('soa')
      continue
    }
    if (!isManagedType(type)) {
      skip('type', { type })
      continue
    }
    if (!rdata) {
      skip('noData')
      continue
    }
    const key = `${owner}/${type}`
    const change = changes.get(key)
    if (change) {
      change.records?.push(rdata)
    }
    else {
      changes.set(key, { name: owner, type, ...(ttl ? { ttl } : {}), changetype: 'REPLACE', records: [rdata] })
    }
  }
  return { changes: [...changes.values()], skipped }
}

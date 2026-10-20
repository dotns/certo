import { describe, expect, it } from 'vitest'
import { abbreviate, formatIso, formatUnix } from '../format'

describe('format', () => {
  it('formats unix seconds and leaves empty values empty', () => {
    expect(formatUnix(null)).toBe('')
    expect(formatUnix(0)).toBe('')
    expect(formatUnix(1_700_000_000)).toBe(new Date(1_700_000_000_000).toLocaleString())
  })

  it('formats RFC 3339 and passes garbage through', () => {
    expect(formatIso('2026-10-18T07:00:00Z')).toBe(new Date('2026-10-18T07:00:00Z').toLocaleString())
    expect(formatIso('nope')).toBe('nope')
  })

  it('abbreviates long secrets only', () => {
    expect(abbreviate('short')).toBe('short')
    expect(abbreviate('abcdefghijklmnopqrstuvwxyz', 4)).toBe('abcd…wxyz')
  })
})

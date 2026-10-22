import { describe, expect, it } from 'vitest'
import { matches } from '../filter'

describe('matches', () => {
  it('matches any field case-insensitively and accepts an empty query', () => {
    expect(matches('', 'a')).toBe(true)
    expect(matches('  ', null, undefined)).toBe(true)
    expect(matches('EXA', 'www.example.com')).toBe(true)
    expect(matches('42', 'x', 42)).toBe(true)
    expect(matches('zzz', 'example', null)).toBe(false)
  })
})

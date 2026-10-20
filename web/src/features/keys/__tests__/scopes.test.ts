import { describe, expect, it } from 'vitest'
import { isGlobalScope, normalizeScopes } from '../scopes'

describe('normalizeScopes', () => {
  it('is global when empty', () => {
    expect(normalizeScopes([])).toEqual(['*'])
    expect(normalizeScopes(['', '  '])).toEqual(['*'])
  })

  it('lowercases, trims, drops trailing dots and duplicates', () => {
    expect(normalizeScopes([' Example.COM. ', 'example.com'])).toEqual(['example.com'])
  })

  it('drops exact names covered by a wildcard', () => {
    expect(normalizeScopes(['example.com', '*.example.com', 'other.test'])).toEqual(['*.example.com', 'other.test'])
  })

  it('recognizes global scopes', () => {
    expect(isGlobalScope(['*'])).toBe(true)
    expect(isGlobalScope(['*.a.test'])).toBe(false)
  })
})

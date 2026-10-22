import { describe, expect, it } from 'vitest'
import { countByOwner } from '../counts'

describe('countByOwner', () => {
  it('counts entries per owner and ignores system rows', () => {
    expect(countByOwner([{ owner: 'a' }, { owner: 'b' }, { owner: 'a' }, {}])).toEqual({ a: 2, b: 1 })
    expect(countByOwner(undefined)).toEqual({})
  })
})

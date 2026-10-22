import { describe, expect, it } from 'vitest'
import en from '../../../../public/locales/en/common.json'
import zhCN from '../../../../public/locales/zh-CN/common.json'

/** Dotted key paths of every leaf in a locale file. */
function leafKeys(obj: unknown, prefix = ''): string[] {
  if (typeof obj !== 'object' || obj === null)
    return [prefix]
  return Object.entries(obj).flatMap(([k, v]) => leafKeys(v, prefix ? `${prefix}.${k}` : k))
}

function leaves(obj: unknown, prefix = ''): Array<[string, unknown]> {
  if (typeof obj !== 'object' || obj === null)
    return [[prefix, obj]]
  return Object.entries(obj).flatMap(([k, v]) => leaves(v, prefix ? `${prefix}.${k}` : k))
}

const placeholders = (s: string) => [...s.matchAll(/\{\{(\w+)\}\}/g)].map(m => m[1]).sort()

describe('locale files', () => {
  it('have the same keys', () => {
    expect(leafKeys(zhCN).sort()).toEqual(leafKeys(en).sort())
  })

  it('have non-empty string values', () => {
    for (const file of [en, zhCN]) {
      for (const [key, value] of leaves(file))
        expect(typeof value === 'string' && value.trim().length > 0, key).toBe(true)
    }
  })

  it('use the same placeholders in every translation', () => {
    const enMap = new Map(leaves(en))
    for (const [key, value] of leaves(zhCN))
      expect(placeholders(String(value)), key).toEqual(placeholders(String(enMap.get(key))))
  })
})

import { describe, expect, it } from 'vitest'
import {
  changeFromForm,
  defaultRecordType,
  deleteChange,
  dsLines,
  emptyForm,
  formFromRrset,
  isManagedType,
  isReverseZone,
  parseAllowList,
  parseRecords,
  relativeName,
  unquoteTxt,
  validateForm,
  zoneTone,
} from '../rrsets'

describe('rrset helpers', () => {
  it('detects reverse zones and picks the default type', () => {
    expect(isReverseZone('23.251.162.in-addr.arpa')).toBe(true)
    expect(isReverseZone('8.b.d.0.1.0.0.2.ip6.arpa')).toBe(true)
    expect(isReverseZone('example.com')).toBe(false)
    expect(defaultRecordType('2.0.192.in-addr.arpa')).toBe('PTR')
    expect(defaultRecordType('example.com')).toBe('A')
  })

  it('relativizes owner names', () => {
    expect(relativeName('example.com.', 'example.com')).toBe('@')
    expect(relativeName('www.example.com.', 'example.com')).toBe('www')
    expect(relativeName('other.test.', 'example.com')).toBe('other.test')
  })

  it('treats SOA as read-only', () => {
    expect(isManagedType('SOA')).toBe(false)
    expect(isManagedType('NS')).toBe(true)
  })

  it('unquotes single-string TXT values', () => {
    expect(unquoteTxt('"v=spf1 -all"')).toBe('v=spf1 -all')
    expect(unquoteTxt('"a \\"b\\""')).toBe('a "b"')
    expect(unquoteTxt('"a" "b"')).toBe('"a" "b"')
    expect(unquoteTxt('plain')).toBe('plain')
  })

  it('builds forms from rrsets and back into changes', () => {
    const form = formFromRrset({ name: 'example.com.', type: 'TXT', ttl: 60, records: ['"hello"'] }, 'example.com')
    expect(form).toEqual({ name: '@', type: 'TXT', ttl: '60', records: 'hello' })
    expect(changeFromForm(form)).toEqual({ name: '@', type: 'TXT', ttl: 60, changetype: 'REPLACE', records: ['hello'] })
    expect(changeFromForm({ ...emptyForm('example.com'), records: ' 192.0.2.1 \n\n192.0.2.2' })).toEqual({
      name: '@',
      type: 'A',
      changetype: 'REPLACE',
      records: ['192.0.2.1', '192.0.2.2'],
    })
    expect(deleteChange({ name: 'www.example.com.', type: 'A', ttl: 60, records: [] })).toEqual({ name: 'www.example.com.', type: 'A', changetype: 'DELETE' })
  })

  it('validates the form', () => {
    expect(validateForm({ name: '', type: 'A', ttl: '', records: '192.0.2.1' })).toEqual([])
    expect(validateForm({ name: '', type: 'A', ttl: 'abc', records: '' })).toEqual([
      'Enter at least one record',
      'TTL must be a whole number of seconds',
    ])
    expect(validateForm({ name: '', type: '', ttl: '', records: 'x' })).toEqual(['Choose a record type'])
    expect(parseRecords('a\n b \n\n')).toEqual(['a', 'b'])
    expect(parseAllowList('192.0.2.1, 2001:db8::/64\n\n10.0.0.0/8 ')).toEqual(['192.0.2.1', '2001:db8::/64', '10.0.0.0/8'])
  })

  it('derives the status tone and DS lines', () => {
    expect(zoneTone({ status: 'pending', exempt: false, system: false })).toBe('pending')
    expect(zoneTone({ status: 'active', exempt: false, system: false })).toBe('active')
    expect(zoneTone({ status: 'active', exempt: true, system: false })).toBe('exempt')
    expect(zoneTone({ status: 'active', exempt: true, system: true })).toBe('exempt')
    expect(dsLines('example.com', ['1 13 2 ab', '1 13 4 cd'])).toBe('example.com. IN DS 1 13 2 ab\nexample.com. IN DS 1 13 4 cd')
  })
})

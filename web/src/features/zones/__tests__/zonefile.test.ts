import { describe, expect, it } from 'vitest'
import { exportZoneFile, parseZoneFile, tokenize } from '../zonefile'

describe('tokenize', () => {
  it('keeps quoted strings together and drops comments', () => {
    expect(tokenize('@ IN TXT "v=spf1 -all" ; note')).toEqual(['@', 'IN', 'TXT', '"v=spf1 -all"'])
    expect(tokenize('x TXT "a;b" "c\\"d"')).toEqual(['x', 'TXT', '"a;b"', '"c\\"d"'])
  })
})

describe('exportZoneFile', () => {
  it('writes relative names with $ORIGIN', () => {
    const out = exportZoneFile('example.com', [
      { name: 'example.com.', type: 'A', ttl: 300, records: ['192.0.2.1', '192.0.2.2'] },
      { name: 'www.example.com.', type: 'CNAME', ttl: 3600, records: ['example.com.'] },
    ])
    expect(out).toBe('$ORIGIN example.com.\n$TTL 3600\n\n@\t300\tIN\tA\t192.0.2.1\n@\t300\tIN\tA\t192.0.2.2\nwww\t3600\tIN\tCNAME\texample.com.\n')
  })
})

describe('parseZoneFile', () => {
  it('groups records by name and type with optional ttl and class', () => {
    const { changes, skipped } = parseZoneFile([
      '$ORIGIN example.com.',
      '$TTL 600',
      '@ IN A 192.0.2.1',
      '@ A 192.0.2.2',
      'www 300 IN CNAME example.com.',
      'mail.example.com. IN 120 MX 10 mx.example.net.',
      '  IN MX 20 mx2.example.net.',
      'txt TXT "v=spf1 -all"',
    ].join('\n'), 'example.com')
    expect(skipped).toEqual([])
    expect(changes).toEqual([
      { name: '@', type: 'A', ttl: 600, changetype: 'REPLACE', records: ['192.0.2.1', '192.0.2.2'] },
      { name: 'www', type: 'CNAME', ttl: 300, changetype: 'REPLACE', records: ['example.com.'] },
      { name: 'mail', type: 'MX', ttl: 120, changetype: 'REPLACE', records: ['10 mx.example.net.', '20 mx2.example.net.'] },
      { name: 'txt', type: 'TXT', ttl: 600, changetype: 'REPLACE', records: ['"v=spf1 -all"'] },
    ])
  })

  it('skips SOA, unsupported types, foreign names and directives', () => {
    const { changes, skipped } = parseZoneFile([
      'example.com. IN SOA ns1.example.com. admin.example.com. (',
      '  2024010101 ; serial',
      '  3600 900 604800 300 )',
      'other.net. IN A 192.0.2.9',
      '@ IN HINFO "x" "y"',
      '$INCLUDE other.zone',
      'bare',
      'noval IN A',
      'ok IN A 192.0.2.3',
    ].join('\n'), 'example.com')
    expect(changes).toEqual([{ name: 'ok', type: 'A', changetype: 'REPLACE', records: ['192.0.2.3'] }])
    expect(skipped.map(s => [s.line, s.reason])).toEqual([
      [1, 'the SOA is maintained by the server'],
      [4, 'other.net. is outside example.com'],
      [5, 'HINFO records cannot be imported'],
      [6, '$INCLUDE is not supported'],
      [7, 'no record type'],
      [8, 'missing record data'],
    ])
  })

  it('resolves names against a $ORIGIN below the zone', () => {
    const { changes } = parseZoneFile('$ORIGIN sub.example.com.\n@ A 192.0.2.1\nhost A 192.0.2.2\n', 'example.com')
    expect(changes.map(c => c.name)).toEqual(['sub', 'host.sub'])
  })
})

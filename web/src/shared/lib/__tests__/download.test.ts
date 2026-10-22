import { afterEach, describe, expect, it, vi } from 'vitest'
import { downloadText } from '../download'

describe('downloadText', () => {
  afterEach(() => vi.restoreAllMocks())

  it('clicks a temporary anchor pointing at a blob URL', () => {
    const create = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:x')
    const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => {})
    downloadText('example.com.zone', 'data')
    expect(create).toHaveBeenCalledOnce()
    expect(click).toHaveBeenCalledOnce()
    expect(revoke).toHaveBeenCalledWith('blob:x')
    expect(document.querySelector('a[download]')).toBeNull()
  })
})

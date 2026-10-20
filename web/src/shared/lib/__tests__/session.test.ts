import { beforeEach, describe, expect, it } from 'vitest'
import { useSession } from '../session'

describe('session store', () => {
  beforeEach(() => {
    localStorage.clear()
    sessionStorage.clear()
    useSession.setState({ token: null, adminKey: null })
  })

  it('persists the token in localStorage and the admin key in sessionStorage', () => {
    useSession.getState().signIn('t')
    useSession.getState().setAdminKey('k')
    expect(localStorage.getItem('certo_token')).toBe('t')
    expect(sessionStorage.getItem('certo_admin_key')).toBe('k')
  })

  it('clears everything on sign out', () => {
    useSession.getState().signIn('t')
    useSession.getState().setAdminKey('k')
    useSession.getState().signOut()
    expect(useSession.getState()).toMatchObject({ token: null, adminKey: null })
    expect(localStorage.getItem('certo_token')).toBeNull()
    expect(sessionStorage.getItem('certo_admin_key')).toBeNull()
  })
})

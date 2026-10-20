import { create } from 'zustand'

const TOKEN_KEY = 'certo_token'
const ADMIN_KEY = 'certo_admin_key'

function read(storage: Storage | undefined, key: string): string | null {
  try {
    return storage?.getItem(key) ?? null
  }
  catch {
    return null
  }
}

function write(storage: Storage | undefined, key: string, value: string | null) {
  try {
    if (value === null)
      storage?.removeItem(key)
    else
      storage?.setItem(key, value)
  }
  catch {
    // storage unavailable (private mode): keep the value in memory only
  }
}

const local = typeof localStorage === 'undefined' ? undefined : localStorage
const session = typeof sessionStorage === 'undefined' ? undefined : sessionStorage

export interface SessionState {
  /** Dashboard JWT; `null` when signed out. */
  token: string | null
  /** Admin key for the current browser session; never persisted across sessions. */
  adminKey: string | null
  signIn: (token: string) => void
  signOut: () => void
  setAdminKey: (key: string | null) => void
}

export const useSession = create<SessionState>()(set => ({
  token: read(local, TOKEN_KEY),
  adminKey: read(session, ADMIN_KEY),
  signIn: (token) => {
    write(local, TOKEN_KEY, token)
    set({ token })
  },
  signOut: () => {
    write(local, TOKEN_KEY, null)
    write(session, ADMIN_KEY, null)
    set({ token: null, adminKey: null })
  },
  setAdminKey: (key) => {
    write(session, ADMIN_KEY, key)
    set({ adminKey: key })
  },
}))

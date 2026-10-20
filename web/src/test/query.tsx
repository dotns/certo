import type { ReactNode } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { vi } from 'vitest'

/** A fresh query client wrapper for `renderHook`. */
export function createWrapper() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={client}>{children}</QueryClientProvider>
  return { client, wrapper }
}

export interface FetchCall { method: string, url: string, body: unknown }

/** Stubs `fetch`; `handler` returns the JSON body (or `null` for 204) and the call log collects every request. */
export function stubFetch(handler: (call: FetchCall) => unknown) {
  const calls: FetchCall[] = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init?: RequestInit) => {
    const call = { method: init?.method ?? 'GET', url, body: init?.body ? JSON.parse(String(init.body)) : undefined }
    calls.push(call)
    const result = handler(call)
    return result === null ? new Response(null, { status: 204 }) : new Response(JSON.stringify(result), { status: 200 })
  }))
  return calls
}

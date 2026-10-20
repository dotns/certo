import type { RrSetChange, Scope } from './api'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  addTsigKey,
  createZone,
  deleteTsigKey,
  deleteZone,
  getDnssec,
  getTransfer,
  getZone,
  listRrsets,
  listZones,
  patchRrsets,
  setActivation,
  setDnssec,
  setTransfer,
  verifyZone,
} from './api'

export const zonesKey = (scope: Scope) => ['zones', scope] as const
export const zoneKey = (scope: Scope, zone: string) => ['zones', scope, zone] as const
export const rrsetsKey = (scope: Scope, zone: string) => ['zones', scope, zone, 'rrsets'] as const
export const dnssecKey = (scope: Scope, zone: string) => ['zones', scope, zone, 'dnssec'] as const
export const transferKey = (scope: Scope, zone: string) => ['zones', scope, zone, 'axfr'] as const

export function useZonesQuery(scope: Scope, enabled = true) {
  return useQuery({ queryKey: zonesKey(scope), queryFn: () => listZones(scope), enabled })
}

export function useZoneQuery(scope: Scope, zone: string) {
  return useQuery({ queryKey: zoneKey(scope, zone), queryFn: () => getZone(scope, zone) })
}

export function useRrsetsQuery(scope: Scope, zone: string, enabled: boolean) {
  return useQuery({ queryKey: rrsetsKey(scope, zone), queryFn: () => listRrsets(scope, zone), enabled })
}

export function useDnssecQuery(scope: Scope, zone: string, enabled: boolean) {
  return useQuery({ queryKey: dnssecKey(scope, zone), queryFn: () => getDnssec(scope, zone), enabled })
}

export function useTransferQuery(scope: Scope, zone: string, enabled: boolean) {
  return useQuery({ queryKey: transferKey(scope, zone), queryFn: () => getTransfer(scope, zone), enabled })
}

/** Invalidates every zone list (user and admin views share the data). */
function useInvalidateZones() {
  const client = useQueryClient()
  return () => client.invalidateQueries({ queryKey: ['zones'] })
}

export function useCreateZoneMutation(scope: Scope) {
  const invalidate = useInvalidateZones()
  return useMutation({
    mutationFn: (body: { name: string, user_id?: number }) => createZone(scope, body),
    onSuccess: invalidate,
  })
}

export function useDeleteZoneMutation(scope: Scope) {
  const invalidate = useInvalidateZones()
  return useMutation({
    mutationFn: (zone: string) => deleteZone(scope, zone),
    onSuccess: invalidate,
  })
}

export function useVerifyZoneMutation(scope: Scope) {
  const invalidate = useInvalidateZones()
  return useMutation({
    mutationFn: (zone: string) => verifyZone(scope, zone),
    onSuccess: invalidate,
  })
}

export function useActivationMutation() {
  const invalidate = useInvalidateZones()
  return useMutation({
    mutationFn: ({ zone, exempt }: { zone: string, exempt: boolean }) => setActivation(zone, exempt),
    onSuccess: invalidate,
  })
}

export function usePatchRrsetsMutation(scope: Scope, zone: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (rrsets: RrSetChange[]) => patchRrsets(scope, zone, rrsets),
    onSuccess: (rrsets) => {
      client.setQueryData(rrsetsKey(scope, zone), rrsets)
      return client.invalidateQueries({ queryKey: zoneKey(scope, zone), exact: true })
    },
  })
}

export function useSetDnssecMutation(scope: Scope, zone: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (body: { enabled: boolean, nsec_mode: string }) => setDnssec(scope, zone, body),
    onSuccess: info => client.setQueryData(dnssecKey(scope, zone), info),
  })
}

export function useSetTransferMutation(scope: Scope, zone: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (allowFrom: string[]) => setTransfer(scope, zone, allowFrom),
    onSuccess: info => client.setQueryData(transferKey(scope, zone), info),
  })
}

export function useAddTsigKeyMutation(scope: Scope, zone: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (body: { name: string, algorithm: string }) => addTsigKey(scope, zone, body),
    onSuccess: () => client.invalidateQueries({ queryKey: transferKey(scope, zone) }),
  })
}

export function useDeleteTsigKeyMutation(scope: Scope, zone: string) {
  const client = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => deleteTsigKey(scope, zone, name),
    onSuccess: () => client.invalidateQueries({ queryKey: transferKey(scope, zone) }),
  })
}

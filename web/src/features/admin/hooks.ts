import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useSession } from '@/shared/lib/session'
import { addDomainFor, createUser, deleteUser, listAllDomains, listAllRecords, listUsers, removeDomainEverywhere } from './api'

export const adminUsersKey = ['admin', 'users'] as const
export const adminDomainsKey = ['admin', 'domains'] as const
export const adminRecordsKey = ['admin', 'records'] as const

export function useAdminKey() {
  return useSession(s => s.adminKey)
}

export function useAdminUsersQuery() {
  const key = useAdminKey()
  return useQuery({ queryKey: adminUsersKey, queryFn: listUsers, enabled: !!key })
}

export function useAdminDomainsQuery() {
  const key = useAdminKey()
  return useQuery({ queryKey: adminDomainsKey, queryFn: listAllDomains, enabled: !!key })
}

export function useAdminRecordsQuery() {
  const key = useAdminKey()
  return useQuery({ queryKey: adminRecordsKey, queryFn: listAllRecords, enabled: !!key, refetchInterval: 15_000 })
}

export function useCreateUserMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: createUser,
    onSuccess: () => client.invalidateQueries({ queryKey: adminUsersKey }),
  })
}

export function useDeleteUserMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: deleteUser,
    onSuccess: () => client.invalidateQueries({ queryKey: ['admin'] }).then(() => client.invalidateQueries({ queryKey: ['zones'] })),
  })
}

export function useAddDomainForMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: addDomainFor,
    onSuccess: () => client.invalidateQueries({ queryKey: adminDomainsKey }),
  })
}

export function useRemoveDomainEverywhereMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: removeDomainEverywhere,
    onSuccess: () => client.invalidateQueries({ queryKey: ['admin'] }),
  })
}

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { addDomain, listDomains, listRecords, removeDomain } from './api'

export const domainsKey = ['domains'] as const
export const recordsKey = ['records'] as const

export function useDomainsQuery() {
  return useQuery({ queryKey: domainsKey, queryFn: listDomains })
}

export function useRecordsQuery() {
  return useQuery({ queryKey: recordsKey, queryFn: listRecords, refetchInterval: 15_000 })
}

export function useAddDomainMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: addDomain,
    onSuccess: () => client.invalidateQueries({ queryKey: domainsKey }),
  })
}

export function useRemoveDomainMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: removeDomain,
    onSuccess: () => Promise.all([
      client.invalidateQueries({ queryKey: domainsKey }),
      client.invalidateQueries({ queryKey: recordsKey }),
    ]),
  })
}

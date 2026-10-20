import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { createKey, deleteKey, listKeys } from './api'

export const keysKey = ['keys'] as const

export function useKeysQuery() {
  return useQuery({ queryKey: keysKey, queryFn: listKeys })
}

export function useCreateKeyMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: createKey,
    onSuccess: () => client.invalidateQueries({ queryKey: keysKey }),
  })
}

export function useDeleteKeyMutation() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: deleteKey,
    onSuccess: () => client.invalidateQueries({ queryKey: keysKey }),
  })
}

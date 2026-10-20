import { useQuery } from '@tanstack/react-query'
import { getInfo } from './api'

export const infoKey = ['info'] as const

export function useInfoQuery() {
  return useQuery({ queryKey: infoKey, queryFn: getInfo, staleTime: Infinity })
}

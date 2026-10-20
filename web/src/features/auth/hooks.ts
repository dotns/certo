import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useSession } from '@/shared/lib/session'
import { deleteAccount, getProfile, login, register } from './api'

export const profileKey = ['profile'] as const

export function useProfileQuery() {
  const token = useSession(s => s.token)
  return useQuery({ queryKey: profileKey, queryFn: getProfile, enabled: !!token })
}

export function useLoginMutation() {
  const signIn = useSession(s => s.signIn)
  return useMutation({
    mutationFn: login,
    onSuccess: res => signIn(res.token),
  })
}

export function useRegisterMutation() {
  const signIn = useSession(s => s.signIn)
  return useMutation({
    mutationFn: register,
    onSuccess: res => signIn(res.token),
  })
}

export function useDeleteAccountMutation() {
  const signOut = useSession(s => s.signOut)
  const client = useQueryClient()
  return useMutation({
    mutationFn: deleteAccount,
    onSuccess: () => {
      signOut()
      client.clear()
    },
  })
}

export function useSignOut() {
  const signOut = useSession(s => s.signOut)
  const client = useQueryClient()
  return () => {
    signOut()
    client.clear()
  }
}

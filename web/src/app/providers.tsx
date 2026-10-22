import type { ReactNode } from 'react'
import { QueryClientProvider } from '@tanstack/react-query'
import { Suspense } from 'react'
import { I18nextProvider } from 'react-i18next'
import { Loading } from '@/shared/components/loading'
import { Toaster } from '@/shared/components/ui/sonner'
import { TooltipProvider } from '@/shared/components/ui/tooltip'
import { queryClient } from '@/shared/lib/query-client'
import { ThemeProvider } from '@/shared/lib/theme'
import i18n from './i18n'

export function Providers({ children }: { children: ReactNode }) {
  return (
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider>
          <TooltipProvider>
            <Suspense fallback={<div className="flex min-h-svh items-center justify-center"><Loading label="" /></div>}>
              {children}
            </Suspense>
            <Toaster position="bottom-right" />
          </TooltipProvider>
        </ThemeProvider>
      </QueryClientProvider>
    </I18nextProvider>
  )
}

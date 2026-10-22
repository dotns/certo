import { SearchIcon } from 'lucide-react'
import { Input } from '@/shared/components/ui/input'

interface SearchInputProps {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  label?: string
}

/** A compact text filter for tables. */
export function SearchInput({ value, onChange, placeholder = 'Filter…', label = 'Filter' }: SearchInputProps) {
  return (
    <div className="relative w-full max-w-xs">
      <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
      <Input type="search" aria-label={label} placeholder={placeholder} value={value} onChange={e => onChange(e.target.value)} className="pl-8" />
    </div>
  )
}

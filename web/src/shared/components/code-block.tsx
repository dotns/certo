import { CopyButton } from '@/shared/components/copy-button'

interface CodeBlockProps {
  title?: string
  code: string
  wrap?: boolean
}

/** Monospace block with a copy button. */
export function CodeBlock({ title, code, wrap = false }: CodeBlockProps) {
  return (
    <div className="flex flex-col gap-1">
      {title && (
        <div className="flex items-center justify-between gap-2 text-sm font-medium">
          <span>{title}</span>
          <CopyButton text={code} />
        </div>
      )}
      <pre className={`overflow-x-auto rounded-lg bg-muted px-3 py-2 font-mono text-xs leading-relaxed ${wrap ? 'whitespace-pre-wrap break-all' : ''}`}>
        {code}
      </pre>
    </div>
  )
}

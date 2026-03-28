import { RefreshCw } from 'lucide-react'

import { formatBtc, formatTime } from '@/lib/format'

interface HeaderProps {
  btcPrice: number | null
  lastUpdated: Date | null
}

export function Header({ btcPrice, lastUpdated }: HeaderProps) {
  return (
    <header className="hero-gradient border-b border-[var(--border)]">
      <div className="mx-auto flex w-full max-w-7xl items-center justify-between px-6 py-5">
        <div className="flex items-center gap-4">
          <div className="text-2xl font-bold tracking-tight">
            <span className="glow-accent text-[var(--accent)]">◆</span> Oracel
          </div>

          <div className="flex items-center gap-2">
            <div className="pulse-dot" />
            <span className="mono text-xs text-[var(--text-secondary)]">LIVE</span>
          </div>
        </div>

        <div className="flex items-center gap-6">
          <div className="text-right">
            <div className="text-xs text-[var(--text-secondary)]">BTC/USDT</div>
            <div className="mono font-semibold text-[var(--text-primary)]">{formatBtc(btcPrice)}</div>
          </div>

          <div className="text-right">
            <div className="text-xs text-[var(--text-secondary)]">Last Update</div>
            <div className="mono flex items-center justify-end gap-1 text-sm text-[var(--text-secondary)]">
              <RefreshCw className="size-3" />
              {formatTime(lastUpdated)}
            </div>
          </div>
        </div>
      </div>
    </header>
  )
}

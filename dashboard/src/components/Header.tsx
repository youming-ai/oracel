import { Activity, Clock, Radio, Zap } from 'lucide-react'

import { formatBtc, formatTime } from '@/lib/format'

declare const __BOT_MODE__: string | undefined

interface HeaderProps {
  btcPrice: number | null
  lastUpdated: Date | null
}

export function Header({ btcPrice, lastUpdated }: HeaderProps) {
  return (
    <header className="header-hud relative overflow-hidden">
      {/* Scanline overlay */}
      <div className="header-scanline" />

      {/* Glowing bottom edge */}
      <div className="header-glow-edge" />

      <div className="relative z-10 mx-auto flex w-full max-w-7xl flex-col gap-3 px-4 py-3 sm:flex-row sm:items-center sm:justify-between sm:px-6 sm:py-4">
        {/* Left: Brand cluster */}
        <div className="flex items-center gap-3 sm:gap-5">
          {/* Logo mark */}
          <div className="header-logo-mark">
            <Zap className="size-4 text-[var(--bg-primary)]" strokeWidth={2.5} />
          </div>

          <div>
            <div className="header-brand">ORACEL</div>
            <div className="mt-0.5 flex items-center gap-2">
              <div className="header-status-chip">
                <Radio className="size-2.5 animate-pulse" />
                <span>{(typeof __BOT_MODE__ === 'string' ? __BOT_MODE__ : 'paper').toUpperCase()}</span>
              </div>
              <div className="header-divider hidden sm:block" />
              <span className="mono hidden text-[10px] tracking-widest text-[var(--text-dim)] sm:inline">
                POLYMARKET BOT v1
              </span>
            </div>
          </div>
        </div>

        {/* Right: Data strip */}
        <div className="flex items-center gap-1">
          {/* BTC Price module */}
          <div className="header-data-module flex-1 sm:flex-none">
            <div className="header-data-label">
              <Activity className="size-2.5" />
              BTC / USDT
            </div>
            <div className="header-data-value text-[var(--text-primary)]">
              {formatBtc(btcPrice)}
            </div>
          </div>

          <div className="header-module-sep" />

          {/* Sync module */}
          <div className="header-data-module flex-1 sm:flex-none">
            <div className="header-data-label">
              <Clock className="size-2.5" />
              LAST SYNC
            </div>
            <div className="header-data-value text-[var(--text-secondary)]">
              {formatTime(lastUpdated)}
            </div>
          </div>
        </div>
      </div>
    </header>
  )
}

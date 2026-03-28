import { Card, CardContent } from '@/components/ui/card'
import type { DashboardStats } from '@/lib/dashboard-types'
import { formatCurrency, formatPercent } from '@/lib/format'

interface StatsCardsProps {
  stats: DashboardStats
  balance: number
  lineCount: number
}

export function StatsCards({ stats, balance, lineCount }: StatsCardsProps) {
  const totalTrades = stats.settlements.length

  return (
    <div className="grid grid-cols-2 gap-4 md:grid-cols-4 lg:grid-cols-6">
      <Card className="glass stat-card gap-0 border-0 py-0 text-[var(--text-primary)] ring-0">
        <CardContent className="p-4">
          <div className="mb-1 text-xs text-[var(--text-secondary)]">Balance</div>
          <div className="mono text-xl font-bold text-[var(--accent)]">{formatCurrency(balance)}</div>
          <div
            className="mono mt-1 text-xs"
            style={{ color: stats.totalPnl >= 0 ? 'var(--win)' : 'var(--loss)' }}
          >
            {stats.totalPnl >= 0 ? '+' : '-'}{formatCurrency(Math.abs(stats.totalPnl))} total
          </div>
        </CardContent>
      </Card>

      <Card className="glass stat-card gap-0 border-0 py-0 text-[var(--text-primary)] ring-0">
        <CardContent className="p-4">
          <div className="mb-1 text-xs text-[var(--text-secondary)]">Win Rate</div>
          <div
            className="mono text-xl font-bold"
            style={{
              color: stats.winRate >= 15 ? 'var(--win)' : stats.winRate >= 10 ? 'var(--warn)' : 'var(--loss)',
            }}
          >
            {formatPercent(stats.winRate)}
          </div>
          <div className="mono mt-1 text-xs text-[var(--text-dim)]">
            {stats.wins.length}W / {stats.losses.length}L
          </div>
        </CardContent>
      </Card>

      <Card className="glass stat-card gap-0 border-0 py-0 text-[var(--text-primary)] ring-0">
        <CardContent className="p-4">
          <div className="mb-1 text-xs text-[var(--text-secondary)]">Total Trades</div>
          <div className="mono text-xl font-bold">{totalTrades}</div>
          <div className="mono mt-1 text-xs text-[var(--text-dim)]">of {lineCount} lines</div>
        </CardContent>
      </Card>

      <Card className="glass stat-card gap-0 border-0 py-0 text-[var(--text-primary)] ring-0">
        <CardContent className="p-4">
          <div className="mb-1 text-xs text-[var(--text-secondary)]">Profit Factor</div>
          <div
            className="mono text-xl font-bold"
            style={{
              color:
                Number.isFinite(stats.profitFactor) && stats.profitFactor > 1 ? 'var(--win)' : 'var(--loss)',
            }}
          >
            {Number.isFinite(stats.profitFactor) ? stats.profitFactor.toFixed(2) : '∞'}
          </div>
          <div className="mono mt-1 text-xs text-[var(--text-dim)]">
            W:{stats.winPnl.toFixed(0)} L:{stats.lossPnl.toFixed(0)}
          </div>
        </CardContent>
      </Card>

      <Card className="glass stat-card gap-0 border-0 py-0 text-[var(--text-primary)] ring-0">
        <CardContent className="p-4">
          <div className="mb-1 text-xs text-[var(--text-secondary)]">Streak</div>
          <div
            className="mono text-xl font-bold"
            style={{ color: stats.streak >= 0 ? 'var(--win)' : 'var(--loss)' }}
          >
            {stats.streak >= 0 ? '+' : ''}
            {stats.streak}
          </div>
          <div className="mono mt-1 text-xs text-[var(--text-dim)]">
            Max W:{stats.maxWinStreak} L:{stats.maxLossStreak}
          </div>
        </CardContent>
      </Card>

      <Card className="glass stat-card gap-0 border-0 py-0 text-[var(--text-primary)] ring-0">
        <CardContent className="p-4">
          <div className="mb-1 text-xs text-[var(--text-secondary)]">Max Drawdown</div>
          <div className="mono text-xl font-bold text-[var(--loss)]">
            -{formatCurrency(stats.maxDrawdown)}
          </div>
          <div className="mono mt-1 text-xs text-[var(--text-dim)]">from peak equity</div>
        </CardContent>
      </Card>
    </div>
  )
}

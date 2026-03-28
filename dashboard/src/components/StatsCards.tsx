import {
  ArrowDownRight,
  ArrowUpRight,
  BarChart3,
  Flame,
  Scale,
  Target,
  TrendingUp,
  Wallet,
} from 'lucide-react'

import { Card, CardContent } from '@/components/ui/card'
import type { DashboardStats } from '@/lib/dashboard-types'
import { formatCurrency, formatPercent } from '@/lib/format'

interface StatsCardsProps {
  stats: DashboardStats
  balance: number
  lineCount: number
}

interface StatDef {
  label: string
  icon: React.ReactNode
  value: string
  sub: string
  color?: string
}

export function StatsCards({ stats, balance, lineCount }: StatsCardsProps) {
  const totalTrades = stats.settlements.length

  const winRateColor =
    stats.winRate >= 15 ? 'var(--win)' : stats.winRate >= 10 ? 'var(--warn)' : 'var(--loss)'
  const profitFactorColor =
    Number.isFinite(stats.profitFactor) && stats.profitFactor > 1 ? 'var(--win)' : 'var(--loss)'
  const streakColor = stats.streak >= 0 ? 'var(--win)' : 'var(--loss)'

  const cards: StatDef[] = [
    {
      label: 'Balance',
      icon: <Wallet className="size-3.5" />,
      value: formatCurrency(balance),
      sub: `${stats.totalPnl >= 0 ? '+' : '-'}${formatCurrency(Math.abs(stats.totalPnl))} total`,
      color: 'var(--accent)',
    },
    {
      label: 'Win Rate',
      icon: <Target className="size-3.5" />,
      value: formatPercent(stats.winRate),
      sub: `${stats.wins.length}W / ${stats.losses.length}L`,
      color: winRateColor,
    },
    {
      label: 'Total Trades',
      icon: <BarChart3 className="size-3.5" />,
      value: `${totalTrades}`,
      sub: `of ${lineCount} lines`,
    },
    {
      label: 'Profit Factor',
      icon: <Scale className="size-3.5" />,
      value: Number.isFinite(stats.profitFactor) ? stats.profitFactor.toFixed(2) : '\u221e',
      sub: `W:${stats.winPnl.toFixed(0)} L:${stats.lossPnl.toFixed(0)}`,
      color: profitFactorColor,
    },
    {
      label: 'Streak',
      icon: stats.streak >= 0 ? <TrendingUp className="size-3.5" /> : <Flame className="size-3.5" />,
      value: `${stats.streak >= 0 ? '+' : ''}${stats.streak}`,
      sub: `Max W:${stats.maxWinStreak} L:${stats.maxLossStreak}`,
      color: streakColor,
    },
    {
      label: 'Max Drawdown',
      icon: <ArrowDownRight className="size-3.5" />,
      value: `-${formatCurrency(stats.maxDrawdown)}`,
      sub: 'from peak equity',
      color: 'var(--loss)',
    },
  ]

  return (
    <div className="grid grid-cols-2 gap-2.5 sm:gap-3 md:grid-cols-3 lg:grid-cols-6">
      {cards.map((card) => (
        <StatCard key={card.label} {...card} />
      ))}
    </div>
  )
}

function StatCard({ label, icon, value, sub, color }: StatDef) {
  const pnlPositive = sub.startsWith('+')
  const pnlNegative = sub.startsWith('-') && label === 'Balance'

  return (
    <Card className="hud-card group gap-0 border-0 py-0 text-[var(--text-primary)] ring-0">
      <CardContent className="p-3 sm:p-4">
        <div className="mb-2 flex items-center gap-1.5">
          <span className="text-[var(--text-dim)]">{icon}</span>
          <span className="text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)]">
            {label}
          </span>
        </div>
        <div
          className="display-font text-lg font-bold leading-none sm:text-xl"
          style={{ color: color ?? 'var(--text-primary)' }}
        >
          {value}
        </div>
        <div
          className="mono mt-1.5 text-[10px] sm:text-xs"
          style={{
            color: pnlPositive ? 'var(--win)' : pnlNegative ? 'var(--loss)' : 'var(--text-dim)',
          }}
        >
          {pnlPositive && <ArrowUpRight className="mr-0.5 inline size-2.5" />}
          {pnlNegative && <ArrowDownRight className="mr-0.5 inline size-2.5" />}
          {sub}
        </div>
      </CardContent>
    </Card>
  )
}

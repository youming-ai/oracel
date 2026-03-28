import { Cell, Pie, PieChart, ResponsiveContainer } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { DashboardStats } from '@/lib/dashboard-types'
import { formatCurrency, formatPercent } from '@/lib/format'
import { Gauge } from 'lucide-react'

interface PerformanceCardProps {
  stats: DashboardStats
}

function getWinRateColor(winRate: number): string {
  if (winRate >= 15) return 'var(--win)'
  if (winRate >= 10) return 'var(--warn)'
  return 'var(--loss)'
}

export function PerformanceCard({ stats }: PerformanceCardProps) {
  const totalTrades = stats.settlements.length
  const winRateColor = getWinRateColor(stats.winRate)

  return (
    <Card className="hud-card gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-4 pt-4 pb-3 sm:px-5 sm:pt-5 sm:pb-4">
        <CardTitle className="card-title-hud">
          <Gauge className="size-3.5 text-[var(--accent)]" />
          Performance
        </CardTitle>
      </CardHeader>

      <CardContent className="px-4 pb-4 sm:px-5 sm:pb-5">
        <div className="flex flex-col gap-5 sm:flex-row sm:items-center sm:gap-6 lg:flex-col lg:gap-5 xl:flex-row xl:gap-8">
          <div className="relative mx-auto aspect-square w-36 shrink-0 sm:mx-0 sm:w-40">
            <ResponsiveContainer width="100%" height="100%">
              <PieChart>
                <Pie
                  data={[
                    { name: 'Wins', value: stats.wins.length },
                    { name: 'Losses', value: stats.losses.length },
                  ]}
                  dataKey="value"
                  innerRadius="58%"
                  outerRadius="95%"
                  stroke="none"
                  isAnimationActive={false}
                >
                  <Cell fill="var(--win)" />
                  <Cell fill="var(--loss)" />
                </Pie>
              </PieChart>
            </ResponsiveContainer>

            <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
              <div className="display-font text-xl font-bold sm:text-2xl" style={{ color: winRateColor }}>
                {formatPercent(stats.winRate)}
              </div>
              <div className="text-[10px] uppercase tracking-wider text-[var(--text-dim)]">Win Rate</div>
            </div>
          </div>

          <div className="w-full space-y-2.5">
            <Metric label="Total Trades" value={`${totalTrades}`} />
            <Metric label="Wins" value={`${stats.wins.length}`} labelColor="var(--win)" valueColor="var(--win)" />
            <Metric
              label="Losses"
              value={`${stats.losses.length}`}
              labelColor="var(--loss)"
              valueColor="var(--loss)"
            />
            <div className="my-1 h-px bg-[var(--border)] opacity-50" />
            <Metric label="Avg Win" value={formatCurrency(stats.avgWin)} valueColor="var(--win)" />
            <Metric label="Avg Loss" value={`-${formatCurrency(stats.avgLoss)}`} valueColor="var(--loss)" />
            <Metric
              label="Profit Factor"
              value={Number.isFinite(stats.profitFactor) ? stats.profitFactor.toFixed(2) : '\u221e'}
              valueColor={
                Number.isFinite(stats.profitFactor) && stats.profitFactor > 1 ? 'var(--win)' : 'var(--loss)'
              }
            />
            <Metric label="Max Drawdown" value={`-${formatCurrency(stats.maxDrawdown)}`} valueColor="var(--loss)" />
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

interface MetricProps {
  label: string
  value: string
  labelColor?: string
  valueColor?: string
}

function Metric({ label, value, labelColor, valueColor }: MetricProps) {
  return (
    <div className="flex items-center justify-between text-xs">
      <span style={{ color: labelColor ?? 'var(--text-secondary)' }}>{label}</span>
      <span className="mono font-semibold" style={{ color: valueColor ?? 'var(--text-primary)' }}>
        {value}
      </span>
    </div>
  )
}

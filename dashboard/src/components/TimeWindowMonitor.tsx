import { Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { TimeWindowStats } from '@/lib/dashboard-types'
import { formatCurrency, formatPercent } from '@/lib/format'
import { Timer } from 'lucide-react'

interface TimeWindowMonitorProps {
  data: TimeWindowStats[]
}

function colorForRate(winRate: number): string {
  if (winRate >= 15) return 'rgba(0,212,170,0.7)'
  if (winRate >= 10) return 'rgba(255,165,2,0.7)'
  return 'rgba(255,71,87,0.7)'
}

function pnlColor(pnl: number): string {
  if (pnl > 0) return 'var(--win)'
  if (pnl < 0) return 'var(--loss)'
  return 'var(--text-dim)'
}

export function TimeWindowMonitor({ data }: TimeWindowMonitorProps) {
  if (data.length === 0) return null

  const chartData = data.map((w) => ({
    label: w.window.label,
    winRate: w.winRate,
  }))

  return (
    <Card className="hud-card gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-4 pt-4 pb-3 sm:px-5 sm:pt-5 sm:pb-4">
        <CardTitle className="card-title-hud">
          <Timer className="size-3.5 text-[var(--accent)]" />
          Time Window Monitor
        </CardTitle>
      </CardHeader>

      <CardContent className="px-4 pb-4 sm:px-5 sm:pb-5">
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          {/* Win Rate by Window */}
          <div>
            <div className="mb-2 text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)]">
              Win Rate by Window
            </div>
            <div className="h-[180px]">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart data={chartData} margin={{ top: 6, right: 4, left: -14, bottom: 0 }}>
                  <CartesianGrid stroke="rgba(30,45,61,0.3)" vertical={false} />
                  <XAxis
                    dataKey="label"
                    axisLine={false}
                    tickLine={false}
                    tick={{ fill: 'var(--text-secondary)', fontFamily: '"Geist Pixel", monospace', fontSize: 9 }}
                  />
                  <YAxis
                    domain={[0, 100]}
                    axisLine={false}
                    tickLine={false}
                    tickFormatter={(value) => `${value}%`}
                    tick={{ fill: 'var(--text-secondary)', fontFamily: '"Geist Pixel", monospace', fontSize: 10 }}
                  />
                  <Tooltip
                    formatter={(value) => `${Number(value).toFixed(1)}%`}
                    contentStyle={{
                      background: 'rgba(10,14,23,0.95)',
                      border: '1px solid rgba(0,212,170,0.2)',
                      borderRadius: '6px',
                      color: 'var(--text-primary)',
                      fontFamily: '"Geist Pixel", monospace',
                      fontSize: '11px',
                      boxShadow: '0 4px 12px rgba(0,0,0,0.4)',
                    }}
                    labelStyle={{ color: 'var(--text-secondary)' }}
                  />
                  <Bar dataKey="winRate" radius={[4, 4, 0, 0]}>
                    {chartData.map((item) => (
                      <Cell key={item.label} fill={colorForRate(item.winRate)} />
                    ))}
                  </Bar>
                </BarChart>
              </ResponsiveContainer>
            </div>
          </div>

          {/* Stats Table */}
          <div>
            <div className="mb-2 text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)]">
              Window Statistics
            </div>
            <div className="space-y-2">
              {data.map((w) => (
                <div
                  key={w.window.label}
                  className="rounded-lg border border-[var(--border-a15)] bg-[var(--accent-a3)] px-3 py-2.5"
                >
                  <div className="mb-1.5 flex items-center justify-between">
                    <span className="mono text-xs font-medium text-[var(--text-primary)]">
                      {w.window.label}
                    </span>
                    <span
                      className="mono text-xs font-semibold"
                      style={{ color: pnlColor(w.pnl) }}
                    >
                      {w.pnl >= 0 ? '+' : ''}{formatCurrency(w.pnl)}
                    </span>
                  </div>
                  <div className="flex items-center gap-3 text-[10px]">
                    <span className="text-[var(--text-secondary)]">
                      {w.trades} trades
                    </span>
                    <span style={{ color: 'var(--win)' }}>
                      {w.wins}W
                    </span>
                    <span style={{ color: 'var(--loss)' }}>
                      {w.losses}L
                    </span>
                    <span
                      className="mono font-semibold"
                      style={{ color: colorForRate(w.winRate) }}
                    >
                      {formatPercent(w.winRate)}
                    </span>
                    <span className="text-[var(--text-dim)]">
                      avg {formatCurrency(Math.abs(w.avgPnl))}
                    </span>
                  </div>
                  {/* Mini progress bar for win rate */}
                  <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-[rgba(30,45,61,0.5)]">
                    <div
                      className="h-full rounded-full transition-all duration-500"
                      style={{
                        width: `${Math.min(w.winRate, 100)}%`,
                        background: colorForRate(w.winRate),
                      }}
                    />
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

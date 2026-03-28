import { Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { HourlyStat } from '@/lib/dashboard-types'

interface HourlyChartProps {
  data: HourlyStat[]
}

function colorForRate(winRate: number): string {
  if (winRate >= 15) {
    return 'rgba(0,212,170,0.7)'
  }

  if (winRate >= 10) {
    return 'rgba(255,165,2,0.7)'
  }

  return 'rgba(255,71,87,0.7)'
}

export function HourlyChart({ data }: HourlyChartProps) {
  return (
    <Card className="glass gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-5 pt-5 pb-4">
        <CardTitle className="text-sm font-semibold text-[var(--text-secondary)]">Hourly Win Rate</CardTitle>
      </CardHeader>

      <CardContent className="h-[240px] px-4 pb-4 sm:px-5 sm:pb-5">
        {data.length === 0 ? (
          <div className="mono flex h-full items-center justify-center text-sm text-[var(--text-dim)]">
            No settled trades yet
          </div>
        ) : (
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={data} margin={{ top: 6, right: 4, left: -14, bottom: 0 }}>
              <CartesianGrid stroke="rgba(30,45,61,0.3)" vertical={false} />
              <XAxis
                dataKey="hour"
                axisLine={false}
                tickLine={false}
                tick={{ fill: 'var(--text-secondary)', fontFamily: '"Geist Pixel", monospace', fontSize: 10 }}
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
                  background: 'rgba(17,24,39,0.95)',
                  border: '1px solid var(--border)',
                  borderRadius: '8px',
                  color: 'var(--text-primary)',
                  fontFamily: '"Geist Pixel", monospace',
                  fontSize: '11px',
                }}
                labelStyle={{ color: 'var(--text-secondary)' }}
              />
              <Bar dataKey="winRate" radius={[4, 4, 0, 0]}>
                {data.map((item) => (
                  <Cell key={item.hour} fill={colorForRate(item.winRate)} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        )}
      </CardContent>
    </Card>
  )
}

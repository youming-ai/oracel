import { Bar, BarChart, CartesianGrid, Legend, ResponsiveContainer, Tooltip, XAxis, YAxis } from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import type { DirectionStats } from '@/lib/dashboard-types'

interface DirectionChartProps {
  data: DirectionStats[]
}

export function DirectionChart({ data }: DirectionChartProps) {
  return (
    <Card className="glass gap-0 border-0 py-0 ring-0">
      <CardHeader className="border-0 px-5 pt-5 pb-4">
        <CardTitle className="text-sm font-semibold text-[var(--text-secondary)]">
          Direction Breakdown
        </CardTitle>
      </CardHeader>

      <CardContent className="h-[240px] px-4 pb-4 sm:px-5 sm:pb-5">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={data} barGap={6} margin={{ top: 6, right: 4, left: -14, bottom: 0 }}>
            <CartesianGrid stroke="rgba(30,45,61,0.3)" vertical={false} />
            <XAxis
              dataKey="direction"
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-secondary)', fontFamily: 'JetBrains Mono', fontSize: 11 }}
            />
            <YAxis
              allowDecimals={false}
              axisLine={false}
              tickLine={false}
              tick={{ fill: 'var(--text-secondary)', fontFamily: 'JetBrains Mono', fontSize: 10 }}
            />
            <Tooltip
              contentStyle={{
                background: 'rgba(17,24,39,0.95)',
                border: '1px solid var(--border)',
                borderRadius: '8px',
                color: 'var(--text-primary)',
                fontFamily: 'JetBrains Mono, monospace',
                fontSize: '11px',
              }}
              labelStyle={{ color: 'var(--text-secondary)' }}
            />
            <Legend
              iconType="rect"
              wrapperStyle={{
                color: 'var(--text-secondary)',
                fontFamily: 'JetBrains Mono, monospace',
                fontSize: '10px',
              }}
            />
            <Bar dataKey="wins" name="Wins" fill="rgba(0,212,170,0.7)" radius={[4, 4, 0, 0]} />
            <Bar dataKey="losses" name="Losses" fill="rgba(255,71,87,0.7)" radius={[4, 4, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}

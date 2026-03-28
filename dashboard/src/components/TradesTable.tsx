import { useEffect, useMemo, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import type { TradeEntry, TradeRecord, TradeSettlement } from '@/lib/dashboard-types'
import { formatBtc, formatCurrency } from '@/lib/format'
import { CheckCircle, Clock, TrendingDown, TrendingUp, XCircle } from 'lucide-react'

function useNow(intervalMs = 1000): Date {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), intervalMs)
    return () => clearInterval(id)
  }, [intervalMs])
  return now
}

/** Parse a time string into a Date. Supports ISO 8601 and legacy HH:MM:SS. */
function parseTradeTime(timeStr: string): Date {
  // ISO format: 2025-03-28T14:30:00Z or similar
  if (timeStr.includes('T') || timeStr.includes('-')) {
    const d = new Date(timeStr)
    if (!Number.isNaN(d.getTime())) return d
  }

  // Legacy HH:MM:SS — assume today UTC
  const parts = timeStr.split(':').map(Number)
  const [h = 0, m = 0, s = 0] = parts
  if (parts.some(Number.isNaN) || h < 0 || h > 23 || m < 0 || m > 59 || s < 0 || s > 59) {
    return new Date()
  }
  const d = new Date()
  d.setUTCHours(h, m, s, 0)
  return d
}

function relativeTime(timeStr: string, now: Date): string {
  const then = parseTradeTime(timeStr)
  const diffSec = Math.max(0, Math.floor((now.getTime() - then.getTime()) / 1000))
  if (diffSec < 60) return `${diffSec}s ago`
  const diffMin = Math.floor(diffSec / 60)
  if (diffMin < 60) return `${diffMin}m ago`
  const diffHr = Math.floor(diffMin / 60)
  if (diffHr < 24) return `${diffHr}h ago`
  const diffDay = Math.floor(diffHr / 24)
  return `${diffDay}d ago`
}

type TradeFilter = 'all' | 'WIN' | 'LOSS' | 'pending'

interface TradesTableProps {
  trades: TradeRecord[]
  pendingTrades: TradeEntry[]
}

function isSettlement(trade: TradeRecord): trade is TradeSettlement {
  return trade.type === 'settlement'
}

function isEntry(trade: TradeRecord): trade is TradeEntry {
  return trade.type === 'entry'
}

const PAGE_SIZE = 20

export function TradesTable({ trades, pendingTrades }: TradesTableProps) {
  const [filter, setFilter] = useState<TradeFilter>('all')
  const [page, setPage] = useState(0)
  const now = useNow()

  const settlements = useMemo(
    () => trades.filter(isSettlement),
    [trades],
  )

  const filteredTrades = useMemo(() => {
    if (filter === 'WIN') {
      return settlements.filter((trade) => trade.result === 'WIN').reverse()
    }

    if (filter === 'LOSS') {
      return settlements.filter((trade) => trade.result === 'LOSS').reverse()
    }

    if (filter === 'pending') {
      return [...pendingTrades].reverse()
    }

    return [...pendingTrades, ...settlements].reverse()
  }, [filter, pendingTrades, settlements])

  const totalPages = Math.max(1, Math.ceil(filteredTrades.length / PAGE_SIZE))
  const safePage = Math.min(page, totalPages - 1)
  const pagedTrades = filteredTrades.slice(
    safePage * PAGE_SIZE,
    safePage * PAGE_SIZE + PAGE_SIZE,
  )

  function changeFilter(value: TradeFilter) {
    setFilter(value)
    setPage(0)
  }

  return (
    <Card className="glass gap-0 border-0 py-0 ring-0">
      <CardHeader className="flex flex-col justify-between gap-4 border-0 px-5 pt-5 pb-4 md:flex-row md:items-center">
        <CardTitle className="text-sm font-semibold text-[var(--text-secondary)]">Recent Trades</CardTitle>

        <div className="flex flex-wrap gap-2">
          <FilterButton label="All" value="all" activeFilter={filter} onClick={changeFilter} />
          <FilterButton label="Wins" value="WIN" activeFilter={filter} onClick={changeFilter} />
          <FilterButton label="Losses" value="LOSS" activeFilter={filter} onClick={changeFilter} />
          <FilterButton label="Pending" value="pending" activeFilter={filter} onClick={changeFilter} />
        </div>
      </CardHeader>

      <CardContent className="px-0 pb-0">
        <Table className="mono text-xs">
          <TableHeader>
            <TableRow className="border-b border-[rgba(30,45,61,0.5)] hover:bg-transparent">
              <TableHead className="h-9 px-3 text-[var(--text-dim)]">Time</TableHead>
              <TableHead className="h-9 px-3 text-[var(--text-dim)]">Status</TableHead>
              <TableHead className="h-9 px-3 text-[var(--text-dim)]">Direction</TableHead>
              <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">PnL</TableHead>
              <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Entry BTC</TableHead>
              <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Exit BTC</TableHead>
              <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Price</TableHead>
              <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Edge</TableHead>
              <TableHead className="h-9 px-3 text-right text-[var(--text-dim)]">Payoff</TableHead>
            </TableRow>
          </TableHeader>

            <TableBody>
              {pagedTrades.map((trade) => (
                <TableRow key={buildTradeKey(trade)} className="trade-row border-b-0">
                  <TableCell className="px-3 py-2 text-[var(--text-secondary)]" title={trade.time}>
                    {relativeTime(trade.time, now)}
                  </TableCell>
                  <TableCell className="px-3 py-2">{renderStatusBadge(trade)}</TableCell>
                  <TableCell className="px-3 py-2">{renderDirectionBadge(trade.direction)}</TableCell>
                  <TableCell className="px-3 py-2 text-right">{renderPnl(trade)}</TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-secondary)]">
                    {isSettlement(trade) ? formatBtc(trade.entryBTC) : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-secondary)]">
                    {isSettlement(trade) ? formatBtc(trade.exitBTC) : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-dim)]">
                    {trade.price !== null ? trade.price.toFixed(3) : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-dim)]">
                    {trade.edge !== null ? `${trade.edge.toFixed(1)}%` : '—'}
                  </TableCell>
                  <TableCell className="px-3 py-2 text-right text-[var(--text-dim)]">
                    {trade.payoff ?? '—'}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
        </Table>

        {totalPages > 1 && (
          <div className="flex items-center justify-between border-t border-[rgba(30,45,61,0.5)] px-4 py-2">
            <span className="mono text-xs text-[var(--text-dim)]">
              {filteredTrades.length} trades · Page {safePage + 1} of {totalPages}
            </span>
            <div className="flex gap-1">
              <Button
                size="xs"
                variant="ghost"
                className="mono rounded-md px-3 py-1 text-xs"
                disabled={safePage === 0}
                style={{ color: 'var(--text-dim)' }}
                onClick={() => setPage(safePage - 1)}
              >
                ← Prev
              </Button>
              <Button
                size="xs"
                variant="ghost"
                className="mono rounded-md px-3 py-1 text-xs"
                disabled={safePage >= totalPages - 1}
                style={{ color: 'var(--text-dim)' }}
                onClick={() => setPage(safePage + 1)}
              >
                Next →
              </Button>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

interface FilterButtonProps {
  label: string
  value: TradeFilter
  activeFilter: TradeFilter
  onClick: (value: TradeFilter) => void
}

function FilterButton({ label, value, activeFilter, onClick }: FilterButtonProps) {
  const active = value === activeFilter

  return (
    <Button
      size="xs"
      variant="ghost"
      className="mono rounded-md px-3 py-1 text-xs"
      style={{
        color: active ? 'var(--accent)' : 'var(--text-dim)',
        background: active ? 'var(--accent-dim)' : 'transparent',
      }}
      onClick={() => onClick(value)}
    >
      {label}
    </Button>
  )
}

function renderStatusBadge(trade: TradeRecord) {
  if (isEntry(trade)) {
    return (
      <Badge className="inline-flex items-center gap-1 rounded-md border-0 bg-[rgba(255,165,2,0.15)] px-2 py-0.5 text-[11px] text-[var(--warn)]">
        <Clock className="size-3" />
        PENDING
      </Badge>
    )
  }

  const win = trade.result === 'WIN'
  return (
    <Badge
      className="inline-flex items-center gap-1 rounded-md border-0 px-2 py-0.5 text-[11px]"
      style={{
        background: win ? 'rgba(0,212,170,0.15)' : 'rgba(255,71,87,0.15)',
        color: win ? 'var(--win)' : 'var(--loss)',
      }}
    >
      {win ? <CheckCircle className="size-3" /> : <XCircle className="size-3" />}
      {win ? 'WIN' : 'LOSS'}
    </Badge>
  )
}

function renderDirectionBadge(direction: 'UP' | 'DOWN') {
  const up = direction === 'UP'
  return (
    <Badge
      className="inline-flex items-center gap-1 rounded-md border-0 px-2 py-0.5 text-[11px]"
      style={{
        background: up ? 'rgba(0,212,170,0.15)' : 'rgba(255,71,87,0.15)',
        color: up ? 'var(--win)' : 'var(--loss)',
      }}
    >
      {up ? <TrendingUp className="size-3" /> : <TrendingDown className="size-3" />}
      {direction}
    </Badge>
  )
}

function renderPnl(trade: TradeRecord) {
  if (isEntry(trade)) {
    return <span className="text-[var(--text-dim)]">—</span>
  }

  const win = trade.result === 'WIN'
  return (
    <span className="font-semibold" style={{ color: win ? 'var(--win)' : 'var(--loss)' }}>
      {win ? '+' : '-'}{formatCurrency(Math.abs(trade.pnl))}
    </span>
  )
}

function buildTradeKey(trade: TradeRecord): string {
  if (isEntry(trade)) {
    return `entry-${trade.time}-${trade.direction}-${trade.conditionId}`
  }

  return `settlement-${trade.time}-${trade.direction}-${trade.result}-${trade.pnl}`
}

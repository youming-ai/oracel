import { useMemo, useState } from 'react'

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
import { CheckCircle, ChevronLeft, ChevronRight, Clock, List, TrendingDown, TrendingUp, XCircle } from 'lucide-react'

function formatTradeTime(timeStr: string): string {
  // ISO 8601 (e.g. "2026-03-29T14:11:45Z") — convert to local time
  if (timeStr.includes('T') || timeStr.includes('-')) {
    const d = new Date(timeStr)
    if (!Number.isNaN(d.getTime())) {
      const pad = (n: number) => String(n).padStart(2, '0')
      return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
    }
  }
  // Legacy HH:MM:SS — no date info, show as-is
  return timeStr
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
    <Card className="hud-card gap-0 border-0 py-0 ring-0">
      <CardHeader className="flex flex-col justify-between gap-3 border-0 px-4 pt-4 pb-3 sm:flex-row sm:items-center sm:px-5 sm:pt-5 sm:pb-4">
        <CardTitle className="card-title-hud">
          <List className="size-3.5 text-[var(--accent)]" />
          Recent Trades
        </CardTitle>

        <div className="flex flex-wrap gap-1.5">
          <FilterButton label="All" value="all" activeFilter={filter} onClick={changeFilter} />
          <FilterButton label="Wins" value="WIN" activeFilter={filter} onClick={changeFilter} />
          <FilterButton label="Losses" value="LOSS" activeFilter={filter} onClick={changeFilter} />
          <FilterButton label="Pending" value="pending" activeFilter={filter} onClick={changeFilter} />
        </div>
      </CardHeader>

      <CardContent className="px-0 pb-0">
        <div className="scrollbar-thin overflow-x-auto">
          <Table className="mono text-xs">
            <TableHeader>
              <TableRow className="border-b border-[rgba(30,45,61,0.5)] hover:bg-transparent">
                <TableHead className="h-8 whitespace-nowrap px-3 text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] sm:px-3">Time</TableHead>
                <TableHead className="h-8 whitespace-nowrap px-3 text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] sm:px-3">Status</TableHead>
                <TableHead className="h-8 whitespace-nowrap px-3 text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] sm:px-3">Dir</TableHead>
                <TableHead className="h-8 whitespace-nowrap px-3 text-right text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] sm:px-3">PnL</TableHead>
                <TableHead className="hidden h-8 whitespace-nowrap px-3 text-right text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] sm:table-cell sm:px-3">Entry BTC</TableHead>
                <TableHead className="hidden h-8 whitespace-nowrap px-3 text-right text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] sm:table-cell sm:px-3">Exit BTC</TableHead>
                <TableHead className="h-8 whitespace-nowrap px-3 text-right text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] sm:px-3">Price</TableHead>
                <TableHead className="hidden h-8 whitespace-nowrap px-3 text-right text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] md:table-cell sm:px-3">Edge</TableHead>
                <TableHead className="hidden h-8 whitespace-nowrap px-3 text-right text-[10px] font-medium uppercase tracking-wider text-[var(--text-dim)] md:table-cell sm:px-3">Payoff</TableHead>
              </TableRow>
            </TableHeader>

            <TableBody>
              {pagedTrades.map((trade) => (
                <TableRow key={buildTradeKey(trade)} className="trade-row border-b-0">
                  <TableCell className="whitespace-nowrap px-3 py-2 text-[var(--text-secondary)]" title={trade.time}>
                    {formatTradeTime(trade.time)}
                  </TableCell>
                  <TableCell className="px-3 py-2">{renderStatusBadge(trade)}</TableCell>
                  <TableCell className="px-3 py-2">{renderDirectionBadge(trade.direction)}</TableCell>
                  <TableCell className="whitespace-nowrap px-3 py-2 text-right">{renderPnl(trade)}</TableCell>
                  <TableCell className="hidden whitespace-nowrap px-3 py-2 text-right text-[var(--text-secondary)] sm:table-cell">
                    {isSettlement(trade) ? formatBtc(trade.entryBTC) : '\u2014'}
                  </TableCell>
                  <TableCell className="hidden whitespace-nowrap px-3 py-2 text-right text-[var(--text-secondary)] sm:table-cell">
                    {isSettlement(trade) ? formatBtc(trade.exitBTC) : '\u2014'}
                  </TableCell>
                  <TableCell className="whitespace-nowrap px-3 py-2 text-right text-[var(--text-dim)]">
                    {trade.price !== null ? trade.price.toFixed(3) : '\u2014'}
                  </TableCell>
                  <TableCell className="hidden whitespace-nowrap px-3 py-2 text-right text-[var(--text-dim)] md:table-cell">
                    {trade.edge !== null ? `${trade.edge.toFixed(1)}%` : '\u2014'}
                  </TableCell>
                  <TableCell className="hidden whitespace-nowrap px-3 py-2 text-right text-[var(--text-dim)] md:table-cell">
                    {trade.payoff ? `${trade.payoff}x` : '\u2014'}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>

        {totalPages > 1 && (
          <div className="flex items-center justify-between border-t border-[rgba(30,45,61,0.5)] px-3 py-2 sm:px-4">
            <span className="mono text-[10px] text-[var(--text-dim)] sm:text-xs">
              {filteredTrades.length} trades
              <span className="hidden sm:inline"> · Page {safePage + 1}/{totalPages}</span>
            </span>
            <div className="flex items-center gap-1">
              <Button
                size="xs"
                variant="ghost"
                className="mono size-7 rounded-md p-0 text-xs sm:h-7 sm:w-auto sm:px-3"
                disabled={safePage === 0}
                style={{ color: 'var(--text-dim)' }}
                onClick={() => setPage(safePage - 1)}
              >
                <ChevronLeft className="size-3.5 sm:hidden" />
                <span className="hidden sm:inline">\u2190 Prev</span>
              </Button>
              <span className="mono text-[10px] text-[var(--text-dim)] sm:hidden">
                {safePage + 1}/{totalPages}
              </span>
              <Button
                size="xs"
                variant="ghost"
                className="mono size-7 rounded-md p-0 text-xs sm:h-7 sm:w-auto sm:px-3"
                disabled={safePage >= totalPages - 1}
                style={{ color: 'var(--text-dim)' }}
                onClick={() => setPage(safePage + 1)}
              >
                <ChevronRight className="size-3.5 sm:hidden" />
                <span className="hidden sm:inline">Next \u2192</span>
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
    <button
      type="button"
      className="filter-chip mono"
      data-active={active || undefined}
      onClick={() => onClick(value)}
    >
      {label}
    </button>
  )
}

function renderStatusBadge(trade: TradeRecord) {
  if (isEntry(trade)) {
    return (
      <Badge className="inline-flex items-center gap-1 rounded-md border-0 bg-[rgba(255,165,2,0.12)] px-1.5 py-0.5 text-[10px] text-[var(--warn)]">
        <Clock className="size-2.5" />
        PENDING
      </Badge>
    )
  }

  const win = trade.result === 'WIN'
  return (
    <Badge
      className="inline-flex items-center gap-1 rounded-md border-0 px-1.5 py-0.5 text-[10px]"
      style={{
        background: win ? 'rgba(0,212,170,0.12)' : 'rgba(255,71,87,0.12)',
        color: win ? 'var(--win)' : 'var(--loss)',
      }}
    >
      {win ? <CheckCircle className="size-2.5" /> : <XCircle className="size-2.5" />}
      {win ? 'WIN' : 'LOSS'}
    </Badge>
  )
}

function renderDirectionBadge(direction: 'UP' | 'DOWN') {
  const up = direction === 'UP'
  return (
    <Badge
      className="inline-flex items-center gap-1 rounded-md border-0 px-1.5 py-0.5 text-[10px]"
      style={{
        background: up ? 'rgba(0,212,170,0.12)' : 'rgba(255,71,87,0.12)',
        color: up ? 'var(--win)' : 'var(--loss)',
      }}
    >
      {up ? <TrendingUp className="size-2.5" /> : <TrendingDown className="size-2.5" />}
      {direction}
    </Badge>
  )
}

function renderPnl(trade: TradeRecord) {
  if (isEntry(trade)) {
    return <span className="text-[var(--text-dim)]">\u2014</span>
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

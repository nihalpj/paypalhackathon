import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import './App.css'
import LedgerGrid from './components/LedgerGrid'
import Toolbar from './components/Toolbar'
import DetailDrawer from './components/DetailDrawer'
import { fetchLedger, postPetition, postQuery } from './lib/api'
import { applyFilter, filterChips } from './lib/format'
import type { LedgerFilter, LedgerRow } from './types'

const POLL_MS = 15_000

export default function App() {
  const [rows, setRows] = useState<LedgerRow[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [filter, setFilter] = useState<LedgerFilter | null>(null)
  const [nlAvailable, setNlAvailable] = useState(true)
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [drawerRow, setDrawerRow] = useState<LedgerRow | null>(null)
  const [petitioning, setPetitioning] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)

  const load = useCallback(async () => {
    abortRef.current?.abort()
    const ctrl = new AbortController()
    abortRef.current = ctrl
    try {
      const data = await fetchLedger()
      if (ctrl.signal.aborted) return
      setRows(data)
      setError(null)
    } catch (err) {
      if (ctrl.signal.aborted) return
      setError(err instanceof Error ? err.message : 'Could not reach the AutoFIRC API')
    } finally {
      if (!ctrl.signal.aborted) setLoading(false)
    }
  }, [])

  // Initial load + 15s polling; interval cleared on unmount.
  useEffect(() => {
    load()
    const timer = window.setInterval(load, POLL_MS)
    return () => {
      window.clearInterval(timer)
      abortRef.current?.abort()
    }
  }, [load])

  // Keep the open drawer fresh across polls.
  useEffect(() => {
    if (drawerRow) {
      const fresh = rows.find((r) => r.id === drawerRow.id)
      if (fresh && fresh.updated_at !== drawerRow.updated_at) setDrawerRow(fresh)
    }
  }, [rows, drawerRow])

  const visibleRows = useMemo(() => applyFilter(rows, filter), [rows, filter])
  const chips = useMemo(() => filterChips(filter), [filter])

  const eligibleSelected = useMemo(
    () =>
      selectedIds.filter(
        (id) => rows.find((r) => r.id === id)?.status === 'compliance_packet_ready',
      ),
    [selectedIds, rows],
  )

  const handleSearch = useCallback(async (query: string) => {
    setNotice(null)
    try {
      const f = await postQuery(query)
      setNlAvailable(true)
      setFilter(f)
    } catch {
      // No LLM key configured (or query API down) — plain client-side text search.
      setNlAvailable(false)
      setFilter({ text: query })
    }
  }, [])

  const handleClearFilter = useCallback(() => {
    setFilter(null)
    setNlAvailable(true)
    setNotice(null)
  }, [])

  const handlePetition = useCallback(async () => {
    if (eligibleSelected.length === 0) return
    setPetitioning(true)
    setNotice(null)
    try {
      const res = await postPetition(eligibleSelected)
      setNotice(
        `Petition queued for ${res.queued} payment${res.queued === 1 ? '' : 's'}` +
          (res.skipped.length > 0 ? ` — ${res.skipped.length} skipped (not packet-ready)` : ''),
      )
      await load()
    } catch (err) {
      setNotice(`Petition failed: ${err instanceof Error ? err.message : 'unknown error'}`)
    } finally {
      setPetitioning(false)
    }
  }, [eligibleSelected, load])

  return (
    <div className="app">
      <header className="app-header">
        <div>
          <h1>
            Auto<span className="brand-accent">FIRC</span>
          </h1>
          <p className="tagline">Cross-border compliance ledger — RBI purpose codes, GST invoices, FEMA realization</p>
        </div>
      </header>

      <main className="app-main">
        {error && (
          <div className="banner banner-error" role="alert">
            <span>Couldn’t load the ledger — {error}</span>
            <button className="btn btn-secondary btn-small" onClick={() => void load()}>
              Retry
            </button>
          </div>
        )}
        {notice && (
          <div className="banner banner-info" role="status">
            {notice}
          </div>
        )}

        <Toolbar
          rows={visibleRows}
          filterActive={filter !== null}
          filterChipsActive={chips}
          nlAvailable={nlAvailable}
          selectedCount={eligibleSelected.length}
          petitioning={petitioning}
          onSearch={handleSearch}
          onClearFilter={handleClearFilter}
          onPetition={() => void handlePetition()}
          onRefresh={() => void load()}
        />

        <div className="grid-area">
          {loading ? (
            <div className="skeleton" aria-label="Loading ledger">
              {Array.from({ length: 8 }, (_, i) => (
                <div key={i} className="skeleton-row" />
              ))}
            </div>
          ) : visibleRows.length === 0 ? (
            <div className="empty">
              <h2>No payments to show</h2>
              <p className="muted">
                {rows.length === 0
                  ? 'Payments appear here as PayPal webhooks arrive. Fire a test webhook and refresh.'
                  : 'No rows match the current filter. Clear it to see the full ledger.'}
              </p>
              {rows.length > 0 && (
                <button className="btn btn-secondary" onClick={handleClearFilter}>
                  Clear filter
                </button>
              )}
            </div>
          ) : (
            <LedgerGrid
              rows={visibleRows}
              onRowClicked={setDrawerRow}
              onSelectionChanged={setSelectedIds}
            />
          )}
        </div>
      </main>

      {drawerRow && <DetailDrawer row={drawerRow} onClose={() => setDrawerRow(null)} />}
    </div>
  )
}

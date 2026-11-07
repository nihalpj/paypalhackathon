import { useState, type FormEvent } from 'react'
import type { LedgerRow } from '../types'
import { formatInr, rowAlert } from '../lib/format'
import NewTransactionModal from './NewTransactionModal'

export interface ToolbarProps {
  rows: LedgerRow[]
  filterActive: boolean
  filterChipsActive: string[]
  nlAvailable: boolean
  selectedCount: number
  petitioning: boolean
  onSearch: (query: string) => void
  onClearFilter: () => void
  onPetition: () => void
  onRefresh: () => void
}

export default function Toolbar({
  rows,
  filterActive,
  filterChipsActive,
  nlAvailable,
  selectedCount,
  petitioning,
  onSearch,
  onClearFilter,
  onPetition,
  onRefresh,
}: ToolbarProps) {
  const [query, setQuery] = useState('')
  const [showNewTxn, setShowNewTxn] = useState(false)

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (query.trim()) onSearch(query.trim())
  }

  const counts = { green: 0, amber: 0, red: 0 }
  let unreconciled = 0
  let pendingPaise = 0
  for (const row of rows) {
    counts[rowAlert(row)]++
    if (row.status !== 'fully_reconciled') {
      unreconciled++
      pendingPaise += row.amount_inr_paise
    }
  }

  return (
    <div className="toolbar">
      <div className="toolbar-row">
        <form className="search" onSubmit={submit}>
          <input
            type="search"
            value={query}
            placeholder="Ask your ledger — e.g. “low-confidence amber alerts from Acme”"
            aria-label="Natural language ledger search"
            onChange={(e) => setQuery(e.target.value)}
          />
          <button type="submit" className="btn btn-secondary">
            Search
          </button>
        </form>
        <div className="toolbar-actions">
          <button
            className="btn btn-secondary"
            onClick={() => setShowNewTxn(true)}
            title="Start a new cross-border payment through the PayPal sandbox checkout"
          >
            + New transaction
          </button>
          <button
            className="btn btn-primary"
            disabled={selectedCount === 0 || petitioning}
            onClick={onPetition}
            title="Authorize bank petition + DGFT reconciliation for selected packets"
          >
            {petitioning ? 'Petitioning…' : `Petition bank (${selectedCount})`}
          </button>
          <button className="btn btn-ghost" onClick={onRefresh}>
            Refresh
          </button>
        </div>
      </div>

      {showNewTxn && <NewTransactionModal onClose={() => setShowNewTxn(false)} />}

      {(filterActive || filterChipsActive.length > 0) && (
        <div className="filter-line">
          {filterChipsActive.map((chip) => (
            <span key={chip} className="filter-chip">
              {chip}
            </span>
          ))}
          {!nlAvailable && (
            <span className="filter-note">LLM filter unavailable — using plain text search</span>
          )}
          <button className="btn btn-ghost btn-small" onClick={onClearFilter}>
            Clear filter
          </button>
        </div>
      )}

      <div className="summary" aria-label="Ledger summary">
        <span className="summary-item">
          <span className="alert-dot alert-dot-green" aria-hidden /> {counts.green} green
        </span>
        <span className="summary-item">
          <span className="alert-dot alert-dot-amber" aria-hidden /> {counts.amber} amber
        </span>
        <span className="summary-item">
          <span className="alert-dot alert-dot-red" aria-hidden /> {counts.red} red
        </span>
        <span className="summary-item">
          {unreconciled} unreconciled
        </span>
        <span className="summary-item summary-pending">
          {formatInr(pendingPaise)} pending realization
        </span>
      </div>
    </div>
  )
}

import { useEffect, useState } from 'react'
import type { DocumentsResponse, LedgerRow } from '../types'
import { PIPELINE, STATUS_LABELS } from '../types'
import { fetchDocuments } from '../lib/api'
import { formatInr, rowAlert } from '../lib/format'
import { femaDeadline } from '../lib/fema'
import { format } from 'date-fns'

interface FieldProps {
  label: string
  value: React.ReactNode
}

function Field({ label, value }: FieldProps) {
  return (
    <div className="drawer-field">
      <dt>{label}</dt>
      <dd>{value ?? <span className="muted">—</span>}</dd>
    </div>
  )
}

const MODE_OF_EXPORT: Record<number, string> = {
  1: 'Mode 1 — cross-border supply',
  2: 'Mode 2 — consumption abroad',
  3: 'Mode 3 — commercial presence',
  4: 'Mode 4 — presence of natural persons',
}

function PipelineStepper({ status }: { status: LedgerRow['status'] }) {
  const currentIndex = PIPELINE.indexOf(status)
  return (
    <ol className="stepper" aria-label="Compliance pipeline progress">
      {PIPELINE.map((step, i) => {
        const state =
          step === status
            ? 'current'
            : step === 'manual_review' && status !== 'manual_review'
              ? i < currentIndex
                ? 'visited-branch'
                : 'branch'
              : i < currentIndex
                ? 'done'
                : 'todo'
        return (
          <li key={step} className={`step step-${state}`} title={STATUS_LABELS[step]}>
            <span className="step-node" aria-hidden />
            <span className="step-label">
              {STATUS_LABELS[step]}
              {step === 'manual_review' && <span className="step-branch-tag">side branch</span>}
            </span>
          </li>
        )
      })}
    </ol>
  )
}

type DocsState =
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'ready'; docs: DocumentsResponse | null }

export default function DetailDrawer({
  row,
  onClose,
}: {
  row: LedgerRow
  onClose: () => void
}) {
  const [docs, setDocs] = useState<DocsState>({ kind: 'loading' })

  useEffect(() => {
    let cancelled = false
    setDocs({ kind: 'loading' })
    fetchDocuments(row.id)
      .then((d) => !cancelled && setDocs({ kind: 'ready', docs: d }))
      .catch((err: unknown) => {
        if (!cancelled)
          setDocs({
            kind: 'error',
            message: err instanceof Error ? err.message : 'Failed to load documents',
          })
      })
    return () => {
      cancelled = true
    }
  }, [row.id])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const alert = rowAlert(row)
  const deadline = femaDeadline(row.invoice_date, row.inr_vostro)

  return (
    <aside className="drawer" aria-label={`Payment detail: ${row.client_name}`}>
      <header className="drawer-header">
        <div>
          <h2>{row.client_name}</h2>
          <p className="mono muted">{row.id}</p>
        </div>
        <button className="btn btn-ghost" onClick={onClose} aria-label="Close detail panel">
          ✕
        </button>
      </header>

      <div className="drawer-body">
        <div className={`alert-banner alert-banner-${alert}`}>
          <span className={`alert-badge alert-${alert}`}>
            <span className="alert-dot" aria-hidden />
            {alert === 'green' && row.status === 'fully_reconciled'
              ? 'FEMA realized & reconciled'
              : `FEMA deadline ${format(deadline, 'dd MMM yyyy')}`}
          </span>
        </div>

        <section>
          <h3>Payment</h3>
          <dl className="drawer-grid">
            <Field label="Amount" value={<span className="amount">{formatInr(row.amount_inr_paise)}</span>} />
            <Field label="Invoice date" value={<span className="mono">{row.invoice_date}</span>} />
            <Field
              label="Invoice number"
              value={row.invoice_number ? <span className="mono">{row.invoice_number}</span> : null}
            />
            <Field label="Vostro account" value={row.inr_vostro ? 'INR vostro (12-month window)' : 'Regular (9-month window)'} />
            <Field label="Capture ID" value={<span className="mono">{row.capture_id}</span>} />
            <Field label="Order ID" value={<span className="mono">{row.order_id}</span>} />
            <Field label="eBRC number" value={row.ebrc_number ? <span className="mono">{row.ebrc_number}</span> : null} />
          </dl>
        </section>

        <section>
          <h3>AI classification</h3>
          <dl className="drawer-grid">
            <Field label="RBI purpose code" value={row.purpose_code ? <span className="mono">{row.purpose_code}</span> : null} />
            <Field label="SAC code" value={row.sac_code ? <span className="mono">{row.sac_code}</span> : null} />
            <Field
              label="GATS mode"
              value={row.mode_of_export ? MODE_OF_EXPORT[row.mode_of_export] : null}
            />
            <Field label="AI confidence" value={row.ai_confidence != null ? `${row.ai_confidence}%` : null} />
          </dl>
          {row.backstop_note && (
            <p className="backstop-note">
              <strong>Needs review —</strong> {row.backstop_note}
            </p>
          )}
        </section>

        <section>
          <h3>Documents</h3>
          {docs.kind === 'loading' && <p className="muted">Fetching presigned links…</p>}
          {docs.kind === 'error' && (
            <p className="doc-error">
              Document links unavailable right now ({docs.message}). Storage may not be configured —
              PDFs remain in the compliance packet.
            </p>
          )}
          {docs.kind === 'ready' && (
            <div className="doc-actions">
              {docs.docs?.invoice_url ? (
                <a
                  className="btn btn-secondary"
                  href={docs.docs.invoice_url}
                  target="_blank"
                  rel="noreferrer"
                >
                  Download invoice
                </a>
              ) : (
                <button className="btn btn-secondary" disabled>
                  Invoice not ready
                </button>
              )}
              {docs.docs?.lut_url ? (
                <a
                  className="btn btn-secondary"
                  href={docs.docs.lut_url}
                  target="_blank"
                  rel="noreferrer"
                >
                  Download LUT
                </a>
              ) : (
                <button className="btn btn-secondary" disabled>
                  LUT not ready
                </button>
              )}
            </div>
          )}
        </section>

        <section>
          <h3>Pipeline</h3>
          <PipelineStepper status={row.status} />
        </section>
      </div>
    </aside>
  )
}

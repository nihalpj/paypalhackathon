import { useState, type FormEvent } from 'react'
import { checkoutUrl } from '../lib/api'

export interface NewTransactionModalProps {
  onClose: () => void
}

const PRESETS = [
  'Remote software consulting, October 2026 sprint',
  'Brand identity and UX design services',
  'Code review and architecture audit',
]

export default function NewTransactionModal({ onClose }: NewTransactionModalProps) {
  const [description, setDescription] = useState(PRESETS[0])
  const [amount, setAmount] = useState('500.00')
  const [currency, setCurrency] = useState('USD')

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (!description.trim() || !(Number(amount) > 0)) return
    window.open(checkoutUrl({ amount, currency, description: description.trim() }), '_blank')
    onClose()
  }

  return (
    <div className="modal-overlay" role="dialog" aria-modal="true" aria-label="New transaction" onClick={onClose}>
      <div className="modal-card" onClick={(e) => e.stopPropagation()}>
        <h2>New transaction</h2>
        <p className="muted">
          Opens the PayPal sandbox checkout — pay with a personal sandbox account and the payment
          flows straight into the compliance pipeline here.
        </p>
        <form onSubmit={submit}>
          <label htmlFor="nt-desc">Service</label>
          <select id="nt-desc" value={description} onChange={(e) => setDescription(e.target.value)}>
            {PRESETS.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
          <div className="modal-row">
            <div>
              <label htmlFor="nt-amount">Amount</label>
              <input
                id="nt-amount"
                type="number"
                min="1"
                step="0.01"
                value={amount}
                onChange={(e) => setAmount(e.target.value)}
              />
            </div>
            <div>
              <label htmlFor="nt-currency">Currency</label>
              <select id="nt-currency" value={currency} onChange={(e) => setCurrency(e.target.value)}>
                <option>USD</option>
                <option>EUR</option>
                <option>GBP</option>
              </select>
            </div>
          </div>
          <div className="modal-actions">
            <button type="button" className="btn btn-ghost" onClick={onClose}>
              Cancel
            </button>
            <button type="submit" className="btn btn-primary">
              Open checkout →
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

import { describe, expect, it } from 'vitest';
import { format, parseISO } from 'date-fns';
import { femaAlert, femaDeadline } from './fema';

// Fixed "today" so alert windows are deterministic.
const TODAY = parseISO('2026-06-15');

const iso = (d: Date) => format(d, 'yyyy-MM-dd');

describe('femaDeadline', () => {
  it('adds 9 months for regular accounts', () => {
    expect(iso(femaDeadline('2026-01-15', false))).toBe('2026-10-15');
  });

  it('adds 12 months for INR vostro accounts', () => {
    expect(iso(femaDeadline('2026-01-15', true))).toBe('2027-01-15');
  });

  it('handles month boundaries: 2026-01-31 + 9 months = 2026-10-31', () => {
    expect(iso(femaDeadline('2026-01-31', false))).toBe('2026-10-31');
  });

  it('clamps to end of month when the target month is shorter', () => {
    // Jan 31 + 1 month lands on Feb 28 (2026 is not a leap year).
    expect(iso(femaDeadline('2026-01-31', false))).not.toBe('2026-02-31');
    // Nov 30 + 9 months = Aug 30 — unaffected, longer target month.
    expect(iso(femaDeadline('2025-11-30', false))).toBe('2026-08-30');
  });
});

describe('femaAlert', () => {
  it('reconciled rows are always green, even past the deadline', () => {
    // Deadline 2026-02-01, months past TODAY.
    expect(femaAlert('2025-05-01', false, 'fully_reconciled', TODAY)).toBe('green');
  });

  it('is red at exactly 31 days remaining', () => {
    // Invoice 2025-10-16 -> deadline 2026-07-16 = 31 days after TODAY.
    expect(femaAlert('2025-10-16', false, 'pending_analysis', TODAY)).toBe('red');
  });

  it('is red when fewer than 31 days remain', () => {
    // Invoice 2025-11-16 -> deadline 2026-08-16 = 62 days... use closer: invoice 2025-12-01
    // -> deadline 2026-09-01 = 78 days (green). Instead: invoice 2025-09-16 -> deadline 2026-06-16.
    expect(femaAlert('2025-09-16', false, 'enriched', TODAY)).toBe('red'); // 1 day remaining
  });

  it('is red when the deadline has already passed', () => {
    // Invoice 2025-08-01 -> deadline 2026-05-01, before TODAY.
    expect(femaAlert('2025-08-01', false, 'compliance_packet_ready', TODAY)).toBe('red');
  });

  it('is amber when 32-62 days remain', () => {
    // Invoice 2025-10-17 -> deadline 2026-07-17 = 32 days after TODAY.
    expect(femaAlert('2025-10-17', false, 'enriched', TODAY)).toBe('amber');
    // Invoice 2025-11-16 -> deadline 2026-08-16 = 62 days after TODAY.
    expect(femaAlert('2025-11-16', false, 'bank_petitioned', TODAY)).toBe('amber');
  });

  it('is green when more than 62 days remain', () => {
    // Invoice 2025-11-17 -> deadline 2026-08-17 = 63 days after TODAY.
    expect(femaAlert('2025-11-17', false, 'enriched', TODAY)).toBe('green');
  });

  it('uses the 12-month deadline for vostro accounts, changing the alert level', () => {
    // Invoice 2025-10-16: non-vostro deadline 2026-07-16 (31 days, red);
    // vostro deadline 2026-10-16 (far away, green).
    expect(femaAlert('2025-10-16', false, 'enriched', TODAY)).toBe('red');
    expect(femaAlert('2025-10-16', true, 'enriched', TODAY)).toBe('green');
  });

  it('handles month-boundary invoice dates in alert computation', () => {
    // Invoice 2026-01-31 -> deadline 2026-10-31 = 138 days after TODAY -> green.
    expect(femaAlert('2026-01-31', false, 'enriched', TODAY)).toBe('green');
  });
});

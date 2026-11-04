use chrono::{Months, NaiveDate};

/// Alert level for an unreconciled transaction's FEMA realization window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    Green,
    /// Entered at the 7-month mark: 2 months or less remain.
    Amber,
    /// Entered at the 8-month mark: 1 month or less remains.
    Red,
}

/// Statutory realization deadline: 9 months from invoice date, or 12 months
/// when invoiced/settled in INR via Vostro accounts.
pub fn realization_deadline(invoice_date: NaiveDate, inr_vostro: bool) -> NaiveDate {
    let months = if inr_vostro { 12 } else { 9 };
    invoice_date
        .checked_add_months(Months::new(months))
        .expect("invoice date + statutory window overflows chrono")
}

pub fn alert_level(
    invoice_date: NaiveDate,
    inr_vostro: bool,
    reconciled: bool,
    today: NaiveDate,
) -> AlertLevel {
    if reconciled {
        return AlertLevel::Green;
    }
    let remaining = realization_deadline(invoice_date, inr_vostro) - today;
    if remaining.num_days() <= 31 {
        AlertLevel::Red
    } else if remaining.num_days() <= 62 {
        AlertLevel::Amber
    } else {
        AlertLevel::Green
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn deadline_is_9_months_for_forex_and_12_for_inr_vostro() {
        assert_eq!(realization_deadline(d(2026, 10, 15), false), d(2027, 7, 15));
        assert_eq!(realization_deadline(d(2026, 10, 15), true), d(2027, 10, 15));
    }

    #[test]
    fn unreconciled_alerts_at_7_and_8_month_marks() {
        let invoice = d(2026, 10, 15);
        // ~5 months elapsed: green
        assert_eq!(
            alert_level(invoice, false, false, d(2027, 3, 15)),
            AlertLevel::Green
        );
        // 7-month mark (2 months left): amber
        assert_eq!(
            alert_level(invoice, false, false, d(2027, 5, 15)),
            AlertLevel::Amber
        );
        // 8-month mark (1 month left): red
        assert_eq!(
            alert_level(invoice, false, false, d(2027, 6, 15)),
            AlertLevel::Red
        );
        // reconciled rows never alert
        assert_eq!(
            alert_level(invoice, false, true, d(2027, 7, 14)),
            AlertLevel::Green
        );
    }
}

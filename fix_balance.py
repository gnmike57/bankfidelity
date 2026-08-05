import re

with open('src/engine/balance.rs', 'r') as f:
    content = f.read()

old_code = """    let old_debit = target_tx.debit.unwrap_or(Decimal::ZERO);
    let old_credit = target_tx.credit.unwrap_or(Decimal::ZERO);

    if old_credit > Decimal::ZERO && old_debit == Decimal::ZERO {
        target_tx.credit = Some(old_credit - discrepancy);
    } else {
        target_tx.debit = Some(old_debit + discrepancy);
    }"""

new_code = """    let old_debit = target_tx.debit.unwrap_or(Decimal::ZERO);
    let old_credit = target_tx.credit.unwrap_or(Decimal::ZERO);
    let net = old_debit - old_credit + discrepancy;

    // Advanced mathematical reconciliation: prevent negative amounts
    // and seamlessly cross the zero boundary if a credit becomes a debit
    // or vice versa due to severe OCR sign flips.
    if net > Decimal::ZERO {
        target_tx.debit = Some(net);
        target_tx.credit = None;
    } else if net < Decimal::ZERO {
        target_tx.credit = Some(-net);
        target_tx.debit = None;
    } else {
        // Exactly zero net delta - keep it empty
        target_tx.debit = None;
        target_tx.credit = None;
    }"""

content = content.replace(old_code, new_code)

with open('src/engine/balance.rs', 'w') as f:
    f.write(content)


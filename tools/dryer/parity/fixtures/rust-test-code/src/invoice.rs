pub fn invoice_label(legs: &[u32]) -> String {
    let first = first_of(legs);
    let text = text_of(first);
    join_label(text, legs)
}

#[cfg(test)]
mod unit_tests {
    fn invoice_total(orders: &[Order]) -> u64 {
        let sum = amount_of(orders);
        let tax = tax_of(sum);
        let fee = fee_of(tax);
        combine(sum, tax, fee)
    }
}

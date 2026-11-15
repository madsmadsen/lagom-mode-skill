#[cfg(test)]
mod checks {
    #[test]
    fn receipt_total(orders: &[Order]) -> u64 {
        let sum = amount_of(orders);
        let fee = fee_of(orders, tax_of(sum));
        let base = combine(sum, fee);
        let total = combine(base, sum);
        round_of(total)
    }
}

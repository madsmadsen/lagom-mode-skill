pub fn invoice_total(orders: &[Order]) -> u64 {
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    for order in orders {
        if order.paid && order.domestic {
            count = count + 1;
            sum = sum + order.amount;
        }
    }
    let tax = sum / 10;
    sum + tax + count
}

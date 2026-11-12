export function invoiceTotal(orders: Order[]): number {
  let sum = 0;
  let count = 0;
  for (const order of orders) {
    if (order.paid && order.domestic) {
      count = count + 1;
      sum = sum + order.amount;
    }
  }
  const tax = sum / 10;
  return sum + tax + count;
}

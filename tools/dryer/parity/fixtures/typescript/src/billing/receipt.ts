export function receiptTotal(lines: Line[]): number {
  let sum = 0;
  let count = 0;
  for (const line of lines) {
    if (line.paid && line.domestic) {
      count = count + 1;
      sum = sum + line.amount;
    }
  }
  const tax = sum / 10;
  return sum + tax + count;
}

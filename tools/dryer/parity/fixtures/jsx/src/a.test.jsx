export function totalPrice(items, tax) {
  let subtotal = 0;
  for (const item of items) {
    subtotal += item.price * item.quantity;
  }
  const result = subtotal + subtotal * tax;
  return <span>{result}</span>;
}

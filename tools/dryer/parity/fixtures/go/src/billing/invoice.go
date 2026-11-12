package billing

func InvoiceTotal(orders []Order) int {
	sum := 0
	count := 0
	for _, order := range orders {
		if order.Paid && order.Domestic {
			count = count + 1
			sum = sum + order.Amount
		}
	}
	tax := sum / 10
	return sum + tax + count
}

package billing

func ReceiptTotal(lines []Line) int {
	sum := 0
	count := 0
	for _, line := range lines {
		if line.Paid && line.Domestic {
			count = count + 1
			sum = sum + line.Amount
		}
	}
	tax := sum / 10
	return sum + tax + count
}

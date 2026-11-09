package calc

func Grade(score int) string {
	if score >= 90 {
		return "A"
	} else if score >= 80 {
		return "B"
	}
	return "C"
}

func Sum(xs []int) int {
	total := 0
	for _, x := range xs {
		total += x
	}
	return total
}

package shipping

func RouteLabel(legs []int) string {
	out := ""
	index := 0
	for index < len(legs) {
		if legs[index] > 5 {
			out = out + "long"
		} else {
			out = out + "short"
		}
		index++
	}
	return out
}

func One() int {
	return 1
}

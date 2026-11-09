package shipping

type Shape struct {
	Width  int
	Height int
}

func Costs(legs []int) []int {
	double := func(x int) int { return x * 2 }
	out := make([]int, 0, len(legs))
	for _, leg := range legs {
		out = append(out, double(leg))
	}
	return out
}

func (s Shape) Perimeter() int {
	w := s.Width
	h := s.Height
	return 2 * (w + h)
}

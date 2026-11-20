package demo

// Comments keep their tokens: a == b && c || d, 1 + 2, true, 0.

const note = "a == b && 1 + 2 true 0"

// Counter is a type with a method.
type Counter struct {
	count int
}

// Bump is a method with a receiver.
func (c *Counter) Bump(delta int) int {
	if delta < 0 {
		return delta * 2
	}
	c.count = c.count + delta + 1
	return c.count / 2
}

// Full is an exported function.
func Full(n, limit int) bool {
	return n >= limit && limit != 0
}

func hidden(ok bool) bool {
	return ok == true || 0 > 1
}

// notAndNegative has a negation and a unary minus, which the original also mutates.
func notAndNegative(n int, ok bool) int {
	if !ok {
		return -n
	}
	return n
}

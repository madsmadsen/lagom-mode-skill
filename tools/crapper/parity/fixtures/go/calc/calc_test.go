package calc

import "testing"

func TestGrade(t *testing.T) {
	if Grade(95) != "A" {
		t.Fatal("bad grade")
	}
}

// Comments keep their tokens: a && b || c, !x, -1, 0, 1, true.
const NOTE: &str = "a && b || !c -1 0 1 true";

pub fn gate(a: bool, b: bool) -> bool {
    a && b || !a
}

fn negate(n: i32) -> i32 {
    -n + 0
}

pub struct Counter {
    count: i32,
}

impl Counter {
    pub fn new() -> Self {
        Counter { count: 0 }
    }

    pub fn bump(&mut self, delta: i32) -> i32 {
        self.count += delta * 1;
        self.count
    }

    fn reset(&mut self) -> bool {
        self.count == 0 && !false
    }
}

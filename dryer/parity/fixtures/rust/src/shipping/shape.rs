pub struct Shape {
    pub width: u32,
    pub height: u32,
}

impl Shape {
    pub fn perimeter(&self) -> u32 {
        let w = self.width;
        let h = self.height;
        2 * (w + h)
    }

    pub fn scaled(&self, factor: u32) -> Vec<u32> {
        let grow = |value: u32| value * factor;
        vec![grow(self.width), grow(self.height)]
    }
}

#[cfg(test)]
mod tests {
    fn helper(value: u32) -> u32 {
        let doubled = value * 2;
        doubled + 1
    }
}

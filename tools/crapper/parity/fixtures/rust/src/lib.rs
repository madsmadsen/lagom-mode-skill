pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "neg"
    } else if n == 0 {
        "zero"
    } else {
        match n {
            1..=9 => "small",
            _ => "big",
        }
    }
}

pub fn first_even(xs: &[i32]) -> Option<i32> {
    for x in xs {
        if x % 2 == 0 {
            return Some(*x);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn works() {
        assert_eq!(super::classify(1), "small");
    }
}

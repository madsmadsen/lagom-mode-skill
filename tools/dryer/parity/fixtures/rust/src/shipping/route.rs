pub fn route_label(legs: &[u32]) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < legs.len() {
        out.push_str(if legs[index] > 5 { "long" } else { "short" });
        index += 1;
    }
    out
}

pub fn one() -> u32 {
    1
}

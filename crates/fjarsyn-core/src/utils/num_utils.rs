#[inline]
pub fn align_to_rounded<
    T: Copy
        + std::ops::Sub<i32, Output = T>
        + std::ops::Add<Output = T>
        + std::ops::BitAnd<Output = T>
        + std::ops::Not<Output = T>,
>(
    value: T,
    alignment: T,
) -> T {
    (value + alignment - 1) & !(alignment - 1)
}

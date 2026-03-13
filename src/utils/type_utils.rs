#[macro_export]
macro_rules! get_type_name {
    // Base case: just an ident
    ($name:ident) => { stringify!($name) };

    // Recursive case: peel off one segment
    ($first:ident :: $($rest:tt)+) => { crate::get_type_name!($($rest)+) };
}

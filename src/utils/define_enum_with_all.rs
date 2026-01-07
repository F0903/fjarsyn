#[macro_export]
macro_rules! define_enum_with_all {
    (
        $(#[$meta:meta])*
        $vis:vis enum $Name:ident {
            $(
                $(#[$v_meta:meta])*
                $Variant:ident $(= $val:expr)?
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis enum $Name {
            $(
                $(#[$v_meta])*
                $Variant $(= $val)?,
            )*
        }

        impl $Name {
            pub const ALL: &'static [Self] = &[
                $(Self::$Variant),*
            ];
        }
    };
}

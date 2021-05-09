#[macro_export]
macro_rules! criterion_group {
    (name = $name:ident; config = $config:expr; targets = $( $target:path ),+ $(,)*) => {
        pub fn $name(criterion: &mut $crate::Criterion<'_>) {
            $(
                $target(criterion);
            )+
        }
    };
    ($name:ident, $( $target:path ),+ $(,)*) => {
        $crate::criterion_group!{
            name = $name;
            config = ();
            targets = $( $target ),+
        }
    }
}

// -------------------------------------------------------------------------
// Driver mode and rustdoc

#[macro_export]
#[cfg(not(any(feature = "role_target", feature = "role_proxy")))]
macro_rules! criterion_main {
    ( $( $group:path ),+ $(,)* ) => {
        fn main() {
            // suppress dead code warning
            $(
                let _: fn(&mut $crate::Criterion)  = $group;
            )+

            $crate::main(env!("CARGO_MANIFEST_DIR"));
        }
    }
}

// -------------------------------------------------------------------------
// Proxy mode

#[macro_export]
#[cfg(feature = "role_proxy")]
macro_rules! criterion_main {
    ( $( $group:path ),+ $(,)* ) => {
        fn main() {
            // suppress dead code warning
            $(
                let _: fn(&mut $crate::Criterion)  = $group;
            )+

            $crate::main();
        }
    }
}

// -------------------------------------------------------------------------
// Target mode

#[macro_export]
#[cfg(feature = "target_std")]
macro_rules! criterion_main {
    ( $( $group:path ),+ $(,)* ) => {
        fn main() {
            $crate::main(|c| {
                $(
                    $group(c);
                )+
            });
        }
    }
}

#[macro_export]
#[cfg(feature = "cortex-m-rt")]
macro_rules! criterion_main {
    ( $( $group:path ),+ $(,)* ) => {
        #[$crate::cortex_m_rt::entry]
        fn main() -> ! {
            $crate::main(|c| {
                $(
                    $group(c);
                )+
            });
        }
    }
}

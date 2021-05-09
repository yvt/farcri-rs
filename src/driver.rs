//! Driver mode entry point

#[doc(hidden)]
pub fn main(compile_time_cargo_manifest_dir: &str) {
    // Use the compile-time `CARGO_MANIFEST_DIR`
    if std::env::var_os("CARGO_MANIFEST_DIR").is_none()
        && !compile_time_cargo_manifest_dir.is_empty()
    {
        eprintln!(
            "We might be running inside cargo-criterion, which does not \
            set `CARGO_MANIFEST_DIR`. We will use the compile-time value ({:?})",
            compile_time_cargo_manifest_dir,
        );
        std::env::set_var("CARGO_MANIFEST_DIR", compile_time_cargo_manifest_dir);
    }

    let exe = super::cargo::compile_self(|cmd| {
        cmd.args(&[
            // Invoke Proxy mode
            "--features",
            "farcri/role_proxy",
        ])
    });

    eprintln!("Invoking FarCri.rs Proxy mode by executing {:?}", exe.path);

    let mut cmd = std::process::Command::new(exe.path);
    // Forward argumenfts
    cmd.args(std::env::args_os().skip(1));

    match () {
        #[cfg(unix)]
        () => {
            use std::os::unix::process::CommandExt;
            Err::<(), _>(cmd.exec()).unwrap();
        }

        #[cfg(not(unix))]
        () => {
            cmd.spawn().unwrap().wait().unwrap();
        }
    }
}

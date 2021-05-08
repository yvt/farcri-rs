//! Driver mode entry point

#[doc(hidden)]
pub fn main() {
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

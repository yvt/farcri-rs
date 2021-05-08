use super::BuildSetup;
use std::{ffi::OsString, io::Error};

/// Provides a `memory.x` file to be included by the linker script of
/// `cortex-m-rt`.
pub struct RtLdscriptSetup {
    dir: tempdir::TempDir,
}

impl RtLdscriptSetup {
    pub async fn new(memory_x_contents: &[u8]) -> Result<Self, Error> {
        let dir = tokio::task::spawn_blocking(|| tempdir::TempDir::new("farcri-rs"))
            .await
            .unwrap()?;

        tokio::fs::write(dir.path().join("memory.x"), memory_x_contents).await?;

        Ok(Self { dir })
    }
}

impl BuildSetup for RtLdscriptSetup {
    fn rustc_flags(&self) -> Vec<String> {
        // `link.x` is provided by `cortex-m-rt`
        vec!["-C".to_string(), "link-arg=-Tlink.x".to_string()]
    }

    fn build_envs(&self) -> Vec<(OsString, OsString)> {
        vec![("FARCRI_LINK_SEARCH".into(), self.dir.path().into())]
    }
}

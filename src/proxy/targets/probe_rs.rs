use anyhow::Result;
use futures_core::ready;
use std::{
    convert::TryInto,
    future::Future,
    io::Write,
    mem::replace,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufRead, AsyncRead, AsyncWrite},
    task::{spawn_blocking, JoinHandle},
    time::{delay_for, Delay},
};

use super::{Arch, BuildSetup, CompiledExecutable, DebugProbe, DynAsyncReadWrite, Target};
use crate::utils::Spmc;

#[derive(Debug)]
pub struct NucleoF401re;

impl Target for NucleoF401re {
    fn target_arch(&self) -> Arch {
        Arch::CORTEX_M4F
    }

    fn cargo_features(&self) -> &[&str] {
        &["target_nucleo_f401re"]
    }

    fn prepare_build(&self) -> Pin<Box<dyn Future<Output = Result<Box<dyn BuildSetup>>>>> {
        Box::pin(async {
            match super::ldscript::RtLdscriptSetup::new(
                b"
                MEMORY
                {
                  /* NOTE K = KiBi = 1024 bytes */
                  FLASH : ORIGIN = 0x08000000, LENGTH = 512K
                  RAM : ORIGIN = 0x20000000, LENGTH = 96K
                }

                _stack_start = ORIGIN(RAM) + LENGTH(RAM);
            ",
            )
            .await
            {
                Ok(x) => Ok(Box::new(x) as _),
                Err(x) => Err(x.into()),
            }
        })
    }

    fn connect(&self) -> Pin<Box<dyn Future<Output = Result<Box<dyn DebugProbe>>>>> {
        Box::pin(async {
            spawn_blocking(|| {
                ProbeRsDebugProbe::new("0483:374b".try_into().unwrap(), "stm32f401re".into())
                    .map(|x| Box::new(x) as _)
            })
            .await
            .unwrap()
        })
    }
}

struct ProbeRsDebugProbe {
    session: Arc<Mutex<probe_rs::Session>>,
}

#[derive(thiserror::Error, Debug)]
enum OpenError {
    #[error("Error while opening the probe")]
    OpenProbe(#[source] probe_rs::DebugProbeError),
    #[error("Error while attaching to the probe")]
    Attach(#[source] probe_rs::Error),
}

#[derive(thiserror::Error, Debug)]
enum RunError {
    #[error("Error while flashing the device")]
    Flash(#[source] probe_rs::flashing::FileDownloadError),
    #[error("Error while resetting the device")]
    Reset(#[source] probe_rs::Error),
}

impl ProbeRsDebugProbe {
    fn new(
        probe_sel: probe_rs::DebugProbeSelector,
        target_sel: probe_rs::config::TargetSelector,
    ) -> anyhow::Result<Self> {
        let probe = probe_rs::Probe::open(probe_sel).map_err(OpenError::OpenProbe)?;

        let session = Arc::new(Mutex::new(
            probe.attach(target_sel).map_err(OpenError::Attach)?,
        ));

        Ok(Self { session })
    }
}

impl DebugProbe for ProbeRsDebugProbe {
    fn program_and_get_output(
        &mut self,
        exe: &CompiledExecutable,
    ) -> Pin<Box<dyn Future<Output = Result<DynAsyncReadWrite<'_>>> + '_>> {
        let exe = exe.path.clone();
        let session = Arc::clone(&self.session);

        Box::pin(async move {
            // Flash the executable
            log::info!("Flashing '{0}'", exe.display());

            let session2 = Arc::clone(&session);
            let exe2 = exe.clone();
            spawn_blocking(move || {
                let mut session_lock = session2.lock().unwrap();
                probe_rs::flashing::download_file(
                    &mut *session_lock,
                    &exe2,
                    probe_rs::flashing::Format::Elf,
                )
            })
            .await
            .unwrap()
            .map_err(RunError::Flash)?;

            // Reset the core
            (session.lock().unwrap().core(0))
                .map_err(RunError::Reset)?
                .reset()
                .map_err(RunError::Reset)?;

            // Attach to RTT
            Ok(attach_rtt(session, &exe, Default::default()).await?)
        })
    }
}

const POLL_INTERVAL: Duration = Duration::from_millis(30);
const RTT_ATTACH_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(thiserror::Error, Debug)]
enum AttachRttError {
    #[error("Error while attaching to the RTT channel")]
    AttachRtt(#[source] probe_rs_rtt::Error),
    #[error("Error while halting or resuming the core to access the RTT channel")]
    HaltCore(#[source] probe_rs::Error),
    #[error("Timeout while trying to attach to the RTT channel.")]
    Timeout,
}

#[derive(Default)]
struct RttOptions {
    /// When set to `true`, the core is halted whenever accessing RTT.
    halt_on_access: bool,
}

async fn attach_rtt(
    session: Arc<Mutex<probe_rs::Session>>,
    exe: &Path,
    options: RttOptions,
) -> Result<DynAsyncReadWrite<'static>, AttachRttError> {
    // Read the executable to find the RTT header
    log::debug!(
        "Reading the executable '{0}' to find the RTT header",
        exe.display()
    );
    let rtt_scan_region = match tokio::fs::read(&exe).await {
        Ok(elf_bytes) => {
            let addr = spawn_blocking(move || find_rtt_symbol(&elf_bytes))
                .await
                .unwrap();
            if let Some(x) = addr {
                log::debug!("Found the RTT header at 0x{:x}", x);
                probe_rs_rtt::ScanRegion::Exact(x as u32)
            } else {
                probe_rs_rtt::ScanRegion::Ram
            }
        }
        Err(e) => {
            log::warn!(
                "Couldn't read the executable to find the RTT header: {:?}",
                e
            );
            probe_rs_rtt::ScanRegion::Ram
        }
    };

    // Attach to RTT
    let start = Instant::now();
    let rtt = loop {
        let session = session.clone();
        let halt_on_access = options.halt_on_access;
        let rtt_scan_region = rtt_scan_region.clone();

        let result = spawn_blocking(move || {
            let _halt_guard = if halt_on_access {
                Some(CoreHaltGuard::new(session.clone()).map_err(AttachRttError::HaltCore)?)
            } else {
                None
            };

            match probe_rs_rtt::Rtt::attach_region(session, &rtt_scan_region) {
                Ok(mut rtt) => {
                    if rtt.up_channels().is_empty() || rtt.down_channels().is_empty() {
                        log::trace!(
                            "The up or down chaneel is missing. Seems \
                            like the target needs some time to get ready"
                        );
                        Ok(None)
                    } else {
                        Ok(Some(rtt))
                    }
                }
                Err(probe_rs_rtt::Error::ControlBlockNotFound) => Ok(None),
                Err(e) => Err(AttachRttError::AttachRtt(e)),
            }
        })
        .await
        .unwrap()?;

        if let Some(rtt) = result {
            break rtt;
        }

        if start.elapsed() > RTT_ATTACH_TIMEOUT {
            return Err(AttachRttError::Timeout);
        }

        delay_for(POLL_INTERVAL).await;
    };

    // Stream the output of all up channels
    Ok(Box::pin(ReadWriteRtt::new(session, rtt, options)) as DynAsyncReadWrite<'_>)
}

fn find_rtt_symbol(elf_bytes: &[u8]) -> Option<u64> {
    let elf = match goblin::elf::Elf::parse(elf_bytes) {
        Ok(elf) => elf,
        Err(e) => {
            log::warn!(
                "Couldn't parse the executable to find the RTT header: {:?}",
                e
            );
            return None;
        }
    };

    for sym in &elf.syms {
        if let Some(Ok(name)) = elf.strtab.get(sym.st_name) {
            if name == "_SEGGER_RTT" {
                return Some(sym.st_value);
            }
        }
    }

    None
}

/// Halts the first core while this RAII guard is held.
struct CoreHaltGuard(Arc<Mutex<probe_rs::Session>>);

impl CoreHaltGuard {
    fn new(session: Arc<Mutex<probe_rs::Session>>) -> Result<Self, probe_rs::Error> {
        {
            let mut session = session.lock().unwrap();
            let mut core = session.core(0)?;
            core.halt(std::time::Duration::from_millis(100))?;
        }

        Ok(Self(session))
    }
}

impl Drop for CoreHaltGuard {
    fn drop(&mut self) {
        let mut session = self.0.lock().unwrap();
        let mut core = match session.core(0) {
            Ok(x) => x,
            Err(e) => {
                log::warn!(
                    "Failed to get the core object while restarting the core (ignored): {:?}",
                    e
                );
                return;
            }
        };
        if let Err(e) = core.run() {
            log::warn!("Failed to restart the core (ignored): {:?}", e);
        }
    }
}

struct ReadWriteRtt {
    session: Arc<Mutex<probe_rs::Session>>,
    options: RttOptions,
    st: ReadWriteRttRt,
}

#[derive(Debug)]
struct Bufs {
    read: [u8; 1024],
    read_pos: usize,
    read_len: usize,
    write: [u8; 1024],
    write_pos: usize,
    write_len: usize,
}

#[derive(Debug)]
enum ReadWriteRttRt {
    Idle {
        bufs: Box<Bufs>,
        rtt: Box<probe_rs_rtt::Rtt>,
        /// If an read or write operation gets stuck, it must wait for this
        /// before accessing RTT channels.
        poll_delay: [Option<Delay>; 2],
    },

    /// `ReadWriteRtt` is currently accessing RTT channels.
    Access {
        /// `Spmc` is used to wake up reading and writing tasks both when the
        /// `Future` completes.
        join_handle:
            Spmc<JoinHandle<tokio::io::Result<(Box<Bufs>, [bool; 2], Box<probe_rs_rtt::Rtt>)>>>,
    },

    Invalid,
}

const SPMC_CONSUMER_READ: usize = 0;
const SPMC_CONSUMER_WRITE: usize = 1;
const NUM_SPMC_CONSUMERS: usize = 2;

impl ReadWriteRtt {
    fn new(
        session: Arc<Mutex<probe_rs::Session>>,
        rtt: probe_rs_rtt::Rtt,
        options: RttOptions,
    ) -> Self {
        Self {
            session,
            options,
            st: ReadWriteRttRt::Idle {
                bufs: Box::new(Bufs {
                    read: [0u8; 1024],
                    read_pos: 0,
                    read_len: 0,
                    write: [0u8; 1024],
                    write_pos: 0,
                    write_len: 0,
                }),
                rtt: Box::new(rtt),
                poll_delay: [None, None],
            },
        }
    }
}

impl AsyncRead for ReadWriteRtt {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<tokio::io::Result<usize>> {
        // Naïve implementation of `poll_read` that uses `<Self as AsyncBufRead>`
        let my_buf = match ready!(Pin::as_mut(&mut self).poll_fill_buf(cx)) {
            Ok(x) => x,
            Err(e) => return Poll::Ready(Err(e)),
        };
        let num_bytes_read = my_buf.len().min(buf.len());
        buf[..num_bytes_read].copy_from_slice(&my_buf[..num_bytes_read]);
        Pin::as_mut(&mut self).consume(num_bytes_read);
        Poll::Ready(Ok(num_bytes_read))
    }
}

impl AsyncBufRead for ReadWriteRtt {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<tokio::io::Result<&[u8]>> {
        let this = Pin::into_inner(self);

        loop {
            match &mut this.st {
                ReadWriteRttRt::Idle { bufs, .. } if bufs.read_pos < bufs.read_len => {
                    // We have some data to return.
                    //
                    // Borrow `this.st` again, this time using the full
                    // lifetime of `self`.
                    if let ReadWriteRttRt::Idle { bufs, .. } = &this.st {
                        return Poll::Ready(Ok(&bufs.read[..bufs.read_len][bufs.read_pos..]));
                    } else {
                        unreachable!()
                    }
                }

                ReadWriteRttRt::Idle { poll_delay, .. }
                    if poll_delay[SPMC_CONSUMER_READ].is_some() =>
                {
                    ready!(Pin::new(poll_delay[SPMC_CONSUMER_READ].as_mut().unwrap()).poll(cx));
                    poll_delay[SPMC_CONSUMER_READ] = None;
                }

                // Can't make progress, consult the target RTT.
                _ => {
                    ready!(this.hit_rtt(SPMC_CONSUMER_READ, cx))?;
                }
            }
        }
    }

    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        match &mut self.st {
            ReadWriteRttRt::Idle { bufs, .. } => {
                bufs.read_pos += amt;
                assert!(bufs.read_pos <= bufs.read_len);
            }
            _ => unreachable!(),
        }
    }
}

impl AsyncWrite for ReadWriteRtt {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<tokio::io::Result<usize>> {
        let this = Pin::into_inner(self);

        loop {
            match &mut this.st {
                ReadWriteRttRt::Idle { bufs, .. }
                    if bufs.write_len < bufs.write.len() || bufs.write_pos >= bufs.write_len =>
                {
                    // If there's no bytes left to flush, reset the write pointer.
                    if bufs.write_pos >= bufs.write_len {
                        bufs.write_pos = 0;
                        bufs.write_len = 0;
                    }

                    // Some or all of `buf` can be stored to `bufs.write`.
                    let num_bytes_written = (bufs.write.len() - bufs.write_len).min(buf.len());
                    bufs.write[bufs.write_len..][..num_bytes_written]
                        .copy_from_slice(&buf[..num_bytes_written]);
                    bufs.write_len += num_bytes_written;

                    return Poll::Ready(Ok(num_bytes_written));
                }

                ReadWriteRttRt::Idle { poll_delay, .. }
                    if poll_delay[SPMC_CONSUMER_WRITE].is_some() =>
                {
                    ready!(Pin::new(poll_delay[SPMC_CONSUMER_WRITE].as_mut().unwrap()).poll(cx));
                    poll_delay[SPMC_CONSUMER_WRITE] = None;
                }

                // Can't make progress, consult the target RTT.
                _ => {
                    ready!(this.hit_rtt(SPMC_CONSUMER_WRITE, cx))?;
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<tokio::io::Result<()>> {
        let this = Pin::into_inner(self);

        loop {
            match &mut this.st {
                ReadWriteRttRt::Idle { bufs, .. } if bufs.write_pos >= bufs.write_len => {
                    // The write buffer is empty.
                    bufs.write_pos = 0;
                    bufs.write_len = 0;
                    return Poll::Ready(Ok(()));
                }

                ReadWriteRttRt::Idle { poll_delay, .. }
                    if poll_delay[SPMC_CONSUMER_WRITE].is_some() =>
                {
                    ready!(Pin::new(poll_delay[SPMC_CONSUMER_WRITE].as_mut().unwrap()).poll(cx));
                    poll_delay[SPMC_CONSUMER_WRITE] = None;
                }

                // Can't make progress, consult the target RTT.
                _ => {
                    ready!(this.hit_rtt(SPMC_CONSUMER_WRITE, cx))?;
                }
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<tokio::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl ReadWriteRtt {
    /// If the current state is `Idle`, start accessing RTT channels. In other
    /// states, poll the state of the access initiated by this method.
    ///
    /// A return value of `Ready(Ok(()))` indicates that some progress was made
    /// and `self.st` should be checked again.
    fn hit_rtt(
        &mut self,
        consumer_index: usize,
        cx: &mut Context<'_>,
    ) -> Poll<tokio::io::Result<()>> {
        match &mut self.st {
            ReadWriteRttRt::Idle { bufs, .. } => {
                // Start accessing RTT channels
                let (mut bufs, mut rtt) = match replace(&mut self.st, ReadWriteRttRt::Invalid) {
                    ReadWriteRttRt::Idle { bufs, rtt, .. } => (bufs, rtt),
                    _ => unreachable!(),
                };

                let halt_on_access = self.options.halt_on_access;
                let session = self.session.clone();

                // Accessing RTT is a blocking operation, so do it in a
                // separate thread
                let join_handle = spawn_blocking(move || {
                    let stalled =
                        Self::hit_rtt_inner(session, &mut rtt, &mut *bufs, halt_on_access)?;

                    // Send the buffer back to the `ReadWriteRtt`
                    Ok((bufs, stalled, rtt))
                });

                let join_handle = Spmc::new(NUM_SPMC_CONSUMERS, join_handle);

                self.st = ReadWriteRttRt::Access { join_handle };
            }

            ReadWriteRttRt::Access { join_handle } => {
                let (bufs, stalled, rtt) =
                    match ready!(join_handle.poll(consumer_index, cx)).unwrap() {
                        Ok(x) => x,
                        Err(e) => return Poll::Ready(Err(e)),
                    };

                let mut poll_delay = [None, None];

                for (stalled, poll_delay) in stalled.iter().zip(poll_delay.iter_mut()) {
                    if *stalled {
                        // Delay the next operation in this direction because the target
                        // will probably need some time before emptying the buffer
                        *poll_delay = Some(delay_for(POLL_INTERVAL));
                    }
                }

                self.st = ReadWriteRttRt::Idle {
                    bufs,
                    rtt,
                    poll_delay,
                };
            }

            ReadWriteRttRt::Invalid => unreachable!(),
        }

        Poll::Ready(Ok(()))
    }

    /// Returns a "stalled" flag indicating whether no progress could be made
    /// because of the target's lack of activity.
    fn hit_rtt_inner(
        session: Arc<Mutex<probe_rs::Session>>,
        rtt: &mut probe_rs_rtt::Rtt,
        bufs: &mut Bufs,
        halt_on_access: bool,
    ) -> tokio::io::Result<[bool; 2]> {
        let _halt_guard = if halt_on_access {
            Some(
                CoreHaltGuard::new(session)
                    .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::Other, e))?,
            )
        } else {
            None
        };

        let mut stalled = [false; 2];

        if bufs.read_pos >= bufs.read_len {
            // The read pointer caught up
            bufs.read_pos = 0;
            bufs.read_len = 0;
        }

        // Copy the up channels' received bytes to `bufs.read`
        for (i, channel) in rtt.up_channels().iter().enumerate() {
            let buf = &mut bufs.read[bufs.read_len..];
            if buf.is_empty() {
                break;
            }

            let num_ch_read_bytes = channel
                .read(buf)
                .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::Other, e))?;

            if num_ch_read_bytes != 0 {
                log::trace!(
                    "Read {:?} ({} bytes) from {:?}",
                    &buf[..num_ch_read_bytes],
                    buf.len(),
                    (channel.number(), channel.name()),
                );

                if i == 1 {
                    // Terminal channel - send it to `ReadWriteRtt`.
                    // Don't bother checking other channels because we don't
                    // want `buf` to be overwritten with a log channel's payload.
                    bufs.read_len += num_ch_read_bytes;
                    break;
                } else {
                    // Log channel - send it to stdout
                    // (Yes, it piggybacks upon the terminal channel's read buffer)
                    std::io::stdout()
                        .write_all(&buf[..num_ch_read_bytes])
                        .unwrap();
                }
            } else if i == 0 {
                stalled[SPMC_CONSUMER_READ] = true;
            }
        }

        // Send bytes from `bufs.write` to the first down channel
        let buf = &bufs.write[bufs.write_pos..bufs.write_len];
        if !buf.is_empty() {
            if let Some(channel) = rtt.down_channels().iter().next() {
                let num_ch_written_bytes = channel
                    .write(buf)
                    .map_err(|e| tokio::io::Error::new(tokio::io::ErrorKind::Other, e))?;

                if num_ch_written_bytes != 0 {
                    log::trace!(
                        "Wrote {:?} ({} bytes) to {:?}",
                        &buf[..num_ch_written_bytes],
                        buf.len(),
                        (channel.number(), channel.name()),
                    );
                    bufs.write_pos += num_ch_written_bytes;
                }

                stalled[SPMC_CONSUMER_WRITE] = bufs.write_pos < bufs.write_len;
            } else {
                log::trace!(
                    "No RTT down channels available; dropping {:?} ({} bytes)",
                    String::from_utf8_lossy(buf),
                    buf.len()
                );
                bufs.write_pos = bufs.write_len;
            }
        }

        Ok(stalled)
    }
}

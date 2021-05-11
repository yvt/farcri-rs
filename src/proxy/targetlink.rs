use anyhow::{bail, Context, Result};
use futures::future;
use rand::Rng;
use std::pin::Pin;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadHalf, WriteHalf},
    sync::oneshot,
    time::{self, Duration},
};

use crate::{bencher::protocol, utils::async_buf_read_skip_until_pattern};

mod slip;

pub(super) struct TargetLink<Stream> {
    reader: BufReader<ReadHalf<Stream>>,
    writer: WriteHalf<Stream>,
}

impl<Stream: AsyncRead + AsyncWrite> TargetLink<Stream> {
    pub(super) async fn new(stream: Stream) -> Result<Self> {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut reader = BufReader::with_capacity(8192, reader);

        // Handshake stage 1 synchronizes the states of two peers and informs
        // them of packet boundaries.
        // Implemented by two concurrent processes:
        //   Process 1: Read until `handshake_packet` is found in the
        //              read bytes
        //   Process 2: Send `handshake_packet` repeatedly until Process 1
        //              completes.
        log::debug!("Performing the handshake stage 1");
        let mut nonce: [u8; protocol::HANDSHAKE_NONCE_LEN] = rand::thread_rng().gen();
        for x in nonce.iter_mut() {
            if *x == protocol::HANDSHAKE_MAGIC[0] || *x == protocol::HANDSHAKE_END_MAGIC[0] {
                *x = 255;
                assert!(
                    *x != protocol::HANDSHAKE_MAGIC[0] && *x != protocol::HANDSHAKE_END_MAGIC[0]
                );
            }
        }
        let mut handshake_packet = protocol::HANDSHAKE_MAGIC.to_owned();
        handshake_packet.extend_from_slice(&nonce);
        log::trace!("handshake_packet = {:?}", handshake_packet);
        {
            let (p2_abort_send, mut p2_abort_recv) = oneshot::channel();
            let p1 = time::timeout(Duration::from_secs(10), async {
                let found =
                    async_buf_read_skip_until_pattern(Pin::new(&mut reader), &handshake_packet)
                        .await
                        .context("Read failed while waiting for a handshake response.")?;

                if !found {
                    bail!("Unexpected EOF encountered while waiting for a handshake response.");
                }

                Ok::<(), anyhow::Error>(())
            });
            let p2 = async {
                loop {
                    // Send a handshake packet. Complete the submission and never
                    // send a partial packet even if we are interrupted by
                    // `p2_abort_recv`.
                    log::trace!("Sending a handshake request");
                    Pin::new(&mut writer)
                        .write_all(&handshake_packet)
                        .await
                        .context("Failed to send a handshake request")?;
                    Pin::new(&mut writer)
                        .flush()
                        .await
                        .context("Failed to send a handshake request")?;

                    if time::timeout(Duration::from_millis(200), &mut p2_abort_recv)
                        .await
                        .is_ok()
                    {
                        // Abort requested
                        log::trace!("Stopping sending a handshake request");
                        return Ok::<(), anyhow::Error>(());
                    } else {
                        // Try again
                    }
                }
            };
            tokio::pin!(p2);

            // Wait until Process 1 completes while keeping Process 2 running
            tokio::select! {
                result = p1 => {
                    // Result<Result<(), anyhow::Error>, time::Elapsed>
                    //                   ^^^^^^^^^^^^^   ^^^^^^^^^^^^^
                    //                   The second `?`  `context` and
                    //                                   the first `?`
                    result.context("Timed out while waiting for a handshake response.")??;
                }
                result = &mut p2 => {
                    // At this point, Process 2 can complete only because of an
                    // I/O error.
                    result?;
                    unreachable!();
                }
            }

            // Abort Process 2 gracefully. Don't drop it abruptly as doing so
            // can result in sending an incomplete handshake request.
            let _ = p2_abort_send.send(());
            p2.await?;
        }

        // Handshake phase 2 drops excessive handshake response packets,
        // completely synchronizing both peers.
        // Implemented by two concurrent processes:
        //   Process 1: Read until `handshake_packet` or `HANDSHAKE_END_MAGIC`
        //              is found in the read bytes
        //   Process 2: Send `HANDSHAKE_END_MAGIC` once.
        log::debug!("Performing the handshake stage 2");
        let p1 = async {
            loop {
                let mut buf = vec![
                    0u8;
                    handshake_packet
                        .len()
                        .max(protocol::HANDSHAKE_END_MAGIC.len())
                ];
                Pin::new(&mut reader)
                    .read_exact(&mut buf[..1])
                    .await
                    .context("Read failed while waiting for a handshake end response.")?;

                const HANDSHAKE_MAGIC0: u8 = protocol::HANDSHAKE_MAGIC[0];
                const HANDSHAKE_END_MAGIC0: u8 = protocol::HANDSHAKE_END_MAGIC[0];

                match buf[0] {
                    HANDSHAKE_MAGIC0 => {
                        // Complete reading `handshake_packet`
                        Pin::new(&mut reader)
                            .read_exact(&mut buf[1..handshake_packet.len()])
                            .await
                            .context("Failed to read a handshake response.")?;
                    }
                    HANDSHAKE_END_MAGIC0 => {
                        // Complete reading `HANDSHAKE_END_MAGIC`
                        Pin::new(&mut reader)
                            .read_exact(&mut buf[1..protocol::HANDSHAKE_END_MAGIC.len()])
                            .await
                            .context("Failed to read a handshake end response.")?;
                        return Ok::<(), anyhow::Error>(());
                    }
                    _ => {
                        bail!("Unexpected handshake end response byte: {}", buf[0]);
                    }
                }
            }
        };
        let p2 = async {
            log::trace!("Sending a handshake end request");
            Pin::new(&mut writer)
                .write_all(&protocol::HANDSHAKE_END_MAGIC)
                .await
                .context("Failed to send a handshake end request.")?;

            Pin::new(&mut writer)
                .flush()
                .await
                .context("Failed to send a handshake end request")?;

            Ok(())
        };
        time::timeout(Duration::from_secs(10), future::try_join(p1, p2))
            .await
            .context("Timed out while waiting for handshake completion.")??;

        Ok(Self { reader, writer })
    }

    pub(super) async fn recv(&mut self) -> Result<protocol::UpstreamMessage<String, Vec<u64>>> {
        let frame = slip::read_frame(&mut self.reader).await?;
        log::trace!("Received a SLIP frame {:?}", frame);
        let msg = serde_cbor::from_slice(&frame)
            .context("Failed to parse the received UpstreamMessage packet.")?;
        log::debug!("recv: {:?}", msg);
        Ok(msg)
    }

    pub(super) async fn send(&mut self, msg: &protocol::DownstreamMessage<String>) -> Result<()> {
        log::debug!("send: {:?}", msg);
        let frame = serde_cbor::to_vec(msg).unwrap();
        log::trace!("Sending a SLIP frame {:?}", frame);
        slip::write_frame(&mut self.writer, &frame).await?;
        Ok(())
    }
}

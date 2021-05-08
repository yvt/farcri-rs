//! Dumb (text-only) front-end, used when cargo-criterion is unavailable
use anyhow::Result;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time,
};

use crate::{bencher::protocol, proxy::targetlink::TargetLink};

pub(super) async fn run_frontend(
    mut target_link: TargetLink<impl AsyncRead + AsyncWrite>,
) -> Result<()> {
    let origin = std::time::Instant::now();

    loop {
        let msg = time::timeout(time::Duration::from_secs(20), target_link.recv())
            .await
            .map_err(|_| anyhow::anyhow!("Timed out while waiting for a message."))??;

        if let protocol::UpstreamMessage::GetInstant = msg {
            let instant = protocol::Instant::from_nanos(origin.elapsed().as_nanos() as u64);
            target_link
                .send(&protocol::DownstreamMessage::Instant(instant))
                .await?;
            continue;
        }

        // TODO: Do better
        log::info!("{:?}", msg);

        if let protocol::UpstreamMessage::MeasurementComplete { .. } = msg {
            target_link
                .send(&protocol::DownstreamMessage::Continue)
                .await?;
        }

        if let protocol::UpstreamMessage::End = msg {
            break;
        }
    }

    Ok(())
}

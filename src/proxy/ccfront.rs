//! [cargo-criterion] front-end
//!
//! [cargo-criterion]: https://github.com/bheisler/cargo-criterion
use anyhow::{bail, Context, Result};
use std::convert::TryFrom;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufStream},
    net::TcpStream,
    time,
};

use crate::{bencher::protocol, proxy::targetlink::TargetLink};

mod ccprotocol;

pub(super) async fn run_frontend(
    mut target_link: TargetLink<impl AsyncRead + AsyncWrite>,
    mut cc_stream: TcpStream,
) -> Result<()> {
    let mut cc_link = CcLink::new(cc_stream).await?;

    // Start proxying messages
    let origin = std::time::Instant::now();
    let mut current_group = None;
    let mut current_benchmark = None;
    loop {
        // Read from target
        let msg = time::timeout(time::Duration::from_secs(20), target_link.recv())
            .await
            .map_err(|_| anyhow::anyhow!("Timed out while waiting for a downstream message."))??;

        match msg {
            protocol::UpstreamMessage::GetInstant => {
                let instant = protocol::Instant::from_nanos(origin.elapsed().as_nanos() as u64);
                target_link
                    .send(&protocol::DownstreamMessage::Instant(instant))
                    .await?;
                continue;
            }

            protocol::UpstreamMessage::End => {
                break;
            }

            protocol::UpstreamMessage::BeginningBenchmarkGroup { group } => {
                cc_link
                    .send(&ccprotocol::OutgoingMessage::BeginningBenchmarkGroup { group: &group })
                    .await?;

                assert!(current_group.is_none());
                current_group = Some(group);
            }

            protocol::UpstreamMessage::FinishedBenchmarkGroup => {
                cc_link
                    .send(&ccprotocol::OutgoingMessage::FinishedBenchmarkGroup {
                        group: &current_group.take().unwrap(),
                    })
                    .await?;

                serve_value_formatter(&mut cc_link).await?;
                target_link
                    .send(&protocol::DownstreamMessage::Continue)
                    .await?;
            }
            protocol::UpstreamMessage::BeginningBenchmark { id } => {
                let id = ccprotocol::RawBenchmarkId::from(&id);

                cc_link
                    .send(&ccprotocol::OutgoingMessage::BeginningBenchmark { id: id.clone() })
                    .await?;

                assert!(current_benchmark.is_none());
                current_benchmark = Some(id);
            }
            protocol::UpstreamMessage::SkippingBenchmark { id } => {
                cc_link
                    .send(&ccprotocol::OutgoingMessage::SkippingBenchmark { id: (&id).into() })
                    .await?;
            }
            protocol::UpstreamMessage::Warmup {
                warm_up_goal_duration,
            } => {
                cc_link
                    .send(&ccprotocol::OutgoingMessage::Warmup {
                        id: current_benchmark.clone().unwrap(),
                        nanos: warm_up_goal_duration.as_nanos() as f64,
                    })
                    .await?;
            }
            protocol::UpstreamMessage::MeasurementStart {
                warm_up_iter_count,
                warm_up_duration,
                num_samples,
                num_iters,
            } => {
                let ns_per_iter = warm_up_duration.as_nanos() as f64 / warm_up_iter_count as f64;
                let estimate_ns = ns_per_iter * num_iters as f64;
                cc_link
                    .send(&ccprotocol::OutgoingMessage::MeasurementStart {
                        id: current_benchmark.clone().unwrap(),
                        sample_count: num_samples as u64,
                        estimate_ns,
                        iter_count: (num_samples as u64).saturating_mul(num_iters),
                    })
                    .await?;
            }
            protocol::UpstreamMessage::MeasurementComplete {
                num_iters_per_sample,
                values,
                benchmark_config,
            } => {
                let iters = vec![num_iters_per_sample as f64; values.len()];
                let times: Vec<_> = values.iter().map(|&x| x as f64).collect();
                let plot_config = ccprotocol::PlotConfiguration {
                    summary_scale: ccprotocol::AxisScale::Linear,
                };

                cc_link
                    .send(&ccprotocol::OutgoingMessage::MeasurementComplete {
                        id: current_benchmark.take().unwrap(),
                        iters: &iters,
                        times: &times,
                        plot_config,
                        sampling_method: ccprotocol::SamplingMethod::Flat,
                        benchmark_config: (&benchmark_config).into(),
                    })
                    .await?;

                serve_value_formatter(&mut cc_link).await?;
                target_link
                    .send(&protocol::DownstreamMessage::Continue)
                    .await?;
            }
        }
    }

    Ok(())
}

async fn serve_value_formatter(cc_link: &mut CcLink) -> Result<()> {
    use super::formatter::ValueFormatter;
    let formatter = super::formatter::CyclesFormatter;

    loop {
        let response = match cc_link.recv().await? {
            ccprotocol::IncomingMessage::FormatValue { value } => {
                ccprotocol::OutgoingMessage::FormattedValue {
                    value: formatter.format_value(value),
                }
            }
            ccprotocol::IncomingMessage::FormatThroughput { value, throughput } => {
                ccprotocol::OutgoingMessage::FormattedValue {
                    value: formatter.format_throughput(&throughput, value),
                }
            }
            ccprotocol::IncomingMessage::ScaleValues {
                typical_value,
                mut values,
            } => {
                let unit = formatter.scale_values(typical_value, &mut values);
                ccprotocol::OutgoingMessage::ScaledValues {
                    unit,
                    scaled_values: values,
                }
            }
            ccprotocol::IncomingMessage::ScaleThroughputs {
                typical_value,
                throughput,
                mut values,
            } => {
                let unit = formatter.scale_throughputs(typical_value, &throughput, &mut values);
                ccprotocol::OutgoingMessage::ScaledValues {
                    unit,
                    scaled_values: values,
                }
            }
            ccprotocol::IncomingMessage::ScaleForMachines { mut values } => {
                let unit = formatter.scale_for_machines(&mut values);
                ccprotocol::OutgoingMessage::ScaledValues {
                    unit,
                    scaled_values: values,
                }
            }
            ccprotocol::IncomingMessage::Continue => break,
            _ => panic!(),
        };

        cc_link.send(&response).await?;
    }

    Ok(())
}

struct CcLink {
    cc_stream: BufStream<TcpStream>,
    receive_buffer: Vec<u8>,
    send_buffer: Vec<u8>,
}

impl CcLink {
    async fn new(cc_stream: TcpStream) -> Result<Self> {
        let mut cc_stream = BufStream::new(cc_stream);

        // read the runner-hello
        let mut hello_buf = [0u8; ccprotocol::RUNNER_HELLO_SIZE];
        cc_stream
            .read_exact(&mut hello_buf)
            .await
            .context("Failed to read the runner-hello.")?;
        log::trace!("Got runner-hello: {:?}", hello_buf);
        if &hello_buf[0..ccprotocol::RUNNER_MAGIC_NUMBER.len()]
            != ccprotocol::RUNNER_MAGIC_NUMBER.as_bytes()
        {
            bail!("Not connected to cargo-criterion.");
        }
        let i = ccprotocol::RUNNER_MAGIC_NUMBER.len();
        let runner_version = [hello_buf[i], hello_buf[i + 1], hello_buf[i + 2]];

        log::info!("Runner version: {:?}", runner_version);

        // now send the benchmark-hello
        let mut hello_buf = [0u8; ccprotocol::BENCHMARK_HELLO_SIZE];
        hello_buf[0..ccprotocol::BENCHMARK_MAGIC_NUMBER.len()]
            .copy_from_slice(ccprotocol::BENCHMARK_MAGIC_NUMBER.as_bytes());
        let mut i = ccprotocol::BENCHMARK_MAGIC_NUMBER.len();
        hello_buf[i] = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap();
        hello_buf[i + 1] = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap();
        hello_buf[i + 2] = env!("CARGO_PKG_VERSION_PATCH").parse().unwrap();
        i += 3;
        hello_buf[i..i + 2].clone_from_slice(&ccprotocol::PROTOCOL_VERSION.to_be_bytes());
        i += 2;
        hello_buf[i..i + 2].clone_from_slice(&ccprotocol::PROTOCOL_FORMAT.to_be_bytes());

        log::trace!("Sending benchmark-hello: {:?}", hello_buf);
        cc_stream
            .write_all(&hello_buf)
            .await
            .context("Failed to send the benchmark-hello.")?;

        cc_stream
            .flush()
            .await
            .context("Failed to send the benchmark-hello.")?;

        Ok(Self {
            cc_stream,
            receive_buffer: Vec::new(),
            send_buffer: Vec::new(),
        })
    }

    async fn recv(&mut self) -> Result<ccprotocol::IncomingMessage> {
        let mut length_buf = [0u8; 4];
        self.cc_stream.read_exact(&mut length_buf).await?;
        let length = u32::from_be_bytes(length_buf);
        self.receive_buffer.resize(length as usize, 0u8);
        self.cc_stream.read_exact(&mut self.receive_buffer).await?;
        let value = serde_cbor::from_slice(&self.receive_buffer)
            .context("Failed to decode the received upstream message.")?;
        log::debug!("recv: {:?}", value);
        Ok(value)
    }

    async fn send(&mut self, message: &ccprotocol::OutgoingMessage<'_>) -> Result<()> {
        log::debug!("send: {:?}", message);
        self.send_buffer.truncate(0);
        serde_cbor::to_writer(&mut self.send_buffer, message)?;
        let size = u32::try_from(self.send_buffer.len()).unwrap();
        let length_buf = size.to_be_bytes();
        self.cc_stream.write_all(&length_buf).await?;
        self.cc_stream.write_all(&self.send_buffer).await?;
        self.cc_stream.flush().await?;
        Ok(())
    }
}

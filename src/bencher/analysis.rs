use super::{func::Function, measurement, protocol, ValueBuf};

pub(super) fn common(
    id: &protocol::RawBenchmarkId<&str>,
    routine: &mut Function<'_>,
    config: &protocol::BenchmarkConfig,
    out_values: &mut ValueBuf,
    measurement: measurement::Measurement<'_>,
) {
    log::info!("Benchmarking {}", id);

    let (num_iters_per_sample, mut measurement) = routine.sample(measurement, config, out_values);

    measurement
        .link()
        .send(&protocol::UpstreamMessage::MeasurementComplete {
            num_iters_per_sample,
            values: &out_values[..],
            benchmark_config: config.clone(),
        });
}

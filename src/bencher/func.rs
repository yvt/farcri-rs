use super::{measurement, protocol, Bencher, ValueBuf};

pub struct Function<'a> {
    f: &'a mut (dyn FnMut(&mut Bencher<'_>) + 'a),
}

impl<'a> Function<'a> {
    pub fn new(f: &'a mut (dyn FnMut(&mut Bencher<'_>) + 'a)) -> Function {
        Function { f }
    }
}

impl Function<'_> {
    pub(super) fn bench<'link>(
        &mut self,
        measurement: measurement::Measurement<'link>,
        iters_per_sample: u64,
        out_values: &mut [u64],
    ) -> measurement::Measurement<'link> {
        let f = &mut self.f;

        let mut b = Bencher {
            iterated: false,
            iters: iters_per_sample,
            value: Default::default(),
            measurement,
            elapsed_time: Default::default(),
            wants_elapsed_time: false,
        };

        for out_value in out_values.iter_mut() {
            (*f)(&mut b);
            b.assert_iterated();
            *out_value = b.value;
        }

        b.measurement
    }

    pub(super) fn warm_up<'link>(
        &mut self,
        measurement: measurement::Measurement<'link>,
        how_long: measurement::Duration,
    ) -> (measurement::Duration, u64, measurement::Measurement<'link>) {
        let f = &mut self.f;
        let mut b = Bencher {
            iterated: false,
            iters: 1,
            value: Default::default(),
            measurement,
            elapsed_time: Default::default(),
            wants_elapsed_time: true,
        };

        let mut total_iters = 0;
        let mut elapsed_time = protocol::Duration::default();
        loop {
            (*f)(&mut b);

            b.assert_iterated();

            total_iters += b.iters;
            elapsed_time += b.elapsed_time;
            if elapsed_time > how_long {
                return (elapsed_time, total_iters, b.measurement);
            }

            b.iters = b.iters.wrapping_mul(2);
        }
    }

    pub(super) fn sample<'link>(
        &mut self,
        mut measurement: measurement::Measurement<'link>,
        config: &protocol::BenchmarkConfig,
        out_durations: &mut ValueBuf,
    ) -> (u64, measurement::Measurement<'link>) {
        let warm_up_time = config.warm_up_time;
        let measurement_time = config.measurement_time;
        let num_samples = config.sample_size.min(out_durations.capacity()).max(1);

        log::debug!("Warm up (warm_up_time = {}) is in progress", warm_up_time);

        measurement.link().send(&protocol::UpstreamMessage::Warmup {
            warm_up_goal_duration: warm_up_time,
        });

        let (wu_elapsed, wu_iters, mut measurement) = self.warm_up(measurement, warm_up_time);
        log::debug!("Completed {} iteration(s) in {}", wu_iters, wu_elapsed);

        // Calculate the required number of samples for measurement
        //
        // This is akin to the `Flat` sampling mode from Criterion.rs. `Linear`
        // is more complicated, and I'm not willing to implement it in
        // constrained systems that FarCri.rs targets.
        let num_iters = wu_iters as u128 * measurement_time.as_nanos() as u128
            / warm_up_time.as_nanos() as u128;
        let num_iters_per_sample = (num_iters / config.sample_size as u128).max(1) as u64;
        let num_iters = num_iters_per_sample
            .checked_mul(num_samples as _)
            .expect("oops, the iteration count overflowed!");

        log::debug!(
            "Measuring, {} samples, {} iterations/sample",
            num_samples,
            num_iters_per_sample
        );

        // TODO: we should avoid sending packets here for architectural layer separation
        measurement
            .link()
            .send(&protocol::UpstreamMessage::MeasurementStart {
                warm_up_duration: wu_elapsed,
                warm_up_iter_count: wu_iters,
                num_samples,
                num_iters,
            });

        // `ArrayVec::resize` is missing <https://github.com/bluss/arrayvec/issues/72>
        while out_durations.len() < num_samples {
            out_durations.push(Default::default());
        }
        while out_durations.len() > num_samples {
            out_durations.pop();
        }
        let out_durations = &mut out_durations[..num_samples];

        let measurement = self.bench(measurement, num_iters_per_sample, out_durations);

        (num_iters_per_sample, measurement)
    }
}

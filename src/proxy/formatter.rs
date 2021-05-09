use crate::bencher::protocol::Throughput;

/// Trait providing functions to format measured values to string so that they can be displayed on
/// the command line or in the reports. The functions of this trait take measured values in f64
/// form; implementors can assume that the values are of the same scale as those produced by the
/// associated [MeasuredValue](trait.MeasuredValue.html) (eg. if your measurement produces values in
/// nanoseconds, the values passed to the formatter will be in nanoseconds).
///
/// Implementors are encouraged to format the values in a way that is intuitive for humans and
/// uses the SI prefix system. For example, the format used by [WallTime](struct.WallTime.html)
/// can display the value in units ranging from picoseconds to seconds depending on the magnitude
/// of the elapsed time in nanoseconds.
pub(crate) trait ValueFormatter {
    /// Format the value (with appropriate unit) and return it as a string.
    fn format_value(&self, value: f64) -> String {
        let mut values = [value];
        let unit = self.scale_values(value, &mut values);
        format!("{:>6} {}", short(values[0]), unit)
    }

    /// Format the value as a throughput measurement. The value represents the measurement value;
    /// the implementor will have to calculate bytes per second, iterations per cycle, etc.
    fn format_throughput(&self, throughput: &Throughput, value: f64) -> String {
        let mut values = [value];
        let unit = self.scale_throughputs(value, throughput, &mut values);
        format!("{:>6} {}", short(values[0]), unit)
    }

    /// Scale the given values to some appropriate unit and return the unit string.
    ///
    /// The given typical value should be used to choose the unit. This function may be called
    /// multiple times with different datasets; the typical value will remain the same to ensure
    /// that the units remain consistent within a graph. The typical value will not be NaN.
    /// Values will not contain NaN as input, and the transformed values must not contain NaN.
    fn scale_values(&self, typical_value: f64, values: &mut [f64]) -> &'static str;

    /// Convert the given measured values into throughput numbers based on the given throughput
    /// value, scale them to some appropriate unit, and return the unit string.
    ///
    /// The given typical value should be used to choose the unit. This function may be called
    /// multiple times with different datasets; the typical value will remain the same to ensure
    /// that the units remain consistent within a graph. The typical value will not be NaN.
    /// Values will not contain NaN as input, and the transformed values must not contain NaN.
    fn scale_throughputs(
        &self,
        typical_value: f64,
        throughput: &Throughput,
        values: &mut [f64],
    ) -> &'static str;

    /// Scale the values and return a unit string designed for machines.
    ///
    /// For example, this is used for the CSV file output. Implementations should modify the given
    /// values slice to apply the desired scaling (if any) and return a string representing the unit
    /// the modified values are in.
    fn scale_for_machines(&self, values: &mut [f64]) -> &'static str;
}

pub(crate) struct CyclesFormatter;

impl CyclesFormatter {
    fn cycles_per_byte(&self, bytes: f64, typical: f64, values: &mut [f64]) -> &'static str {
        let cycles_per_byte = typical / bytes;
        let (denominator, unit) = if cycles_per_byte < 1000.0 {
            (1.0, "  cycles/B")
        } else if cycles_per_byte < 1000.0 * 1000.0 {
            (1000.0, "Kcycles/B")
        } else if cycles_per_byte < 1000.0 * 1000.0 * 1000.0 {
            (1000.0 * 1000.0, "Mcycles/B")
        } else {
            (1000.0 * 1000.0 * 1000.0, "Gcycles/B")
        };

        for val in values {
            let cycles_per_byte = *val / bytes;
            *val = cycles_per_byte / denominator;
        }

        unit
    }

    fn cycles_per_element(&self, elems: f64, typical: f64, values: &mut [f64]) -> &'static str {
        let cycles_per_element = typical / elems;
        let (denominator, unit) = if cycles_per_element < 1000.0 {
            (1.0, " cycles/elem")
        } else if cycles_per_element < 1000.0 * 1000.0 {
            (1000.0, "Kcycles/elem")
        } else if cycles_per_element < 1000.0 * 1000.0 * 1000.0 {
            (1000.0 * 1000.0, "Mcycles/elem")
        } else {
            (1000.0 * 1000.0 * 1000.0, "Gcycles/elem")
        };

        for val in values {
            let cycles_per_element = *val / elems;
            *val = cycles_per_element / denominator;
        }

        unit
    }
}

impl ValueFormatter for CyclesFormatter {
    fn scale_throughputs(
        &self,
        typical: f64,
        throughput: &Throughput,
        values: &mut [f64],
    ) -> &'static str {
        match *throughput {
            Throughput::Bytes(bytes) => self.cycles_per_byte(bytes as f64, typical, values),
            Throughput::Elements(elems) => self.cycles_per_element(elems as f64, typical, values),
        }
    }

    fn scale_values(&self, typical_value: f64, values: &mut [f64]) -> &'static str {
        let (factor, unit) = if typical_value < 10f64.powi(3) {
            (10f64.powi(0), "cycles")
        } else if typical_value < 10f64.powi(6) {
            (10f64.powi(-3), "Kcycles")
        } else if typical_value < 10f64.powi(9) {
            (10f64.powi(-6), "Mcycles")
        } else {
            (10f64.powi(-9), "Gcycles")
        };

        for val in values {
            *val *= factor;
        }

        unit
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str {
        // no scaling is needed
        "cycles"
    }
}

fn short(n: f64) -> String {
    if n < 10.0 {
        format!("{:.4}", n)
    } else if n < 100.0 {
        format!("{:.3}", n)
    } else if n < 1000.0 {
        format!("{:.2}", n)
    } else if n < 10000.0 {
        format!("{:.1}", n)
    } else {
        format!("{:.0}", n)
    }
}

<img src="doc/banner.jpg" alt="FarCri.rs">

<p align="center">
FarCri.rs: Criterion.rs for Remote Targets
</p>
<p align="center">
<a href="https://docs.rs/farcri/"><img src="https://docs.rs/farcri/badge.svg" alt="docs.rs"></a> <a href="https://crates.io/crates/farcri"><img src="https://img.shields.io/crates/v/farcri"></a> <img src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue">
</p>

FarCri\.rs is a benchmark framework for constrained systems (e.g., microcontrollers) based on [Criterion.rs]. It includes a shrunken-down version of Criterion\.rs's measurement code that runs on a target system and a host program that mediates the communication between the target system and the [cargo-criterion] frontend.

[Criterion.rs]: https://github.com/bheisler/criterion.rs
[cargo-criterion]: https://github.com/bheisler/cargo-criterion

- [x] Basic measurement
- [x] Integration with cargo-criterion
- [ ] Custom values (e.g., performance counters)
- [ ] `Linear` sampling method
- [ ] `std` target
- [ ] Binary size measurement
- [ ] Passing custom Cargo features

## Example

`benches/example.rs`:

```rust
#![no_std]
#![cfg_attr(target_os = "none", no_main)]

use farcri::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("expensive_calculation", |b| b.iter(|| {
        /* do expensive calculation */
    }));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
```

`Cargo.toml`:

```toml
  ⋮

[dev-dependencies.farcri]
git = "https://github.com/yvt/farcri-rs.git"
rev = "..."

[[bench]]
name = "example"
harness = false
```

## Try it

*Prerequisites:* a [NUCLEO-F401RE] development board, Rust 1.51.0 or newer, libusb1, and [cargo-criterion]

```
$ env FARCRI_TARGET=nucleo_f401re cargo bench -p farcri_example --plotting-backend gnuplot
[⋯ INFO  farcri::proxy::targets::probe_rs] Flashing '⋯/farcri-rs/target/thumbv7em-none-eabihf/release/deps/sort-8f14de0564ff7f2f'
       ⋮
noop                    time:   [10.005 cycles 10.005 cycles 10.006 cycles]

sort [i32]/1            time:   [71.021 cycles 71.022 cycles 71.023 cycles]
                        thrpt:  [71.023  cycles/elem 71.022  cycles/elem 71.021  cycles/elem]

sort [i32]/4            time:   [188.04 cycles 188.04 cycles 188.05 cycles]
                        thrpt:  [47.012  cycles/elem 47.011  cycles/elem 47.011  cycles/elem]

sort [i32]/16           time:   [1.4423 Kcycles 1.4424 Kcycles 1.4424 Kcycles]
                        thrpt:  [90.148  cycles/elem 90.147  cycles/elem 90.146  cycles/elem]

sort [i32]/64           time:   [1.1193 Kcycles 1.1194 Kcycles 1.1194 Kcycles]
                        thrpt:  [17.490  cycles/elem 17.490  cycles/elem 17.490  cycles/elem]

sort [i32]/256          time:   [3.5197 Kcycles 3.5197 Kcycles 3.5198 Kcycles]
                        thrpt:  [13.749  cycles/elem 13.749  cycles/elem 13.749  cycles/elem]
```

[NUCLEO-F401RE]: https://www.st.com/en/evaluation-tools/nucleo-f401re.html
[cargo-criterion]: https://github.com/bheisler/cargo-criterion

## Implementation

User benchmark crates use the `criterion_main!` macro exported by this library. In each Cargo build run, this library is built in *Driver mode*, *Host mode*, *Proxy mode*, or *Target mode*. The mode decision is done by Cargo features `role_*` and affects the expansion result of exported macros, dictating what role the compiled executable takes.


                     You
                      │ cargo bench
                      v
             ╭──────────────────────────────────────────────────────────────────────┐
             │                               Cargo                                  │
             ╰──────────────────────────────────────────────────────────────────────╯
          build & run │         cargo bench ^  │                 cargo bench ^  │
            ╭─────────╯   ╭─────────────────╯  │ build   ╭───────────────────╯  │ build
            v             │                    v         │                      v
    ╭───────────────────╮ │        ╭───────────────────╮ │      ╵       ╭───────────────────╮
    │ benches/*.rs      │ │        │ benches/*.rs      │ │      ╵       │ benches/*.rs      │
    │  (no user code)   │ │        │  (no user code)   │ │      ╵       │  bench groups     │
    ├───────────────────┤ │        ├───────────────────┤ │      ╵       │      ...          │<╮
    │     FarCri.rs     │─╯        │     FarCri.rs     │─╯      ╵       ├───────────────────┤ │
    │    Driver mode    │─────────>│     Proxy mode    │<───────┴──────>│     FarCri.rs     │─╯
    ├──────────────────┬┤    run   ├────────────────┬─┬┤    run ╵       │    Target mode    │
    │        std       v│          │  std-dependent v ││      & ╵       ├──────────────────┬┤
    ╰───────────────────╯          │      crates      ││   talk ╵       │  target-specific v│
                                   │  e.g., probe-rs  ││        ╵       │       crates      │
                                   ├────────────────┬─┼┤        ╵       │  e.g., cortex-m   │
                                   │        std     v v│        ╵       ╰───────────────────╯
                                   ╰───────────────────╯        ╵     Features: farcri/role_target
                            Features: farcri/role_proxy         ╵               farcri/target_nucleo_f401re
                                                                ╵                    ⋮
                                                           host ╵ target
                                                                ╵

 - By default (i.e., when the user does `cargo bench` or `cargo criterion`), this library is built in *Driver mode*, in which case `criterion_main!` produces a main function that invokes Cargo to build and run the current benchmark target (i.e., it uses Cargo to rebuild itself) in Proxy mode. This mode is selected by the absence of Cargo features, so it cannot depend on any other crates requiring `std` (dependencies are always additive in Cargo). This is very restrictive, which is why this mode exists separately from Proxy mode.

 - *Proxy mode* is selected by a private Cargo feature named `role_proxy`. In Proxy mode, the compiled executable takes the role of coordinating the actual benchmark execution on a remote target device. First, it invokes Cargo to build the current benchmark target (i.e., it uses Cargo to rebuild itself) with an additional parameter that causes this library to be built in Target mode. After that, it runs the compiled program on the target device and forwards messages concerning the measurement results to `cargo-criterion`.

 - *Target mode* is selected by a private Cargo feature named `role_target` and other target-specific features. The Target mode executable is supposed to run on a remote target device, which usually has no operating system and requires a `no_std` environment. The compiled executable of Target mode runs the actual benchmark code and transmits the measurement result back to the Proxy mode program running on the host system through a target-specific transport mechanism.

> **Rationale:** The primary goal for this design is to provide a good user experience. It's paramount that the users can run benchmarks by a simple, memorable command that is nothing more than something like `cargo criterion`.
>
> Each execution environment has a unique set of required crates (e.g., asynchronous I/O, peripheral access crates, HAL crates), and using Cargo features is the only way to control the dependencies of a single crate. The catch is that crate dependencies are always additive. For example, Proxy mode requires `probe-rs` to communicate with a debug probe connected to the computer. If Proxy mode didn't have its own Cargo feature, `probe-rs` would have to be specified as a non-`optional` dependency, and Cargo would always try to build `probe-rs`, which would fail in Target mode.

## License

FarCri.<span></span>rs is dual licensed under the Apache 2.0 license and the MIT license.

FarCri.<span></span>rs includes some portion of [Criterion.rs], which is licensed under the same licenses.

[Criterion.rs]: https://github.com/bheisler/criterion.rs

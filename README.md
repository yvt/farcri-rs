# FarCri.rs: Criterion.rs on Remote Target

WIP

- [x] Basic measurement
- [ ] Custom values (e.g., performance counters)
- [ ] `Linear` sampling method
- [x] Integration with [cargo-criterion]

[cargo-criterion]: https://github.com/bheisler/cargo-criterion

## Try it

*Prerequisites:* [NUCLEO-F401RE], Rust 1.51.0 or newer, libusb1, and [cargo-criterion]

```
$ env FARCRI_TARGET=nucleo_f401re cargo bench -p farcri_example --plotting-backend gnuplot
[⋯ INFO  farcri::proxy::targets::probe_rs] Flashing '⋯/farcri-rs/target/thumbv7em-none-eabihf/release/deps/sort-8f14de0564ff7f2f'
       ⋮
sort [i32; 100]         time:   [1.5693 Kcycles 1.5694 Kcycles 1.5694 Kcycles]
                        change: [-0.0017% +0.0000% +0.0017%] (p = 0.96 > 0.05)
                        No change in performance detected.
```

[NUCLEO-F401RE]: https://www.st.com/en/evaluation-tools/nucleo-f401re.html
[cargo-criterion]: https://github.com/bheisler/cargo-criterion

## Implementation

User benchmark crates use the `criterion_main!` macro exported by this library. In a single Cargo build run, this library is built in one of *Driver mode*, *Host mode*, *Proxy mode*, and *Target mode*. The mode decision is done by Cargo features `role_*` and affects the expansion result of exported macros, dictating what role the compiled executable takes.


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

 - By default (i.e., when the user does `cargo bench` or `cargo criterion`), this library is built in *Driver mode*, in which case `criterion_main!` produces a main function that invokes Cargo to build and run the current benchmark target (i.e., it instructs Cargo to build itself) in Proxy mode. This mode is selected by the absence of Cargo features, so it cannot depend on any other crates requiring `std` (dependencies are always additive in Cargo). This is very restrictive, which is why this mode exists separately from Proxy mode.

 - *Proxy mode* is selected by a private Cargo feature named `role_proxy`. In Proxy mode, the compiled executable takes the role of conducting the actual benchmark execution on a remote target device. First, it invokes Cargo to build the current benchmark target (i.e., it instructs Cargo to build itself) with an additional parameter that causes this library to be built in Target mode. After that, it runs the compiled program on the target device and forwards the measurement result to `cargo-criterion`. (It's a user error to run the benchmark outside `cargo-criterion`.)

 - *Target mode* is selected by a private Cargo feature named `role_target` and other target-specific features. The Target mode executable is supposed to run on a remote target device, which usually has no operating system and requires a `no_std` environment. The compiled executable of Target mode runs the actual benchmark code and transmits the measurement result back to the Proxy mode program running on the host system through a target-specific transport mechanism.

> **Rationale:** The primary goal for this design is to provide a good user experience. It's paramount that the users can run benchmarks by a simple, memorable command that is nothing more than something like `cargo criterion`.
>
> Each execution environment has a unique set of required crates (e.g., asynchronous I/O, peripheral access crates, HAL crates), and using Cargo features is the only way to control the dependencies of a single crate. The catch is that crate dependencies are always additive. For example, Proxy mode requires `probe-rs` to communicate with a debug probe connected to the computer. If Proxy mode didn't have its own Cargo feature, `probe-rs` would have to be specified as a non-`optional` dependency, and Cargo would always try to build `probe-rs`, which would fail in Target mode.

## License

FarCri.<span></span>rs is dual licensed under the Apache 2.0 license and the MIT license.

FarCri.<span></span>rs includes some portion of [Criterion.rs], which is licensed under the same licenses.

[Criterion.rs]: https://github.com/bheisler/criterion.rs

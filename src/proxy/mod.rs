//! Proxy mode entry point
use anyhow::{Context as _, Result};
use clap::Clap;

use crate::bencher::protocol;

mod dumbfront;
mod targetlink;
mod targets;

#[doc(hidden)]
#[tokio::main]
pub async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("farcri=info"))
        .init();

    if let Err(e) = main_inner().await {
        log::error!("Command failed.\n{:?}", e);
        std::process::exit(1);
    }
}

#[derive(Clap, Debug)]
struct Opts {
    // ----------------------------------------------------------------
    // Standard Cargo test harness parameters
    /// Run tests and not benchmarks
    #[clap(long = "test")]
    test: bool,

    /// Run benchmarks instead of tests
    #[clap(long = "bench")]
    bench: bool,

    test_selector: Vec<String>,

    // ----------------------------------------------------------------
    /// Target chip/board, can also be specified by `$FARCRI_TARGET`
    #[clap(
        long = "farcri-target",
        parse(try_from_str = try_parse_target),
        possible_values(&TARGET_POSSIBLE_VALUES),
        default_value(default_from_env("FARCRI_TARGET")),
    )]
    target: &'static dyn targets::Target,

    /// Override target architecture, can also be specified by `$FARCRI_ARCH`
    ///
    /// See the documentation of `Arch::from_str` for full syntax.
    #[clap(
        long = "farcri-arch",
        parse(try_from_str = std::str::FromStr::from_str),
    )]
    arch: Option<targets::Arch>,

    /// Dry run - specifies not to download or execute the benchmark code on the
    /// target.
    #[clap(long = "farcri-dry-run")]
    dry_run: bool,

    /// Log level of the test program
    #[clap(long = "farcri-log-level",
        possible_values(&LogLevel::variants()), case_insensitive = true,
        default_value = "info")]
    log_level: LogLevel,
}

fn default_from_env(name: &str) -> &'static str {
    std::env::var(name)
        .ok()
        .map(|x| &**Box::leak(Box::new(x)))
        .unwrap_or("")
}

#[derive(Debug, Clone, Copy, arg_enum_proc_macro::ArgEnum)]
enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

lazy_static::lazy_static! {
    static ref TARGET_POSSIBLE_VALUES: Vec<&'static str> =
        targets::TARGETS.iter().map(|x|x.0).collect();
}

fn try_parse_target(arg_target: &str) -> Result<&'static dyn targets::Target, &'static str> {
    targets::TARGETS
        .iter()
        .find(|x| x.0 == arg_target)
        .ok_or("no such target")
        .map(|x| x.1)
}

async fn main_inner() -> Result<()> {
    // Parse arguments
    let opts: Opts = Clap::parse();
    log::debug!("opts = {:#?}", opts);

    if !opts.bench {
        log::info!("Exiting because `--bench` is not specified");
        return Ok(());
    }

    if !opts.test_selector.is_empty() {
        log::warn!("Test names are specified but we don't currently support them");
    }

    let target = opts.target;
    let build_setup = target
        .prepare_build()
        .await
        .context("Failed to setup a build environment")?;

    // Derive the target architecture information
    let arch = opts.arch.unwrap_or_else(|| target.target_arch());
    log::debug!("arch = {}", arch);

    let arch_opt = arch.build_opt().with_context(|| {
        format!(
            "The target architecture '{}' is invalid or unsupported.",
            arch
        )
    })?;
    log::debug!("arch_opt = {:?}", arch_opt);

    // Derive `RUSTFLAGS`
    let target_features = &arch_opt.target_features;
    let mut rustflags = if target_features.is_empty() {
        String::new()
    } else {
        format!("-C target-feature={}", target_features)
    };
    log::debug!("target_features = {:?}", target_features);

    for x in build_setup.rustc_flags() {
        rustflags.push_str(" ");
        rustflags.push_str(&x);
    }
    log::debug!("rustflags = {:?}", rustflags);

    log::debug!("cargo_features = {:?}", target.cargo_features());

    // Connect to the target now. Fail-fast so that the user can divert
    // attention without risking wasting time.
    let probe = if opts.dry_run {
        None
    } else {
        Some(
            target
                .connect()
                .await
                .context("Failed to connect to the target.")?,
        )
    };

    log::info!("Building the target executable");
    let exe = crate::cargo::compile_self(|cmd| {
        cmd.arg("--features=farcri/role_target")
            .args(
                target
                    .cargo_features()
                    .iter()
                    .map(|f| format!("--features=farcri/{}", f)),
            )
            .arg(match opts.log_level {
                LogLevel::Off => "--features=farcri/max_level_off",
                LogLevel::Error => "--features=farcri/max_level_error",
                LogLevel::Warn => "--features=farcri/max_level_warn",
                LogLevel::Info => "--features=farcri/max_level_info",
                LogLevel::Debug => "--features=farcri/max_level_debug",
                LogLevel::Trace => "--features=farcri/max_level_trace",
            })
            .arg("--target")
            .arg(&arch_opt.target_triple)
            .args(if target_features.is_empty() {
                None
            } else {
                log::debug!("Specifying `-Zbuild-std=core` because of a custom target feature set");
                Some("-Zbuild-std=core")
            })
            .env("RUSTFLAGS", &rustflags)
            .envs(build_setup.build_envs())
    });

    let mut probe = if let Some(probe) = probe {
        probe
    } else {
        log::warn!("Exiting now because a `--farcry-dry-run` option is present.");
        return Ok(());
    };

    let target_stream = probe
        .program_and_get_output(&exe)
        .await
        .context("Failed to load the benchmark application to the target.")?;

    let mut target_link = targetlink::TargetLink::new(target_stream).await?;

    // Send the greeting message
    let mode = if opts.bench {
        protocol::Mode::Benchmark
    } else {
        protocol::Mode::Test
    };
    let greeting = protocol::DownstreamMessage::Greeting {
        _unused: Default::default(),
        mode,
    };
    log::info!("Options: {:?}", greeting);
    target_link
        .send(&greeting)
        .await
        .context("Failed to send the greeting message.")?;

    // TODO: cargo-criterion front-end
    log::info!("Using the dumb front-end");
    dumbfront::run_frontend(target_link).await?;

    Ok(())
}

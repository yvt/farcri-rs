use core::{cell::RefCell, fmt::Write};
use cortex_m::interrupt;

static LOG_CHANNEL: interrupt::Mutex<RefCell<Option<rtt_target::UpChannel>>> =
    interrupt::Mutex::new(RefCell::new(None));

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        interrupt::free(move |cs| {
            let mut log_channel = LOG_CHANNEL.borrow(cs).borrow_mut();
            if let Some(channel) = &mut *log_channel {
                writeln!(
                    channel,
                    "[{:5} {}] {}",
                    record.level(),
                    record.target(),
                    record.args()
                )
                .unwrap();
            }
        });
    }

    fn flush(&self) {}
}

pub struct Comm {
    down: rtt_target::DownChannel,
    up: rtt_target::UpChannel,
}

impl Comm {
    pub fn new() -> Self {
        let channels = rtt_target::rtt_init! {
            up: {
                0: {
                    size: 1024
                    mode: NoBlockSkip
                    name: "Log"
                }
                1: {
                    size: 1024
                    mode: BlockIfFull
                    name: "Terminal"
                }
            }
            down: {
                0: {
                    size: 512
                    mode: BlockIfFull
                    name: "Terminal"
                }
            }
        };
        let (up0, up1) = channels.up;

        interrupt::free(move |cs| {
            *LOG_CHANNEL.borrow(cs).borrow_mut() = Some(up0);
        });
        log::set_logger(&Logger).unwrap();
        log::set_max_level(log::LevelFilter::Trace);

        Self {
            up: up1,
            down: channels.down.0,
        }
    }

    pub fn write(&mut self, mut b: &[u8]) {
        while b.len() > 0 {
            let bytes_written = self.up.write(b);
            b = &b[bytes_written..];
        }
    }

    pub fn read(&mut self, b: &mut [u8]) -> usize {
        loop {
            let num_bytes_read = self.down.read(b);
            if num_bytes_read > 0 {
                return num_bytes_read;
            }
            core::hint::spin_loop();
        }
    }
}

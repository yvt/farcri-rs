use futures::ready;
use std::{
    future::Future,
    marker::Unpin,
    pin::Pin,
    task::{self, Poll},
};
use tokio::io::{self, AsyncBufRead, AsyncReadExt};

pub async fn retry_on_fail<R, T, E: std::fmt::Debug>(mut f: impl FnMut() -> R) -> Result<T, E>
where
    R: Future<Output = Result<T, E>>,
{
    let mut count = 8u32;
    loop {
        match f().await {
            Ok(x) => return Ok(x),
            Err(e) => {
                log::warn!("Attempt failed: {:?}", e);
                count -= 1;
                if count == 0 {
                    log::warn!("Retry limit reached");
                    return Err(e);
                } else {
                    log::warn!("Retrying... (remaining count = {:?})", count);
                }
            }
        }
    }
}

/// Discard the output of `this` until `pattern` is found and wholly read.
///
/// Returns `true` if the `pattern` was found and read; `false` otherwise.
pub async fn async_buf_read_skip_until_pattern(
    mut this: Pin<&mut impl AsyncBufRead>,
    pattern: &[u8],
) -> io::Result<bool> {
    assert!(!pattern.is_empty());

    // The last portion of the previous read + the first portion of the current
    // read. (`buf[-(pattern.len() - 1)..pattern.len() - 1]`)
    // This is used to locate a boundary-crossing occurence of `pattern`.
    let mut overlap = vec![0u8; (pattern.len() - 1) * 2];
    let overlap = &mut overlap[..];

    match this
        .as_mut()
        .read_exact(&mut overlap[0..pattern.len() - 1])
        .await
    {
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
        result => {
            let read_bytes = result?;
            assert_eq!(read_bytes, pattern.len() - 1);
        }
    }

    loop {
        // Unfortunately `tokio::io::AsyncBufReadExt` doesn'h have `fill_buf`.
        let result = futures::future::poll_fn(|cx| {
            let buf = match futures::ready!(this.as_mut().poll_fill_buf(cx)) {
                Ok(buf) => buf,
                Err(e) => return Poll::Ready(Some(Err(e))),
            };

            if buf.len() == 0 {
                // EOF
                return Poll::Ready(Some(Ok(false)));
            }

            //                  0                          buf.len()
            //     buf          ░░░░░░░░░░░░░░░░░░░░░░░░░░░
            // overlap      ▒▒▒▒▒▒▒▒
            //            -p        p
            //                          (p = pattern.len() - 1)

            // Fill the second half of `overlap` to search in range
            // `buf[-p .. min(buf.len(), p)]`
            let copied_to_overlap = buf.len().min(pattern.len() - 1);
            overlap[pattern.len() - 1..][..copied_to_overlap]
                .copy_from_slice(&buf[..copied_to_overlap]);

            if let Some(i) = slice_find(&overlap[..pattern.len() - 1 + copied_to_overlap], pattern)
            {
                // Consume `buf[..i - p + pattern.len()]`
                this.as_mut().consume(i + 1);
                return Poll::Ready(Some(Ok(true)));
            }

            // Search in range `buf[0..]`
            if let Some(i) = slice_find(buf, pattern) {
                // Consume `buf[..i + pattern.len()]`
                this.as_mut().consume(i + pattern.len());
                return Poll::Ready(Some(Ok(true)));
            }

            // Leave the last part in the first half of `overlap` for the
            // next iteration
            // (Copy the last `p` bytes of `buf[-p .. buf.len()]`)
            if buf.len() <= pattern.len() - 1 {
                // `buf.len() <= p`, so the copied part is wholly included in `overlap`
                overlap.copy_within(overlap.len() - (pattern.len() - 1).., 0);
            } else {
                // `buf.len() >= p`, so the copied part is wholly included in `buf[0..]`
                overlap[..pattern.len() - 1]
                    .copy_from_slice(&buf[buf.len() - (pattern.len() - 1)..]);
            }

            // Consume `buf[..]`
            let len = buf.len();
            this.as_mut().consume(len);

            // Repeat
            Poll::Ready(None)
        })
        .await;

        if let Some(result) = result {
            return result;
        }
    }
}

/// `O(m * n)` search
fn slice_find<T: PartialEq>(haystack: &[T], needle: &[T]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Single-producer-multiple-consumer - allows one `Future`'s completion to be
/// awaited for by multiple consumers.
#[derive(Debug)]
pub struct Spmc<Fut: Future> {
    fut: Fut,
    wakers: Box<[Option<task::Waker>]>,
    active_consumer_index: Option<usize>,
}

impl<Fut: Future + Unpin> Spmc<Fut> {
    #[inline]
    pub fn new(num_consumers: usize, fut: Fut) -> Self {
        Self {
            fut,
            wakers: vec![None; num_consumers].into(),
            active_consumer_index: None,
        }
    }

    /// Poll the inner `Future`.
    ///
    /// If the inner `Future` isn't resolved yet, this function will return
    /// `Pending` and registers the `Waker` to the specified consumer's waker
    /// slot.
    ///
    /// When the inner `Future` can make progress, all registered `Waker`s are
    /// woken up. Eventually, when the inner `Future` finishes, the next call to
    /// `poll` will return the `Future`'s output wrapped in `Ready(_)`.
    /// This method must not be called again after it returns `Ready(_)`.
    ///
    /// The implementation assumes that every consumer that calls `poll` will
    /// try to make progress by calling `poll` over and over until the inner
    /// `Future` completes.
    pub fn poll(&mut self, consumer_index: usize, cx: &mut task::Context<'_>) -> Poll<Fut::Output> {
        assert!(consumer_index < self.wakers.len());

        // The first call decides who is `active_consumer_index`
        let active_consumer_index = *self.active_consumer_index.get_or_insert(consumer_index);

        if consumer_index == active_consumer_index {
            // Only `active_consumer_index` polls the inner `Future`
            let output = ready!(Pin::new(&mut self.fut).poll(cx));

            // Wake all other wakers
            for waker in self.wakers.iter_mut() {
                if let Some(waker) = waker.take() {
                    waker.wake();
                }
            }

            // And return the output
            Poll::Ready(output)
        } else {
            // Other consumers are passive and just register wakers
            let waker_cell = &mut self.wakers[consumer_index];
            if waker_cell
                .as_ref()
                .filter(|w| cx.waker().will_wake(w))
                .is_none()
            {
                *waker_cell = Some(cx.waker().clone());
            }

            Poll::Pending
        }
    }
}

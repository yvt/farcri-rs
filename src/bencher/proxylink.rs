//! Connection to the Proxy program
use serde::Serialize;

use super::protocol;
use crate::target::BencherIo;

pub(crate) struct ProxyLink<'a> {
    io: &'a mut BencherIo,
    /// Packet buffer, used for sending and receiving both.
    buf: &'a mut [u8],
    /// `buf[buf_pos..buf_len]` is yet to be decoded.
    buf_pos: usize,
    /// `buf[0..buf_len]` contains valid data.
    buf_len: usize,
    /// `buf[buf_pos..buf_scan]` does not contain `SLIP_FRAME_END`.
    buf_scan: usize,
}

const SLIP_FRAME_END: u8 = 0xc0;
const SLIP_FRAME_ESC: u8 = 0xdb;
const SLIP_FRAME_ESC_END: u8 = 0xdc;
const SLIP_FRAME_ESC_ESC: u8 = 0xdd;

impl<'a> ProxyLink<'a> {
    #[inline]
    pub fn new(io: &'a mut BencherIo, buf: &'a mut [u8]) -> Self {
        let mut pos = 0;
        let mut len = 0;
        let mut next = |io: &mut BencherIo| {
            if pos == len {
                len = io.read(buf);
                assert_ne!(len, 0);
                pos = 1;
                buf[0]
            } else {
                pos += 1;
                buf[pos - 1]
            }
        };

        let mut packet = [0u8; protocol::HANDSHAKE_MAGIC.len() + protocol::HANDSHAKE_NONCE_LEN];
        let mut got_handshape = false;

        packet[..protocol::HANDSHAKE_MAGIC.len()].copy_from_slice(protocol::HANDSHAKE_MAGIC);

        log::debug!("Performing handshake");
        'outer: loop {
            loop {
                let b = next(io);
                if b == protocol::HANDSHAKE_MAGIC[0] {
                    break;
                } else if got_handshape {
                    if b != protocol::HANDSHAKE_END_MAGIC[0] {
                        panic!("bad handshake end packet");
                    }
                    break 'outer;
                }
            }

            // Read `HANDSHAKE_MAGIC[1..]` and nonce
            for &b_ref in protocol::HANDSHAKE_MAGIC[1..].iter() {
                let b = next(io);
                if b != b_ref {
                    // invalid packet
                    continue 'outer;
                }
            }

            log::trace!("Got a valid `HANDSHAKE_MAGIC`, now reading nonce");

            // Get nonce
            for b_out in packet[protocol::HANDSHAKE_MAGIC.len()..].iter_mut() {
                *b_out = next(io);
            }

            log::trace!("Replying with {:?}", packet);

            // Reply
            io.write(&packet);

            // Now we can accept `HANDSHAKE_END_MAGIC`
            got_handshape = true;
        }

        for &b_ref in protocol::HANDSHAKE_END_MAGIC[1..].iter() {
            let b = next(io);
            if b != b_ref {
                // invalid packet, we can't recover
                panic!("bad handshake end packet");
            }
        }

        log::trace!("Got a valid `HANDSHAKE_END_MAGIC`");
        log::trace!("Replying with {:?}", protocol::HANDSHAKE_END_MAGIC);

        // Reply
        io.write(protocol::HANDSHAKE_END_MAGIC);

        Self {
            io,
            buf,
            buf_pos: 0,
            buf_len: 0,
            buf_scan: 0,
        }
    }

    #[inline]
    pub fn io(&mut self) -> &mut BencherIo {
        self.io
    }

    /// Receive one `DownstreamMessage`.
    pub fn recv(&mut self) -> protocol::DownstreamMessage<&str> {
        loop {
            let packet_start = self.buf_pos;
            if let Some(end) = self.buf[self.buf_scan..self.buf_len]
                .iter()
                .position(|&b| b == SLIP_FRAME_END)
            {
                // Found the terminator of the current packet
                self.buf_scan += end + 1;
                self.buf_pos = self.buf_scan;
                if end > 0 {
                    // Non-empty message.
                    let mut packet_end = self.buf_pos - 1;

                    // Expand SLIP escape sequences
                    {
                        let mut window = &mut self.buf[packet_start..packet_end];
                        let mut read_ptr = 0;
                        while read_ptr < window.len() {
                            let b1 = window[read_ptr];
                            if b1 == SLIP_FRAME_ESC && read_ptr + 1 < window.len() {
                                let b2 = window[read_ptr + 1];
                                window[0] = match b2 {
                                    SLIP_FRAME_ESC_END => SLIP_FRAME_END,
                                    SLIP_FRAME_ESC_ESC => SLIP_FRAME_ESC,
                                    _ => panic!("invalid SLIP escape"),
                                };
                                read_ptr += 1;
                            } else {
                                window[0] = b1;
                            }
                            window = &mut window[1..];
                        }
                        packet_end -= window.len();
                    }

                    // Decode it
                    let packet = &mut self.buf[packet_start..packet_end];
                    log::trace!("recv (raw): {:?}", packet);
                    let msg = serde_cbor::de::from_mut_slice(packet).unwrap();
                    log::debug!("recv: {:?}", msg);
                    return msg;
                }
            } else {
                // Looks like we need to read some more to find the terminator
                if self.buf.len() - self.buf_len <= 1 {
                    // The buffer is full.
                    if self.buf_pos == 0 {
                        panic!("too large received packet");
                    } else {
                        // We can make some room by discarding the already-read
                        // portion `buf[0..buf_pos]`.
                        self.buf.copy_within(self.buf_pos..self.buf_len, 0);
                        self.buf_len -= self.buf_pos;
                        self.buf_pos = 0;
                        self.buf_scan = self.buf_len;
                    }
                } else {
                    let buf_outer = &mut self.buf[self.buf_len..];
                    let num_read_bytes = self.io.read(buf_outer);
                    assert!(num_read_bytes <= buf_outer.len());
                    assert_ne!(num_read_bytes, 0);

                    self.buf_scan = self.buf_len;
                    self.buf_len += num_read_bytes;
                }
            }
        }
    }

    /// Send one `UpstreamMessage`. Destroys any remaining messages in the
    /// receiving buffer.
    pub fn send(&mut self, msg: &protocol::UpstreamMessage<&str, &[u64]>) {
        self.buf_pos = 0;
        self.buf_len = 0;
        self.buf_scan = 0;

        // Encode
        let writer = serde_cbor::ser::SliceWrite::new(self.buf);
        let mut ser = serde_cbor::ser::Serializer::new(writer);
        msg.serialize(&mut ser).unwrap();
        let num_bytes = ser.into_inner().bytes_written();

        log::debug!("send: {:?}", msg);
        log::trace!("  encoded as: {:?}", &self.buf[..num_bytes]);

        // Create a SLIP frame
        let num_extra_bytes = self.buf[..num_bytes]
            .iter()
            .filter(|&&b| matches!(b, SLIP_FRAME_END | SLIP_FRAME_ESC))
            .count();
        let num_frame_bytes = num_bytes
            .checked_add(num_extra_bytes)
            .and_then(|x| x.checked_add(1))
            .expect("packet being sent is too large");
        {
            let mut window = self
                .buf
                .get_mut(..num_frame_bytes)
                .expect("packet being sent is too large");
            let mut read_ptr = num_bytes.wrapping_sub(1);

            // Append `SLIP_FRAME_END`
            if let [tail @ .., head] = window {
                *head = SLIP_FRAME_END;
                window = tail;
            } else {
                unreachable!();
            }

            // Escape in-place
            while read_ptr < window.len() {
                let b = window[read_ptr];
                let escape_code = match b {
                    SLIP_FRAME_END => SLIP_FRAME_ESC_END,
                    SLIP_FRAME_ESC => SLIP_FRAME_ESC_ESC,
                    _ => b,
                };

                if escape_code == b || window.len() < 2 {
                    // output as-is
                    if let [tail @ .., head] = window {
                        *head = b;
                        window = tail;
                    } else {
                        unreachable!();
                    }
                } else {
                    if let [tail @ .., head1, head2] = window {
                        *head1 = SLIP_FRAME_ESC;
                        *head2 = escape_code;
                        window = tail;
                    } else {
                        unreachable!();
                    }
                }

                read_ptr = read_ptr.wrapping_sub(1);
            }
        }

        // Send it
        log::trace!("  SLIP frame: {:?}", &self.buf[..num_frame_bytes]);

        self.io.write(&self.buf[..num_frame_bytes]);
    }
}

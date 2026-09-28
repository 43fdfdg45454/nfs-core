//! Entry points for the fuzz targets (fuzz/): every decoder of what the server sends the client.

use crate::attr::Attrs;
use crate::compound::Results;
use crate::ops;
use bytes::Bytes;
use nfs_rpc::Decoder;

/// Decodes the results of operation `code` as the client does (nothing more for operations
/// whose results it does not read).
fn decode(code: u32, d: &mut Decoder) {
    _ = match code {
        9 => Attrs::decode(d).map(drop),
        10 => ops::fh(d).map(drop),
        26 => ops::dir::readdir(d).map(drop),
        27 => ops::dir::readlink(d).map(drop),
        18 => ops::open::open(d).map(drop),
        25 => ops::file::read(d).map(drop),
        38 => ops::file::write(d).map(drop),
        5 => ops::file::commit(d).map(drop),
        60 => ops::copy::copy(d).map(drop),
        42 => ops::session::exchange_id(d).map(drop),
        43 => ops::session::create_session(d).map(drop),
        53 => ops::session::sequence(d).map(drop),
        6 | 11 | 28 | 29 => ops::skip_change_info(d),
        _ => Ok(()),
    };
}

const DECODED: [u32; 16] = [9, 10, 26, 27, 18, 25, 38, 5, 60, 42, 43, 53, 6, 11, 28, 29];

/// One operation's results; the first byte picks which.
pub fn result(data: &[u8]) {
    let Some((&which, rest)) = data.split_first() else { return };
    decode(
        DECODED[usize::from(which) % DECODED.len()],
        &mut Decoder::new(Bytes::copy_from_slice(rest)),
    );
}

/// A whole COMPOUND reply read as the client reads it: the first byte says how many operations
/// it expects, the next ones which (as with `result`), the rest is the reply.
pub fn compound(data: &[u8]) {
    let Some((&count, rest)) = data.split_first() else { return };
    let count = usize::from(count % 8).min(rest.len());
    let (codes, reply) = rest.split_at(count);
    let Ok(mut results) = Results::parse(Decoder::new(Bytes::copy_from_slice(reply))) else {
        return;
    };
    _ = results.status();
    for &code in codes {
        let code = DECODED[usize::from(code) % DECODED.len()];
        match results.next(code) {
            Ok(d) => decode(code, d),
            Err(_) => return,
        }
    }
}

/// A call from the server on the back channel: CB_NULL or CB_COMPOUND (first byte), and its
/// arguments.
pub fn callback(data: &[u8]) {
    let Some((&procedure, args)) = data.split_first() else { return };
    let (callbacks, _returns) = crate::callback::Callbacks::new();
    let args = Decoder::new(Bytes::copy_from_slice(args));
    _ = nfs_rpc::Handler::call(
        &*callbacks,
        ops::session::CALLBACK_PROGRAM,
        1,
        u32::from(procedure & 1),
        args,
    );
}

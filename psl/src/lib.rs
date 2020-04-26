//! Bundle the public suffix list in the compiled code.

use flate2::read::GzDecoder;
use log::trace;
use once_cell::sync::Lazy;
use publicsuffix::List;
use std::io::{Read, Result as IoResult};

const PSL: &[u8; 71408] = include_bytes!("public_suffix_list.dat.gz");
const DATE: &str = include_str!("date.txt");

struct PslRead(usize);

impl Read for PslRead {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        let pos = &mut self.0;
        let max = buf.len().min(PSL.len() - *pos);
        (&mut buf[0..max]).copy_from_slice(&PSL[*pos..(*pos + max)]);
        *pos += max;
        Ok(max)
    }
}

pub static PUBLIC_SUFFIX_LIST: Lazy<List> = Lazy::new(|| {
    let mut d = GzDecoder::new(PslRead(0));
    let mut s = String::new();
    d.read_to_string(&mut s).expect("Ungzip public suffix list");
    trace!("Public suffix list from: {}", DATE);
    List::from_string(s).expect("Public suffix list from string")
});

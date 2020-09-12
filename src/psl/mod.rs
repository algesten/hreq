//! Bundle the public suffix list in the compiled code.

use flate2::read::GzDecoder;
use once_cell::sync::Lazy;
use publicsuffix::List;
use std::io::{Cursor, Read};

const PSL: &[u8] = include_bytes!("public_suffix_list.dat.gz");
const DATE: &str = include_str!("date.txt");

pub static PUBLIC_SUFFIX_LIST: Lazy<List> = Lazy::new(|| {
    let io = Cursor::new(PSL);

    let mut d = GzDecoder::new(io);

    let mut s = String::new();
    d.read_to_string(&mut s).expect("Ungzip public suffix list");

    trace!("Public suffix list from: {}", DATE.trim());
    List::from_string(s).expect("Public suffix list from string")
});

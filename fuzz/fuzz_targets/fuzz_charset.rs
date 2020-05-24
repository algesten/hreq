#![no_main]
use libfuzzer_sys::fuzz_target;

use encoding_rs::*;
use hreq::CharCodec;

static ENCODINGS: [&'static Encoding; 39] = [
    &UTF_8_INIT,
    &REPLACEMENT_INIT,
    &GBK_INIT,
    &BIG5_INIT,
    &EUC_JP_INIT,
    &GB18030_INIT,
    &UTF_16BE_INIT,
    &UTF_16LE_INIT,
    &SHIFT_JIS_INIT,
    &EUC_KR_INIT,
    &ISO_2022_JP_INIT,
    &X_USER_DEFINED_INIT,
    &WINDOWS_1250_INIT,
    &WINDOWS_1251_INIT,
    &WINDOWS_1252_INIT,
    &WINDOWS_1253_INIT,
    &WINDOWS_1254_INIT,
    &WINDOWS_1255_INIT,
    &WINDOWS_1256_INIT,
    &WINDOWS_1257_INIT,
    &WINDOWS_1258_INIT,
    &KOI8_U_INIT,
    &MACINTOSH_INIT,
    &IBM866_INIT,
    &KOI8_R_INIT,
    &ISO_8859_2_INIT,
    &ISO_8859_3_INIT,
    &ISO_8859_4_INIT,
    &ISO_8859_5_INIT,
    &ISO_8859_6_INIT,
    &ISO_8859_7_INIT,
    &ISO_8859_10_INIT,
    &ISO_8859_13_INIT,
    &ISO_8859_14_INIT,
    &WINDOWS_874_INIT,
    &ISO_8859_15_INIT,
    &ISO_8859_16_INIT,
    &ISO_8859_8_I_INIT,
    &X_MAC_CYRILLIC_INIT,
];

const MAX_OUT: usize = 10;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let enc_fr = random_encoding(data[0] as usize);
    let enc_to = random_encoding(data[1] as usize);
    if let (Some(enc_fr), Some(enc_to)) = (enc_fr, enc_to) {
        let data = &data[2..];
        let mut codec = CharCodec::new(enc_fr, enc_to);
        let mut out = vec![0; MAX_OUT];
        let mut total = 0;
        loop {
            if total == data.len() {
                return;
            }
            let mut consumed = 0;
            let max = (data.len() - total).min(MAX_OUT);
            codec.decode_from_buf(&data[total..(total + max)], &mut out, &mut consumed).ok();
            total += consumed;
            if consumed == 0 {
                break;
            }
        }
    }
});

fn random_encoding(i: usize) -> Option<&'static Encoding> {
    if i >= ENCODINGS.len() {
        return None;
    }
    Some(ENCODINGS[i])
}

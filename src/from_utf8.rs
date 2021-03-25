use std::str::from_utf8;

pub fn from_utf8_lossy_replace(mut buf: Vec<u8>, replacement: u8) -> String {
    assert!(replacement.is_ascii());

    // index into buf where we are to pick up checking legal utf8.
    let mut start_from = 0;

    loop {
        match from_utf8(&buf[start_from..]) {
            // This unsafe is ok, because the entire buf has been checked/fixed.
            Ok(_) => break unsafe { String::from_utf8_unchecked(buf) },

            Err(e) => {
                let idx = e.valid_up_to();
                let len = e.error_len();

                let relative_end = if let Some(len) = len {
                    // replace all chars in len with
                    for i in idx..(idx + len) {
                        buf[i] = replacement;
                    }

                    idx + len
                } else {
                    // unexpected end of string, replace last char.
                    buf[idx - 1] = replacement;

                    idx
                };

                start_from += relative_end;
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    pub fn utf8_legal() {
        let buf = "möst excellent".to_string().into_bytes();
        let s = from_utf8_lossy_replace(buf, b'?');
        assert_eq!(s, "möst excellent");
    }

    #[test]
    pub fn utf8_illegal_middle() {
        let buf = vec![0x6d, 0x6f, 0x73, 0x74, 0x20, 0x88, 0x20]; // "most ? "
        let s = from_utf8_lossy_replace(buf, b'?');
        assert_eq!(s, "most ? ");
    }

    #[test]
    pub fn utf8_illegal_end() {
        let buf = vec![0x6d, 0x6f, 0x73, 0x74, 0x20, 0xfe]; // "most ? "
        let s = from_utf8_lossy_replace(buf, b'?');
        assert_eq!(s, "most ?");
    }
}

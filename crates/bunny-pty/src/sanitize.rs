//! Strip terminal capability probe noise from PTY output.

/// Remove device-attribute and window-report CSI that leak through `tmux attach`.
pub fn strip_probe_noise(data: &str) -> String {
    let bytes = data.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let start = i;
            i += 2;
            let mut is_probe = false;
            if i < bytes.len() && bytes[i] == b'?' {
                is_probe = true;
                i += 1;
            } else if i < bytes.len() && bytes[i] == b'>' {
                is_probe = true;
                i += 1;
            } else if i < bytes.len() && bytes[i] == b'6' && i + 1 < bytes.len() && bytes[i + 1] == b'n' {
                // CPR request
                is_probe = true;
                i += 2;
            }
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || bytes[i] == b';'
                    || bytes[i] == b'?'
                    || bytes[i] == b'>')
            {
                i += 1;
            }
            if i < bytes.len() {
                let end_byte = bytes[i];
                if is_probe && matches!(end_byte, b'c' | b'R' | b't') {
                    i += 1;
                    continue;
                }
                // Unknown CSI — keep it
                out.extend_from_slice(&bytes[start..=i]);
                i += 1;
                continue;
            }
            out.extend_from_slice(&bytes[start..i]);
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    let mut s = String::from_utf8_lossy(&out).into_owned();
    // Bare fragments when ESC was consumed upstream
    for pat in ["?1;2c", ">0;", "1;2c0;"] {
        while s.contains(pat) {
            s = strip_bare_fragment(&s, pat);
        }
    }
    s
}

fn strip_bare_fragment(s: &str, prefix: &str) -> String {
    if let Some(idx) = s.find(prefix) {
        let rest = &s[idx + prefix.len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == ';' || c == 'c'))
            .unwrap_or(rest.len());
        let mut out = String::with_capacity(s.len());
        out.push_str(&s[..idx]);
        out.push_str(&rest[end..]);
        out
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_device_attribute_noise() {
        let raw = "\x1b[?1;2c\x1b[>0;276;0croot@host:~# 1;2c0;276;0c";
        let clean = strip_probe_noise(raw);
        assert_eq!(clean, "root@host:~# ");
    }
}

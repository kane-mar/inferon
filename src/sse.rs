//! SSE frame parsing shared by usage and quality extraction.
//!
//! Both `usage` and `quality` walk an accumulated SSE buffer looking only at
//! `data:` payloads; this keeps that walk in one place.

use serde_json::Value;

fn trim_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = bytes.split_last() {
        if last.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// The JSON payloads of every `data:` line in an accumulated SSE buffer,
/// skipping the `[DONE]` sentinel and any non-JSON frame.
pub fn data_frames(bytes: &[u8]) -> impl Iterator<Item = Value> + '_ {
    bytes.split(|&b| b == b'\n').filter_map(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let data = trim_ascii_ws(line.strip_prefix(b"data:")?);
        if data == b"[DONE]" {
            return None;
        }
        serde_json::from_slice::<Value>(data).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_frames_skips_done_and_non_json() {
        let sse = "event: ping\ndata: {\"a\":1}\n\ndata: not json\n\ndata: [DONE]\n\n";
        let frames: Vec<Value> = data_frames(sse.as_bytes()).collect();
        assert_eq!(frames, vec![serde_json::json!({"a": 1})]);
    }

    #[test]
    fn data_frames_tolerates_crlf_and_padding() {
        let sse = "data:   {\"a\": 2}  \r\n\r\n";
        let frames: Vec<Value> = data_frames(sse.as_bytes()).collect();
        assert_eq!(frames, vec![serde_json::json!({"a": 2})]);
    }
}

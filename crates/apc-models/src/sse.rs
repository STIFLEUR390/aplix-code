//! Server-sent events, split from an arbitrary chunking of the HTTP body.

/// Decodes `text/event-stream` bodies. Feed it whatever the socket delivered; it returns the
/// `data:` payload of every complete event.
#[derive(Debug, Default)]
pub struct Sse {
    buf: String,
}

impl Sse {
    /// Append a chunk and return the payloads now complete.
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        // Normalise line endings so one splitter handles both CRLF and LF servers.
        self.buf.push_str(&chunk.replace("\r\n", "\n"));

        let mut payloads = Vec::new();
        while let Some(end) = self.buf.find("\n\n") {
            let block: String = self.buf.drain(..end + 2).collect();
            let mut data = String::new();
            for line in block.lines() {
                // `event:` and `:` comments carry nothing we need: the type lives in the JSON.
                if let Some(value) = line.strip_prefix("data:") {
                    data.push_str(value.strip_prefix(' ').unwrap_or(value));
                    data.push('\n');
                }
            }
            let data = data.trim_end_matches('\n');
            if !data.is_empty() {
                payloads.push(data.to_owned());
            }
        }
        payloads
    }
}

#[cfg(test)]
mod tests {
    use super::Sse;

    #[test]
    fn splits_events_across_chunk_boundaries() {
        let mut sse = Sse::default();
        assert!(sse.push("data: {\"a\":").is_empty());
        assert_eq!(sse.push("1}\n\n"), vec!["{\"a\":1}"]);
    }

    #[test]
    fn returns_several_events_from_one_chunk() {
        let mut sse = Sse::default();
        assert_eq!(
            sse.push("data: one\n\ndata: two\n\n"),
            vec!["one".to_owned(), "two".to_owned()]
        );
    }

    #[test]
    fn accepts_crlf_and_ignores_comments() {
        let mut sse = Sse::default();
        assert_eq!(
            sse.push(": keep-alive\r\nevent: delta\r\ndata: hi\r\n\r\n"),
            vec!["hi".to_owned()]
        );
    }

    #[test]
    fn keeps_an_incomplete_tail() {
        let mut sse = Sse::default();
        assert!(sse.push("data: half").is_empty());
        assert_eq!(sse.push("\n\n"), vec!["half".to_owned()]);
    }
}

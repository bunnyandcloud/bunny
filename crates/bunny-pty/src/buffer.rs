use parking_lot::RwLock;

#[derive(Debug, Clone)]
struct StoredLine {
    offset: u64,
    data: String,
}

#[derive(Debug)]
pub struct CircularBuffer {
    lines: RwLock<Vec<StoredLine>>,
    max_lines: usize,
    byte_cap: usize,
    total_bytes: RwLock<usize>,
    offset: RwLock<u64>,
}

impl CircularBuffer {
    pub fn new(max_lines: usize, byte_cap: usize) -> Self {
        Self {
            lines: RwLock::new(Vec::new()),
            max_lines,
            byte_cap,
            total_bytes: RwLock::new(0),
            offset: RwLock::new(0),
        }
    }

    /// Append PTY data; returns the buffer offset after the last complete line in `data`.
    pub fn append(&self, data: &str) -> u64 {
        let mut lines = self.lines.write();
        let mut total = self.total_bytes.write();
        let mut end_offset = *self.offset.read();

        for chunk in data.split_inclusive('\n') {
            let line = chunk.to_string();
            let len = line.len();
            end_offset += 1;
            *total += len;
            lines.push(StoredLine {
                offset: end_offset,
                data: line,
            });

            while lines.len() > self.max_lines || *total > self.byte_cap {
                if let Some(removed) = lines.first() {
                    *total = total.saturating_sub(removed.data.len());
                    lines.remove(0);
                } else {
                    break;
                }
            }
        }

        *self.offset.write() = end_offset;
        end_offset
    }

    pub fn current_offset(&self) -> u64 {
        *self.offset.read()
    }

    /// Lines with `from < offset <= to` (attach stream only).
    pub fn replay_range(&self, from: u64, to: u64) -> Vec<(u64, String)> {
        let lines = self.lines.read();
        lines
            .iter()
            .filter(|l| l.offset > from && l.offset <= to)
            .map(|l| (l.offset, l.data.clone()))
            .collect()
    }

    pub fn replay_from(&self, from_offset: u64) -> Vec<(u64, String)> {
        let to = self.current_offset();
        self.replay_range(from_offset, to)
    }

    pub fn tail(&self, n: usize) -> String {
        let lines = self.lines.read();
        lines
            .iter()
            .rev()
            .take(n)
            .rev()
            .map(|l| l.data.as_str())
            .collect()
    }

    pub fn all_content(&self) -> String {
        let lines = self.lines.read();
        lines.iter().map(|l| l.data.as_str()).collect()
    }

    pub fn replace(&self, content: &str) {
        {
            let mut lines = self.lines.write();
            lines.clear();
            *self.total_bytes.write() = 0;
            *self.offset.write() = 0;
        }
        if !content.is_empty() {
            self.append(content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CircularBuffer;

    #[test]
    fn replay_range_is_half_open_and_fences_live() {
        let buf = CircularBuffer::new(100, 1024 * 1024);
        assert_eq!(buf.append("line1\nline2\n"), 2);
        assert_eq!(buf.append("line3\n"), 3);

        let catch_up = buf.replay_range(1, 2);
        assert_eq!(catch_up.len(), 1);
        assert_eq!(catch_up[0].0, 2);
        assert_eq!(catch_up[0].1, "line2\n");

        let live = buf.replay_range(2, 3);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].0, 3);

        assert!(buf.replay_range(3, 3).is_empty());
    }

    #[test]
    fn append_returns_end_offset_per_chunk() {
        let buf = CircularBuffer::new(100, 1024 * 1024);
        assert_eq!(buf.append("a\n"), 1);
        assert_eq!(buf.append("b\n"), 2);
        assert_eq!(buf.current_offset(), 2);
    }
}

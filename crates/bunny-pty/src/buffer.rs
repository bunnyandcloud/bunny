use parking_lot::RwLock;

#[derive(Debug)]
pub struct CircularBuffer {
    lines: RwLock<Vec<String>>,
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

    pub fn append(&self, data: &str) {
        let mut lines = self.lines.write();
        let mut total = self.total_bytes.write();
        let mut offset = self.offset.write();

        for chunk in data.split_inclusive('\n') {
            let line = chunk.to_string();
            let len = line.len();
            *total += len;
            lines.push(line);
            *offset += 1;

            while lines.len() > self.max_lines || *total > self.byte_cap {
                if let Some(removed) = lines.first().cloned() {
                    *total = total.saturating_sub(removed.len());
                    lines.remove(0);
                } else {
                    break;
                }
            }
        }
    }

    pub fn current_offset(&self) -> u64 {
        *self.offset.read()
    }

    pub fn replay_from(&self, from_offset: u64) -> Vec<(u64, String)> {
        let lines = self.lines.read();
        let start = from_offset.saturating_sub(1) as usize;
        lines
            .iter()
            .enumerate()
            .skip(start.min(lines.len()))
            .map(|(i, l)| ((i + 1) as u64, l.clone()))
            .collect()
    }

    pub fn tail(&self, n: usize) -> String {
        let lines = self.lines.read();
        lines.iter().rev().take(n).rev().cloned().collect()
    }

    pub fn all_content(&self) -> String {
        let lines = self.lines.read();
        lines.concat()
    }

    pub fn restore(&self, content: &str) {
        if !content.is_empty() {
            self.append(content);
        }
    }

    /// Replace buffer contents (used when loading persisted scrollback on reconnect).
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

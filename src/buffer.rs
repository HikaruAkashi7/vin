use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ropey::Rope;

pub struct Buffer {
    pub rope: Rope,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Whether the file used CRLF (normalized to LF internally, restored on save).
    pub crlf: bool,
    /// One indent level ("\t" or spaces), inferred from content like VSCode's detectIndentation.
    pub indent: String,
}

fn detect_indent(text: &str) -> String {
    let (mut tabs, mut spaces, mut two) = (0, 0, 0);
    for line in text.lines().take(10_000) {
        if line.starts_with('\t') {
            tabs += 1;
        } else {
            let n = line.len() - line.trim_start_matches(' ').len();
            if n > 0 && n < line.len() {
                spaces += 1;
                if n % 4 == 2 {
                    two += 1;
                }
            }
        }
    }
    if tabs > spaces {
        "\t".into()
    } else if two * 10 >= spaces && two > 0 {
        "  ".into()
    } else {
        "    ".into()
    }
}

fn read(path: &Path) -> io::Result<(String, bool)> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let crlf = text.contains("\r\n");
    Ok((
        if crlf {
            text.replace("\r\n", "\n")
        } else {
            text
        },
        crlf,
    ))
}

impl Buffer {
    pub fn empty() -> Self {
        Self {
            rope: Rope::new(),
            path: None,
            dirty: false,
            crlf: false,
            indent: "    ".into(),
        }
    }

    /// A missing file yields an empty buffer; the file is created on save.
    pub fn open(path: &Path) -> io::Result<Self> {
        let (text, crlf) = read(path)?;
        Ok(Self {
            indent: detect_indent(&text),
            rope: Rope::from_str(&text),
            path: Some(path.to_path_buf()),
            dirty: false,
            crlf,
        })
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| io::Error::other("no file path"))?;
        let mut text = self.rope.to_string();
        if self.crlf {
            text = text.replace('\n', "\r\n");
        }
        fs::write(path, text)?;
        self.dirty = false;
        Ok(())
    }

    /// Char count of the line, excluding the newline.
    pub fn line_len(&self, line: usize) -> usize {
        let slice = self.rope.line(line);
        let len = slice.len_chars();
        if len > 0 && slice.char(len - 1) == '\n' {
            len - 1
        } else {
            len
        }
    }

    pub fn last_line(&self) -> usize {
        self.rope.len_lines() - 1
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into())
    }
}

#[cfg(test)]
mod tests {
    use super::detect_indent;

    #[test]
    fn detects_indent_style() {
        assert_eq!(detect_indent("fn a() {\n\tb();\n}\n"), "\t");
        assert_eq!(detect_indent("a:\n  b:\n    c: 1\n  d: 2\n"), "  ");
        assert_eq!(
            detect_indent("fn a() {\n    b();\n        c();\n}\n"),
            "    "
        );
        assert_eq!(detect_indent("plain\ntext\n"), "    ");
    }
}

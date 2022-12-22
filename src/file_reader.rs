// A file reader which supports transclusion

use std::io::{Read, BufReader, Result};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::str::{self, FromStr};
use std::ffi::OsStr;

const BUFFER_SIZE: usize = 1024;
const MAX_TRANSCLUDE_DEPTH: usize = 10;

const TRANSCLUDE_START_BYTE: u8 = b'{';

const TRANSCLUDE_END_BYTE: u8 = b'}';

const ESCAPE_BYTE: u8 = b'\\';

const HTML_EXTENSION: &'static str = "html";


// Count occurrences of the escape character and return how to handle the next
// byte based on the number of consecutive occurences encountered so far.
struct EscapeCounter {
    consecutive_escapes: usize
}

enum EscapeResult {
    Parse,
    NoParse,
    Skip
}

impl EscapeCounter {
    pub fn new() -> Self {
        Self {
            consecutive_escapes: 0
        }
    }

    pub fn next(&mut self, byte: u8) -> EscapeResult {
        let r = if self.consecutive_escapes % 2 == 0 {
            if byte == ESCAPE_BYTE {
                EscapeResult::Skip
            } else {
                EscapeResult::Parse
            }
        } else {
            EscapeResult::NoParse
        };

        if byte == ESCAPE_BYTE {
            self.consecutive_escapes += 1;
        } else {
            self.consecutive_escapes = 0;
        }

        r
    }
}


struct ReaderData {
    reader: BufReader<File>,
    path: PathBuf,
    buf: [u8; BUFFER_SIZE],
    is_transclude_enabled: bool,
    start: usize,
    end: usize
}

pub struct FileReader {
    readers: Vec<ReaderData>
}

impl FileReader {
    pub fn new<P>(path: P) -> Result<Self>
    where P: AsRef<Path>
    {
        let mut new = Self {
            readers: Vec::with_capacity(MAX_TRANSCLUDE_DEPTH),
        };

        new.add_file(path)?;

        Ok(new)
    }

    fn add_file<P>(&mut self, path: P) -> Result<()>
    where P: AsRef<Path>
    {
        let path = path.as_ref();
        let file = File::open(path)?;

        if self.readers.len() < MAX_TRANSCLUDE_DEPTH {
            self.readers.push(ReaderData {
                reader: BufReader::new(file),
                path: PathBuf::from(path),
                buf: [0; BUFFER_SIZE],
                is_transclude_enabled: is_transclude_enabled(path),
                start: 0,
                end: 0
            });
        }

        Ok(())
    }

    pub fn get_size(&self) -> usize {
        // For files with transclusion enabled, we can't know the "true" size
        // without traversing the full file, but we also have to return a size
        // at the start of the HTTP response, so we use the same hack as with
        // index_executable & form_executable and respond with maximum size
        self.readers.get(0)
            .and_then(|r| if !r.is_transclude_enabled {
                    r.reader.get_ref()
                        .metadata()
                        .map(|m| m.len() as usize)
                        .ok()
                } else {
                    Some(usize::MAX)
                })
            .unwrap_or(usize::MAX)
    }
}

impl Read for FileReader {
    fn read(&mut self, read_into: &mut[u8]) -> Result<usize> {
        let mut bytes_written = 0;
        let mut ec = EscapeCounter::new();

        while bytes_written < read_into.len() {
            let d = match self.readers.last_mut() {
                Some(d) => d,
                None => break
            };

            if !d.is_transclude_enabled {
                // normal read, no parsing
                let bytes_read = d.reader.read(&mut read_into[bytes_written..])?;
                bytes_written += bytes_read;

                if bytes_read == 0 {
                    self.readers.pop();
                }

                continue;
            }

            if d.start >= d.end {
                d.end = d.reader.read(&mut d.buf)?;
                d.start = 0;

                if d.end == 0 {
                    self.readers.pop();
                    break;
                }
            }

            while d.start < d.end && bytes_written < read_into.len() {
                match ec.next(d.buf[d.start]) {
                    EscapeResult::Parse => match d.buf[d.start] {
                        TRANSCLUDE_START_BYTE => {
                            d.buf.copy_within(d.start.., 0);
                            d.end -= d.start;
                            d.end += d.reader.read(&mut d.buf[d.end..])?;
                            d.start = 0;

                            match transclude(d) {
                                Some(path) => drop(self.add_file(path)),
                                None => {
                                    let d = self.readers.last_mut().unwrap();
                                    d.start = 0;
                                    ec.consecutive_escapes = 1;
                                }
                            }

                            break;
                        },
                        _ => {
                            read_into[bytes_written] = d.buf[d.start];
                            bytes_written += 1;
                        }
                    },
                    EscapeResult::NoParse => {
                        read_into[bytes_written] = d.buf[d.start];
                        bytes_written += 1;
                    },
                    EscapeResult::Skip => {}
                }

                d.start += 1;
            }
        }

        Ok(bytes_written)
    }
}

fn transclude(d: &mut ReaderData) -> Option<PathBuf> {
    let mut bytes_written = 0;
    let mut ec = EscapeCounter::new();

    while d.start < d.end {
        match ec.next(d.buf[d.start]) {
            EscapeResult::Parse => match d.buf[d.start] {
                TRANSCLUDE_END_BYTE => {
                    d.start += 1;
                    let s = str::from_utf8(&d.buf[1..bytes_written]).ok()?;
                    let path = PathBuf::from_str(s).ok()?;

                    let path = if path.is_relative() {
                        d.path.parent().map(|p| p.join(path))?
                    } else {
                        path
                    };

                    if path.exists() {
                        return Some(path);
                    } else {
                        return None;
                    }
                },
                _ => {
                    d.buf[bytes_written] = d.buf[d.start];
                    bytes_written += 1;
                }
            },
            EscapeResult::NoParse => {
                d.buf[bytes_written] = d.buf[d.start];
                bytes_written += 1;
            },
            EscapeResult::Skip => {}
        }

        d.start += 1;
    }

    None
}

fn is_transclude_enabled(path: &Path) -> bool {
    let ext = path.extension()
        .unwrap_or(OsStr::new(""))
        .to_ascii_lowercase();

    ext == HTML_EXTENSION
}

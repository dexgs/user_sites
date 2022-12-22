use std::path::{Path, PathBuf};
use std::fs::DirEntry;
use std::io::{Result, Read};
use std::cmp::{self, Ordering};
use crate::file_reader::FileReader;
use chrono::{DateTime, Local};
use urlencoding::encode;

fn is_special_file_name<S>(file_name: S) -> bool
where S: AsRef<str> {
    let file_name = file_name.as_ref();

    match file_name {
        "header.html" | "footer.html" | "styles.css" | "title" => true,
        _ => false
    }
}

pub fn generate_index<F: 'static>(
    path: impl AsRef<Path>, header: Option<&str>, f: F,
    page_size: usize, page_number: usize) -> Result<String>
where F: Fn(Result<DirEntry>) -> Option<DirEntry> {
    let path = path.as_ref();
    let mut entries: Vec<DirEntry> = path
        .read_dir()?
        .filter_map(f)
        .filter(|file| {
            file.metadata().is_ok() && file.metadata().unwrap().modified().is_ok()
            && !is_special_file_name(file.file_name().to_string_lossy())
        })
        .collect();
    // Sort entries (Directories first, then files) where each group is sorted
    // chronologically by last modified date. TOP (newest) -> BOTTOM (oldest).
    entries.sort_unstable_by(|e1, e2| {
        let (m1, m2) = (e1.metadata().unwrap(), e2.metadata().unwrap());
        if m1.is_file() && m2.is_dir() {
            return Ordering::Greater;
        } else if m1.is_dir() && m2.is_file() {
            return Ordering::Less;
        }
        m2.modified().unwrap().cmp(&m1.modified().unwrap())
    });

    // Skip the "/home/user/www" and just display the rest of the path
    let display_path = format!("{}", path.components().skip(4).fold(PathBuf::new(), |mut p, e| { p.push(e); p }).to_str().unwrap_or(""));

    let title = if let Some(head) = header {
        head.to_owned()
    } else if let Some(head) = read_file(path.join("title")) {
        head.trim().to_owned()
    } else {
        display_path.to_owned()
    };

    let head = format!("<title>{}</title>\n<link rel=\"stylesheet\" href=\"styles.css\"/>\n<base href=\"./\"/>", title);

    // Set the page heading
    let header = if let Some(header) = header {
        format!("   <h1>{}</h1>", header)
    } else if let Some(header) = read_file(path.join("header.html")) {
        header.trim_end().to_owned()
    } else {
        format!("    <h1>{}</h1>", display_path)
    };

    // Build the page body
    let mut body = header;

    if page_size == 0 {
        // No pagination
        body.push_str("
            <ol class=\"entries\">");
        body.push_str("
            <a href=\"../\">../<br/></a>");

        for entry in entries {
            body.push_str(&format_entry(&entry));
        }

        body.push_str("
            </ol>");
    } else {
        // Pagination
        let num_pages = (entries.len() + page_size - 1) / page_size;
        let last_index = entries.len() - 1;
        let start = cmp::min(page_number * page_size, last_index);
        let end = cmp::min(start + page_size - 1, last_index);

        body.push_str(&format!("
            <ol class=\"entries\" start=\"{}\">", start + 1));
        body.push_str("
            <a href=\"../\">../</a>");

        for entry in &entries[start..=end] {
            body.push_str(&format_entry(&entry));
        }

        body.push_str("
            </ol>
            <nav class=\"pagination\">");


        let has_prev_page = start > 0;
        let has_next_page = end < last_index;

        if has_prev_page {
            body.push_str(&format!("\n<a href=\".?p={}&n={}\">Prev. Page</a>",
                                  page_number, page_size));
        }

        body.push_str(&format!("\n<form>
                      <span class=\"page-number\" data-num-pages=\"{np}\">
                          <label for=\"page-number-input\">Page #</label>
                          <input id=\"page-number-input\" type=\"number\" name=\"p\" value=\"{p}\" min=\"1\" max=\"{np}\" size=\"4\"/>
                      </span>
                      <span class=\"page-size\">
                          <label for=\"page-size-input\">Page Size</label>
                          <input id=\"page-size-input\" type=\"number\" name=\"n\" value=\"{n}\" min=\"1\" width=\"2\" size=\"4\"/>
                          <input type=\"submit\" value=\"Go\"/>
                      </span>
                      </form>",
                      p = page_number + 1, np = num_pages, n = page_size));

        if has_next_page {
            body.push_str(&format!("\n<a href=\".?p={}&n={}\">Next Page</a>",
                                  page_number + 2, page_size));
        }

        body.push_str("</nav>")
    }

    // Try loading a footer if one is available
    if let Some(footer) = read_file(path.join("footer.html")) { 
        body.push_str(footer.trim_end())
    }

    Ok(format_html!(head, body))
}

fn format_entry(entry: &DirEntry) -> String {
    let metadata = entry.metadata().unwrap();
    let last_modified = DateTime::<Local>::from(metadata.modified().unwrap()).format("%d/%m/%Y %T");
    let size = metadata.len();

    let name = entry.file_name().to_str().unwrap_or("").to_string();
    let href = encode(&name);

    format!("<li><a href=\"{href}\" data-modified=\"{last_modified}\" data-size=\"{size}\">{name}<br/></a></li>")
}

fn read_file(file_path: PathBuf) -> Option<String> {
    let mut s = String::new();
    let mut reader = FileReader::new(file_path).ok()?;
    reader.read_to_string(&mut s).ok()?;
    s.push_str("\n");
    Some(s)
}

use std::path::{Path, PathBuf};
use std::fs::{DirEntry, File};
use std::io::{Result, Read};
use std::cmp::Ordering;
use chrono::{DateTime, Local};

// Add a CSS rule to hide the special files read by generate_index and attempt
// to load a stylesheet
const CSS: &'static str = "
        <style>
            a[href=\"footer.html\"], a[href=\"header.html\"], a[href=\"styles.css\"], a[href=\"title\"] {
                display: none !important;
            }
        </style>
        <link rel=\"stylesheet\" href=\"styles.css\"/>";

pub fn generate_index<F: 'static>(path: impl AsRef<Path>, header: Option<&str>, f: F) -> Result<String>
where F: Fn(Result<DirEntry>) -> Option<DirEntry> {
    let path = path.as_ref();
    let mut entries: Vec<DirEntry> = path
        .read_dir()?
        .filter_map(f)
        .filter(|f| {
            f.metadata().is_ok() && f.metadata().unwrap().modified().is_ok()
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

    let title = if let Some(title) = header {
        title.to_owned()
    } else if let Some(title) = read_file(path.join("title")) {
        title.trim().to_owned()
    } else {
        display_path.clone()
    };

    // Set the page heading
    let mut header = if let Some(header) = header {
        format!("   <h1>{}</h1>", header)
    } else if let Some(header) = read_file(path.join("header.html")) {
        header.trim().to_owned()
    } else {
        format!("    <h1>{}</h1>", display_path)
    };
    header.push_str(CSS);

    // Build the page body
    let mut body = header;
    body.push_str("
        <div class=\"entries\">");
    body.push_str("
            <a href=\"../\">../<br/></a>");
    for entry in entries {
        body.push_str(&format_entry(&entry));
    }
    body.push_str("
        </div>");

    // Try loading a footer if one is available
    if let Some(footer) = read_file(path.join("footer.html")) { 
        body.push_str(footer.trim())
    }

    Ok(format_html!(title, body))
}

fn format_entry(entry: &DirEntry) -> String {
    let metadata = entry.metadata().unwrap();
    let last_modified = DateTime::<Local>::from(metadata.modified().unwrap()).format("%d/%m/%Y %T");
    let size = metadata.len();
    let mut name = entry.file_name().to_str().unwrap_or("").to_owned();
    if metadata.is_dir() {
        name.push_str("/");
    }
    format!("
            <a href=\"{name}\" data-modified=\"{last_modified}\" data-size=\"{size}\">{name}<br/></a>",
            name=name, last_modified=last_modified, size=size)
}

fn read_file(file_path: PathBuf) -> Option<String> {
    let mut s = String::new();
    let mut file = File::open(file_path).ok()?;
    file.read_to_string(&mut s).ok()?;
    s.push_str("\n");
    Some(s)
}

#[macro_use]
mod html_common;
mod error_pages;
mod auto_index;
mod file_reader;

use file_reader::FileReader;

use std::env;
use micro_http_server::{MicroHTTP, Client, Request, FormData};
use anyhow::Error;
use std::thread;
use std::path::{Path, PathBuf, Component};
use std::fs::{OpenOptions, File, metadata};
use std::io::{self, ErrorKind, Result, Read, Write, BufRead, BufReader};
use std::result::Result as StdResult;
use std::process::{Command, Stdio};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use httpdate::fmt_http_date;
use urlencoding;


fn main() -> StdResult<(), Error> {
    let port = env::args().nth(1).unwrap().parse()?;
    let upstream = Arc::new(env::args().nth(2).unwrap_or(String::new()));
    let server = MicroHTTP::new(("0.0.0.0", port))?;

    loop {
        let client = server.next_client()?;
        if let Some(client) = client {
            let upstream = upstream.clone();
            thread::spawn(move || handle_client(client, upstream));
        }
    }
}


fn handle_client(mut client: Client, upstream: Arc<String>) -> Option<()> {
    let (path_string, request) = client.request_mut().take()?;

    let path = if path_string.starts_with("/") {
        PathBuf::from(&path_string[1..])
    } else {
        PathBuf::from(&path_string)
    };

    // Prevent accessing directories that are not descendants of /home by disabling
    // using parent directories (../) in paths.
    let mut components = path.components().filter(|c| {
        match c {
            Component::ParentDir => false,
            _ => true
        }
    });
    // Assuming first component is user name
    let user = components.nth(0);
    let file_path = match user {
        Some(user) => {
            let path = components.fold(PathBuf::new(), |mut p, c| { p.push(c); p });
            Path::new("/home").join(user.as_os_str()).join("www").join(path)
        },
        None => PathBuf::from("/home")
    };

    let response_status = if file_path.is_dir() && !path_string.ends_with("/") {
        client.respond("302 Found", &[], &vec![format!("Location: {upstream}{}/", path.display())]).map(|_| ())
    } else {
        match request {
            Request::GET(query, headers) => handle_get(&file_path, query, headers, client),
            Request::POST(_, mut data) => handle_post(&file_path, &mut data, client)
        }
    };

    if let Err(e) = response_status {
        eprintln!("{}", e);
    }

    Some(())
}


// Return file handle and reported file size in bytes
fn open_file<P>(file_path: P) -> Result<(BufReader<File>, usize)>
where P: AsRef<Path>
{
    let file_handle = OpenOptions::new()
        .read(true)
        .write(false)
        .open(file_path.as_ref())?;
    let file_size = file_handle.metadata()?.len() as usize;

    Ok((BufReader::new(file_handle), file_size))
}


// Helper function to respond to GET requests
fn handle_get(
    file_path: &PathBuf, mut query: HashMap<String, String>,
    headers: HashMap<String, String>, mut client: Client) -> Result<()>
{
    let mut file_path = file_path.to_owned();

    if file_path.is_dir() {
        // Only modify the path if the new destination exists
        file_path.push("index_executable");

        if !file_path.exists() || !file_path.is_file() {
            file_path.pop();

            file_path.push("index.html");

            if !file_path.exists() || !file_path.is_file() {
                file_path.pop();
            }
        }
    }

    if file_path.exists()
        && !file_path.ends_with("form_executable")
        && !file_path.ends_with("allowed_variables")
    {
        if file_path.is_dir() {
            let page_size = query.get("n")
                .and_then(|s| s.parse().ok()).unwrap_or(0);
            let page_number = query.get("p")
                .and_then(|s| s.parse().ok()).unwrap_or(1) - 1;

            // serve autoindex
            let index = if &file_path == &Path::new("/home") {
                auto_index::generate_index(&file_path, Some("People"), |entry| {
                    let entry = entry.ok()?;
                    if entry.file_type().ok()?.is_dir() && entry.path().join("www").exists() {
                        return Some(entry);
                    } else {
                        None
                    }
                }, page_size, page_number)?
            } else {
                auto_index::generate_index(
                    &file_path, None, |entry| { entry.ok() },
                    page_size, page_number)?
            };
            client.respond(
                "200 OK",
                &index.as_bytes(),
                &vec!["Cache-Control: max-age=30".to_owned()])?;
        } else if file_path.ends_with("index_executable") {
            let allowed_variables_file = get_adjacent_allowed_variables_file(&file_path)?;
            let allowed_variables = get_allowed_variables(allowed_variables_file)?;
            filter_env_variables(&mut query, &allowed_variables);
            // run program
            let child_process = Command::new(file_path.as_os_str())
                .envs(query)
                .arg(file_path)
                .stdout(Stdio::piped())
                .spawn()?;
            // This is a really nasty hack, but to get around the requirement of
            // the content length header, just set it to the max possible value.
            // modern browsers will be able to handle this even if it's not standard.
            client.respond_chunked(
                "200 OK",
                child_process.stdout.expect("Capturing stdout"),
                usize::MAX,
                &vec!["Cache-Control: no-cache".to_owned()])?;
        } else {
            // serve file
            let modified = metadata(&file_path).and_then(|m| m.modified())?;
            let modified_string = fmt_http_date(modified);
            if let Some(modified_since) = headers.get("if-modified-since") {
                if modified_since == &modified_string {
                    client.respond("304 Not Modified", b"", &vec![])?;
                    return Ok(());
                }
            }

            let headers = vec![
                format!("Last-Modified: {}", modified_string),
                "Cache-Control: max-age=30".to_owned()
            ];

            match FileReader::new(&file_path) {
                Ok(r) => {
                    let size = r.get_size();
                    client.respond_chunked("200 OK", r, size, &headers)?;
                },
                Err(_) => {
                    client.respond("500 Internal Server Error", error_pages::ERROR_500.as_bytes(), &vec![])?;
                }
            }
        }
    } else {
        client.respond("404 Not Found", error_pages::ERROR_404.as_bytes(), &vec![])?;
    }
    Ok(())
}


// Helper function to respond to POST requests
fn handle_post(file_path: &PathBuf, data: &mut Option<FormData>, mut client: Client) -> Result<()> {
    let mut file_path = file_path.to_owned();

    // Unlike GET requests, POST requests MUST be handled by an executable
    if !file_path.ends_with("form_executable") {
        file_path.push("form_executable");
    }
    // If the executable path does not exist (or the points to a directory), exit.
    if !file_path.exists() || !file_path.is_file() {
        client.respond("404 Not Found", error_pages::ERROR_404.as_bytes(), &vec![])?;
        return Ok(());
    }
    let executable_path = file_path.as_os_str();
    let mut command = Command::new(executable_path);
    command.arg(&file_path)
        .stdout(Stdio::piped())
        .stdin(Stdio::null());
    // Different data will be fed to the executable depending on how the form
    // was encoded.
    match data.as_mut() {
        // URL encoded form
        Some(FormData::KeyVal(vars)) => {
            let allowed_variables_file = get_adjacent_allowed_variables_file(&file_path)?;
            let allowed_variables = get_allowed_variables(allowed_variables_file)?;
            filter_env_variables(vars, &allowed_variables);
            command.envs(vars);
        },
        // Plaintext form
        Some(FormData::Text(text)) => {
            command.arg(text);
        },
        // Multipart form
        Some(FormData::Stream(_)) => {
            command.stdin(Stdio::piped());
        },
        _ => {}
    }
    let mut child_process = command.spawn()?;
    if let Some(mut stdin) = child_process.stdin.take() {
        if let Some(FormData::Stream(mut reader)) = data.take() {
            let mut buffer = [0; 4096];
            while let Ok(bytes_read) = reader.read(&mut buffer) {
                if bytes_read == 0 { break; }
                stdin.write_all(&buffer)
                    .expect("Writing to stdin");
            }
        }
    }
    client.respond_ok_chunked(child_process.stdout.expect("Capturing stdout"), usize::MAX)?;
    Ok(())
}


// Filter out variable definitions that are already present
fn filter_env_variables(vars: &mut HashMap<String, String>, allowed_variables: &HashSet<String>) {
    for var in vars.keys().map(|k| k.to_owned()).collect::<Vec<String>>() {
        // Remove var if it is already defined or all caps
        if let Ok(var) = urlencoding::decode(&var) {
            let var = var.to_string();
            if env::var(&var).is_ok()
                || var == var.to_uppercase()
                || !allowed_variables.contains(&var)
            {
                vars.remove(&var);
            }
        }
    }
}

// Get a reader for a file named "allowed_varibles" adjacent to the file located
// at the given path.
fn get_adjacent_allowed_variables_file<P>(path: P) -> Result<BufReader<File>>
where P: AsRef<Path>
{
    let allowed_variables_path = path
        .as_ref()
        .parent()
        .ok_or(io::Error::from(ErrorKind::Other))?
        .join("allowed_variables");
    Ok(open_file(allowed_variables_path)?.0)
}

// Read the allowed variable keys from a reader
fn get_allowed_variables<R>(reader: R) -> Result<HashSet<String>>
where R: BufRead {
    let mut allowed_variables = HashSet::new();

    for line in reader.lines() {
        allowed_variables.insert(line?.trim().to_owned());
    }

    Ok(allowed_variables)
}

use std::env;
use micro_http_server::{MicroHTTP, Client, Request, FormData};
use anyhow::Error;
use std::thread;
use std::path::{Path, PathBuf, Component};
use std::fs::{OpenOptions, File, metadata};
use std::io::{Result, Read, Write};
use std::result::Result as StdResult;
use std::process::{Command, Stdio};
use std::collections::HashMap;
use httpdate::fmt_http_date;
use urlencoding;

#[macro_use]
mod html_common;
mod error_pages;
mod auto_index;


fn main() -> StdResult<(), Error> {
    let port = env::args().nth(1).unwrap().parse()?;
    let server = MicroHTTP::new(("0.0.0.0", port))?;

    loop {
        let client = server.next_client()?;
        if let Some(client) = client {
            thread::spawn(move || handle_client(client));
        }
    }
}


fn handle_client(mut client: Client) -> Option<()> {
    let (path, request) = client.request_mut().take()?;
    // Prevent accessing directories that are not descendants of /home by disabling
    // using parent directories (../) in paths.
    let mut components = path.components().filter(|c| {
        match c {
            Component::ParentDir => false,
            _ => true
        }
    });
    // Assuming first component is / and second is user name
    let user = components.nth(1);
    let file_path = match user {
        Some(user) => {
            let path = components.fold(PathBuf::new(), |mut p, c| { p.push(c); p });
            Path::new("/home").join(user).join("www").join(path)
        },
        None => PathBuf::from("/home")
    };

    let response_status = match request {
        Request::GET(query, _) => handle_get(&file_path, query, client),
        Request::POST(_, mut data) => handle_post(&file_path, &mut data, client)
    };

    if let Err(e) = response_status {
        eprintln!("{}", e);
    }

    Some(())
}


// Return file handle and reported file size in bytes
fn open_file<P>(file_path: P) -> Result<(File, usize)> 
where P: AsRef<Path>
{
    let file_handle = OpenOptions::new().read(true).write(false).open(file_path.as_ref())?;
    let file_size = file_handle.metadata()?.len() as usize;
    Ok((file_handle, file_size))
}


// Helper function to respond to GET requests
fn handle_get(
    file_path: &PathBuf, mut query: HashMap<String, String>, mut client: Client) -> Result<()> 
{
    let mut file_path = file_path.to_owned();

    if file_path.is_dir() {
        // Only modify the path if the new destination exists
        file_path.push("index_executable");
        if !file_path.exists() { file_path.pop(); }
        file_path.push("index.html");
        if !file_path.exists() { file_path.pop(); }
    }

    if file_path.exists() && !file_path.ends_with("form_executable") {
        if file_path.is_dir() {
            // serve autoindex
            let index = if &file_path == &Path::new("/home") {
                auto_index::generate_index(&file_path, Some("People"), |entry| {
                    let entry = entry.ok()?;
                    if entry.file_type().ok()?.is_dir() && entry.path().join("www").exists() {
                        return Some(entry);
                    } else {
                        None
                    }
                })?
            } else {
                auto_index::generate_index(&file_path, None, |entry| { entry.ok() })?
            };
            client.respond(
                "200 OK",
                &index.as_bytes(),
                &vec!["Cache-Control: max-age=5".to_owned()])?;
        } else if file_path.ends_with("index_executable") {
            filter_env_variables(&mut query);
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
            let headers = vec![
                format!("Last-Modified: {}", fmt_http_date(modified))
            ];
            match open_file(&file_path) {
                Ok((file, size)) => {
                    client.respond_chunked("200 OK", file, size, &headers)?
                },
                Err(_) => client.respond("500 Internal Server Error", error_pages::ERROR_500.as_bytes(), &vec![])?
            };
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
    command.arg(file_path)
        .stdout(Stdio::piped())
        .stdin(Stdio::null());
    // Different data will be fed to the executable depending on how the form
    // was encoded.
    match data.as_mut() {
        // URL encoded form
        Some(FormData::KeyVal(vars)) => {
            filter_env_variables(vars);
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
fn filter_env_variables(vars: &mut HashMap<String, String>) {
    for var in vars.keys().map(|k| k.to_owned()).collect::<Vec<String>>() {
        // Remove var if it is already defined or all caps
        if let Ok(var) = urlencoding::decode(&var) {
            let var = var.to_string();
            if env::var(&var).is_ok() || var == var.to_uppercase() {
                vars.remove(&var);
            }
        }
    }
}

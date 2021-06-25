use std::env;
use micro_http_server::{MicroHTTP, Client};
use anyhow::Error;
use std::thread;
use std::path::{Path, PathBuf, Component};
use std::fs::{OpenOptions, File};
use std::io::Result;
use std::result::Result as StdResult;
use std::sync::{Arc, Mutex};
use std::process::{Command, Stdio};
use std::collections::HashMap;
use std::time::Duration;

#[macro_use]
mod html_common;
mod error_pages;
mod auto_index;

type SharedData = Arc<Mutex<HashMap<PathBuf, (usize, Option<String>)>>>;

// The max number of clients which are allowed to view a single page at once
const MAX_CONCURRENT_ACCESSORS: usize = 5000;

fn main() -> StdResult<(), Error> {
    let port = env::args().nth(1).unwrap().parse()?;
    let server = MicroHTTP::new(("0.0.0.0", port))?;

    let shared = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let client = server.next_client()?;
        if let Some(client) = client {
            let shared = shared.clone();
            thread::spawn(move || handle_client(client, shared));
        }
    }
}

fn handle_client(mut client: Client, shared: SharedData) -> Option<()> {
    let request = client.request().as_ref()?;
    let path = PathBuf::from(request);
    // Prevent accessing directories that are not descendants of /home by disabling
    // using parent directories (../) in paths.
    let mut components = path.components().filter(|c| {
        if let Component::ParentDir = c {
            false
        } else {
            true
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

    if get_concurrent_accessors(&shared, &path) > MAX_CONCURRENT_ACCESSORS {
        // Tell the client the server is too busy to serve the request
        client.respond("503 Service Unavailable", error_pages::ERROR_503.as_bytes(), &vec![])
            .expect("Reporting error to client");
        return None;
    }

    update_shared_data(&shared, &file_path, UpdateType::Accessing);
    if let Err(e) = handle_get(&file_path, "", client, &shared) { eprintln!("{}", e); }
    update_shared_data(&shared, &file_path, UpdateType::Closing);

    Some(())
}

// Return file handle and reported file size in bytes
fn open_file(file_path: PathBuf) -> Result<(File, usize)> {
    let file_handle = OpenOptions::new().read(true).write(false).open(file_path)?;
    let file_size = file_handle.metadata()?.len() as usize;
    Ok((file_handle, file_size))
}

// Helper function to respond to GET requests
fn handle_get(file_path: &PathBuf, query: &str, mut client: Client, shared: &SharedData) -> Result<()> {
    let mut file_path = file_path.to_owned();

    if file_path.is_dir() {
        // Only modify the path if the new destination exists
        file_path.push("index_executable");
        if !file_path.exists() { file_path.pop(); }
        file_path.push("index.html");
        if !file_path.exists() { file_path.pop(); }
    }

    if file_path.exists() {
        if file_path.is_dir() {
            // serve autoindex
            let index = if let Some(index) = get_cache(&shared, &file_path) {
                index
            } else {
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
                set_cache(shared, &file_path, &index);
                index
            };
            client.respond_ok(index.as_bytes())?;
        } else if file_path.ends_with("index_executable") {
            // run program
            let child_process = Command::new(file_path.as_os_str())
                .arg(file_path)
                .arg(query)
                .stdout(Stdio::piped())
                .spawn()?;
            // A value of 1 for the Content-Size header is not valid HTTP, but modern
            // browsers can still handle it and being able to do this is much nicer than
            // having to read the whole output into a buffer in order to get an accurate
            // size before sending it.
            client.respond_ok_chunked(child_process.stdout.expect("Capturing stdout"), 1)?;
        } else {
            // serve file
            match open_file(file_path) {
                Ok((file, size)) => client.respond_ok_chunked(file, size)?,
                Err(_) => client.respond("500 Internal Server Error", error_pages::ERROR_500.as_bytes(), &vec![])?
            };
        }
    } else {
        client.respond("404 Not Found", error_pages::ERROR_404.as_bytes(), &vec![])?;
    }
    Ok(())
}

enum UpdateType {
    Accessing,
    Closing
}

fn update_shared_data(shared: &SharedData, path: &PathBuf, update_type: UpdateType) {
    let mut shared = shared.lock().unwrap();
    match shared.get_mut(path) {
        Some((num_accessors, _)) => {
            match update_type {
                UpdateType::Accessing => *num_accessors += 1,
                UpdateType::Closing => {
                    std::thread::sleep(Duration::from_secs(1));
                    *num_accessors -= 1
                }
            }
            if *num_accessors == 0 {
                shared.remove(path);
            }
        },
        None => {
            if let UpdateType::Accessing = update_type {
                shared.insert(path.to_owned(), (1, None));
            }
        }
    }
}

fn set_cache(shared: &SharedData, path: &PathBuf, new_cache: &String) {
    if let Some((_, cache)) = shared.lock().unwrap().get_mut(path) {
        *cache = Some(new_cache.to_owned());
    }
}

fn get_cache(shared: &SharedData, path: &PathBuf) -> Option<String> {
    Some(shared.lock().unwrap().get(path)?.1.as_ref()?.to_owned())
}

fn get_concurrent_accessors(shared: &SharedData, path: &PathBuf) -> usize {
    if let Some((accessors, _)) = shared.lock().unwrap().get(path) {
        *accessors
    } else {
        0
    }
}

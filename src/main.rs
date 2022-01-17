use std::env;
use micro_http_server::{MicroHTTP, Client, Request, FormData};
use anyhow::Error;
use std::thread;
use std::path::{Path, PathBuf, Component};
use std::fs::{OpenOptions, File};
use std::io::{Result, Read, Write};
use std::result::Result as StdResult;
use std::sync::{Arc, Mutex};
use std::process::{Command, Stdio};
use std::collections::HashMap;
use std::time::Duration;
use bytes::{Bytes, BytesMut};

#[macro_use]
mod html_common;
mod error_pages;
mod auto_index;

type SharedData = Arc<Mutex<(HashMap<PathBuf, (usize, Option<Bytes>)>, usize)>>;

// The max number of clients which are allowed to view a single page at once
const MAX_CONCURRENT_ACCESSORS: usize = 5000;
// The maximum file size to cache to memory
const MAX_CACHE_FILE_SIZE: usize = 50 * 1024 * 1024; // 50 MiB
// The maximum cache size allowed
const MAX_CACHE_SIZE: usize = 1 * 1024 * 1024 * 1024; // 1 GiB


fn main() -> StdResult<(), Error> {
    let port = env::args().nth(1).unwrap().parse()?;
    let server = MicroHTTP::new(("0.0.0.0", port))?;

    let shared = Arc::new(Mutex::new((HashMap::new(), 0)));

    loop {
        let client = server.next_client()?;
        if let Some(client) = client {
            let shared = shared.clone();
            thread::spawn(move || handle_client(client, shared));
        }
    }
}


fn handle_client(mut client: Client, shared: SharedData) -> Option<()> {
    let (path, request) = client.request_mut().take()?;
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

    let access_number = update_shared_data(&shared, &file_path, UpdateType::Accessing);
    if let Err(e) = match request {
        Request::GET(query, _) => handle_get(&file_path, query, client, &shared, access_number),
        Request::POST(_, mut data) => handle_post(&file_path, &mut data, client)
    } { eprintln!("{}", e); }
    update_shared_data(&shared, &file_path, UpdateType::Closing);

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


// Wrapper around File that cached reads to a buffer
struct CachedReadFile {
    file: File,
    cache: BytesMut
}

impl Read for CachedReadFile {
    fn read(&mut self, buf: &mut[u8]) -> Result<usize> {
        let bytes_read = self.file.read(buf)?;
        self.cache.extend_from_slice(&buf[..bytes_read]);
        Ok(bytes_read)
    }
}


// Helper function to respond to GET requests
fn handle_get(file_path: &PathBuf, mut query: HashMap<String, String>, mut client: Client, shared: &SharedData, access_number: usize) -> Result<()> {
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
            if let Some(index) = get_cache(&shared, &file_path) {
                client.respond_ok(&index)?;
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
                client.respond_ok(&index.as_bytes())?;
                set_cache(shared, &file_path, index.clone().into());
            };
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
            client.respond_ok_chunked(child_process.stdout.expect("Capturing stdout"), usize::MAX)?;
        } else {
            // serve file
            match get_cache(shared, &file_path) {
                Some(cached) => {
                    client.respond_ok(&cached)?;
                },
                None => {
                    match open_file(&file_path) {
                            Ok((file, size)) => {
                                if access_number == 1 && size < MAX_CACHE_FILE_SIZE
                                && get_cache_size(shared) < MAX_CACHE_SIZE
                                {
                                    // If this client is the second to concurrently
                                    // access the file, cache it to reduce I/O
                                    let mut cached_read_file = CachedReadFile {
                                        file: file,
                                        cache: BytesMut::with_capacity(size)
                                    };
                                    let bytes_read = client.respond_ok_chunked(&mut cached_read_file, size)?;
                                    set_cache(shared, &file_path, cached_read_file.cache.into());
                                    bytes_read
                                } else {
                                    client.respond_ok_chunked(file, size)?
                                }
                            },
                            Err(_) => client.respond("500 Internal Server Error", error_pages::ERROR_500.as_bytes(), &vec![])?
                    };
                }
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
            thread::spawn(move || {
                let mut buffer = [0; 4096];
                while let Ok(bytes_read) = reader.read(&mut buffer) {
                    if bytes_read == 0 { break; }
                    stdin.write_all(&buffer)
                        .expect("Writing to stdin");
                }
            });
        }
    }
    client.respond_ok_chunked(child_process.stdout.expect("Capturing stdout"), usize::MAX)?;
    Ok(())
}


enum UpdateType {
    Accessing,
    Closing
}


// Update the record of the number of clients accessing a resource, removing it
// if there are no more accessors. Return how many accessors came before this one
// if UpdateType is `Accessing`. Otherwise return 0.
fn update_shared_data(shared: &SharedData, path: &PathBuf, update_type: UpdateType) -> usize {
    let lock = &mut shared.lock().unwrap().0;

    let mut access_number = 0;

    let do_decrement = match lock.get_mut(path) {
        Some((num_accessors, _)) => {
            match update_type {
                UpdateType::Accessing => {
                    access_number = *num_accessors;
                    *num_accessors += 1;
                    false
                },
                UpdateType::Closing => true
            }
        },
        None => {
            if let UpdateType::Accessing = update_type {
                lock.insert(path.to_owned(), (1, None));
            }
            false
        }
    };
    drop(lock);
    if do_decrement {
        std::thread::sleep(Duration::from_secs(1));
        let (map, cache_size) = &mut *shared.lock().unwrap();
        if let Some((num_accessors, cache)) = map.get_mut(path) {
            *num_accessors -= 1;
            if *num_accessors == 0 {
                *cache_size -= cache.as_ref().map_or(0, |c| c.len());
                map.remove(path);
            }
        }
    }

    return access_number;
}


fn set_cache(shared: &SharedData, path: &PathBuf, new_cache: Bytes) {
    let (map, cache_size) = &mut *shared.lock().unwrap();

    if let Some((_, cache)) = map.get_mut(path) {
        let new_cache_size = *cache_size - cache.as_ref().map_or(0, |c| c.len());
        if new_cache_size + new_cache.len() < MAX_CACHE_SIZE {
            *cache_size = new_cache_size + new_cache.len();
            *cache = Some(new_cache);
        }
    }
}

fn get_cache_size(shared: &SharedData) -> usize {
    shared.lock().unwrap().1
}


fn get_cache(shared: &SharedData, path: &PathBuf) -> Option<Bytes> {
    Some(shared.lock().unwrap().0.get(path)?.1.as_ref()?.to_owned())
}


fn get_concurrent_accessors(shared: &SharedData, path: &PathBuf) -> usize {
    if let Some((accessors, _)) = shared.lock().unwrap().0.get(path) {
        *accessors
    } else {
        0
    }
}


// Filter out variable definitions that are already present
fn filter_env_variables(vars: &mut HashMap<String, String>) {
    for var in vars.keys().map(|k| k.to_owned()).collect::<Vec<String>>() {
        // Remove var if it is already defined or all caps
        if env::var(&var).is_ok() || var == var.to_uppercase() {
            vars.remove(&var);
        }
    }
}

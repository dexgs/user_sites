use std::env;
use micro_http_server::{MicroHTTP, Client};
use anyhow::Error;
use std::thread;
use std::path::{Path, PathBuf, Component};
use std::fs::{OpenOptions, File};
use std::io::Result;
use std::result::Result as StdResult;

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
    let user = components.nth(1)?;
    if let Component::Normal(_) = user {
        let home_dir = Path::new("/home").join(user).join("www");
        let mut file_path = home_dir.join(
            components.fold(PathBuf::new(), |mut p, c| { p.push(c); p }));

        if file_path.is_dir() {
            file_path.push("index.html");
        }

        if file_path.exists() {
            match open_file(file_path) {
                Ok((file, size)) => client.respond_ok_chunked(file, size)
                    .expect("Serving file to client"),
                Err(_) => client.respond("500 Internal Server Error", &[], &vec![])
                    .expect("Reporting error to client"),
            };
        } else {
            client.respond("404 Not Found", &[], &vec![])
                .expect("Reporting error to client");
        }
    }
    None
}

// Return file handle and reported file size in bytes
fn open_file(file_path: PathBuf) -> Result<(File, usize)> {
    let file_handle = OpenOptions::new().read(true).write(false).open(file_path)?;
    let file_size = file_handle.metadata()?.len() as usize;
    Ok((file_handle, file_size))
}

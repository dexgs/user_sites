use std::{
	io::{self, Read, Write, BufRead, BufReader},
	net::{SocketAddr, TcpStream},
	collections::HashMap,
	str
};
use urlencoding::decode;
// use super::os_windows;

/// The URL of a request, represented as a String after
/// decoding the percent-encoded path in a request header
pub type URL = String;

/// The headers of a request, represented as a mapping
/// of String keys to String values.
pub type Headers = HashMap<String, String>;

/// An HTTP request from a client. Currently, only
/// GET and POST are supported.
#[derive(Debug)]
pub enum Request {
	/// A GET request which has query data and headers
	GET(QueryData, Headers),
	/// A POST request which has headers and the data
	/// from its body
	POST(Headers, Option<FormData>)
}

/// The query data encoded in a request URL.
pub type QueryData = HashMap<String, String>;

/// The contents of the body of a form. Can be key-value data,
/// an arbitrary string, or a handle on the underlying TCP connection.
#[derive(Debug)]
pub enum FormData {
	/// Data is key-value pairs
	KeyVal(HashMap<String, String>),
	/// Data is plain text
	Text(String),
	/// Data is a stream of bytes
	Stream(BufReader<TcpStream>)
}

/// This struct represents a client which has connected to the µHTTP server.microhttp
///
/// If an instance of this struct is dropped, the connection is closed.
#[derive(Debug)]
pub struct Client {
	stream: TcpStream,
	addr: SocketAddr,
	request: Option<(URL, Request)>
}

fn read_request_type(reader: &mut BufReader<TcpStream>) -> io::Result<String> {
	let mut buffer = Vec::new();
	reader.read_until(b' ', &mut buffer)?;
	buffer.pop();
	Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn read_request_url(reader: &mut BufReader<TcpStream>) -> io::Result<(URL, QueryData)> {
	let mut buffer = Vec::new();
	reader.read_until(b' ', &mut buffer)?;
	buffer.pop();
	let url = String::from_utf8_lossy(&buffer).to_string();
	let (url, query) = match url.split_once('?') {
		Some((url_string, query_string)) => {
			(url_string, parse_url_encoded_key_value_pairs(query_string))
		},
		None => (url.as_str(), QueryData::new())
	};
	let url = decode(url)
		.map_err(|_| io::Error::from(io::ErrorKind::Other))?
		.to_string();
	Ok((URL::from(url), query))
}

fn parse_url_encoded_key_value_pairs(s: &str) -> HashMap<String, String> {
	s.trim().split('&').filter_map(
		|pair| {
			match pair.split_once('=') {
				Some((k, v)) => {
					let k = decode(k).ok()?.to_string();
					let v = decode(v).ok()?.to_string();
					Some((k, v))
				},
				None => Some((pair.to_string(), String::new()))
			}
		})
	.collect()
}

fn read_request_headers(reader: &mut BufReader<TcpStream>) -> io::Result<Headers> {
	let mut headers = Headers::new();
	// Initialize with non-empty contents so the loop runs at least once
	let mut buffer = String::from("_");
	while buffer.trim() != "" {
		buffer = String::new();
		reader.read_line(&mut buffer)?;
		if let Some((k, v)) = buffer.split_once(": ") {
			headers.insert(k.trim().to_lowercase(), v.trim().to_owned());
		}
	}
	Ok(headers)
}

fn read_form_content_to_string(mut reader: BufReader<TcpStream>, headers: &Headers) -> Option<String> {
	if let Some(length) = headers.get("content-length") {
		let length = length.parse().ok()?;
		reader.get_mut().set_nonblocking(true).ok()?;
		let mut buffer = vec![0; length];
		reader.read_exact(&mut buffer).ok()?;
		return Some(String::from_utf8(buffer).ok()?);
	}
	None
}

fn read_form_data(reader: BufReader<TcpStream>, headers: &Headers) -> io::Result<Option<FormData>> {
	match headers.get("content-type").map(|s| s.as_str()) {
		Some("text/plain") => {
			Ok(read_form_content_to_string(reader, headers).map(|text| FormData::Text(text)))
		},
		Some("application/x-www-form-urlencoded") => {
			Ok(read_form_content_to_string(reader, headers).map(|data| {
				FormData::KeyVal(parse_url_encoded_key_value_pairs(&data))
			}))
		},
		Some("multipart/form-data") => {
			Ok(Some(FormData::Stream(reader)))
		},
		_ => Ok(None)
	}
}

impl Client {
	pub(crate) fn new(stream: TcpStream, addr: SocketAddr) -> Result<Client,::std::io::Error> {
		let mut reader = BufReader::new(stream.try_clone()?);
		let request_type = read_request_type(&mut reader)?;
		let request = match request_type.as_str() {
			"GET" => {
				let (url, query) = read_request_url(&mut reader)?;
				let headers = read_request_headers(&mut reader)?;
				Some((url, Request::GET(query, headers)))
			},
			"POST" => {
				let (url, _) = read_request_url(&mut reader)?;
				let headers = read_request_headers(&mut reader)?;
				let data = read_form_data(reader, &headers)?;
				Some((url, Request::POST(headers, data)))
			},
			_ => None
		};
		Ok(Client {
			stream: stream,
			addr: addr,
			request: request
		})
	}

	/// Return the address of the requesting client, for example "1.2.3.4:9435".
	pub fn addr(&self) -> SocketAddr {
		self.addr
	}

	/// Return the request the client made or None if the client
	/// didn't make any or an invalid one.
	///
	/// **Note**: At the moment, only HTTP GET and POST are supported.
	/// Any other requests will not be collected.
	pub fn request(&self) -> &Option<(URL, Request)> {
		&self.request
	}

	/// Return a mutable reference to the request the client made
	/// or None if the client didn't make any or made an invalid
	/// one.
	///
	/// **Note**: At the moment, only HTTP GET and POST are supported.
	/// Any other requests will not be collected.
	pub fn request_mut(&mut self) -> &mut Option<(URL, Request)> {
		&mut self.request
	}

	/// Send a HTTP 200 OK response to the client + the provided data.
	/// The data may be an empty array, for example the following
	/// implementation echos all requests except "/hello":
	///
	/// Consider using ``respond_ok_chunked`` for sending file-backed data.
	///
	/// ```
	/// use micro_http_server::MicroHTTP;
	/// use std::io::*;
	/// let server = MicroHTTP::new("127.0.0.1:4000").expect("Could not create server.");
	/// # let mut connection = ::std::net::TcpStream::connect("127.0.0.1:4000").unwrap();
	/// # connection.write("GET /\r\n\r\n".as_bytes());
	/// let mut client = server.next_client().unwrap().unwrap();
	/// let request_str: String = client.request().as_ref().unwrap().clone();
	///
	/// match request_str.as_ref() {
	/// 	"/hello" => client.respond_ok(&[]),
	///     _ => client.respond_ok(request_str.as_bytes())  // Echo request
	/// };
	/// ```
	pub fn respond_ok(&mut self, data: &[u8]) -> io::Result<usize> {
		self.respond_ok_chunked(data, data.len())
	}

	// The test in this doc comment is no_run because it refers to an arbitrary
	// file that may not exist on the current system.

	/// Send a HTTP 200 OK response to the client + the provided data.
	/// The data may be any type implementing [Read](std::io::Read) and
	/// will be read in chunks. This is useful for serving file-backed
	/// data that should not be loaded into memory all at once.
	///
	/// ```no_run
	/// use micro_http_server::MicroHTTP;
	/// use std::io::*;
	/// use std::fs::*;
	/// let server = MicroHTTP::new("127.0.0.1:4000").expect("Could not create server.");
	/// # let mut connection = ::std::net::TcpStream::connect("127.0.0.1:4000").unwrap();
	/// # connection.write("GET /\r\n\r\n".as_bytes());
	/// let mut client = server.next_client().unwrap().unwrap();
	/// client.request();
	///
	/// let mut file_handle = OpenOptions::new()
	///		.read(true)
	///		.write(false)
	///		.open("/some/local/file")
	///		.unwrap();
	///	let file_len = file_handle.metadata().unwrap().len() as usize;
	///
	/// client.respond_ok_chunked(file_handle, file_len);
	///
	/// ```
	pub fn respond_ok_chunked(&mut self, data: impl Read, content_size: usize) -> io::Result<usize> {
		self.respond_chunked("200 OK", data, content_size, &vec!())
	}

	/// Send response data to the client.
	///
	/// This is similar to ``respond_ok``, but you may control the details yourself.
	///
	/// Consider using ``respond_chunked`` for sending file-backed data.
	///
	/// # Parameters
	/// * ``status_code``: Select the status code of the response, e.g. ``200 OK``.
	/// * ``data``: Data to transmit. May be empty.
	/// * ``headers``: Additional headers to add to the response. May be empty.
	///
	/// Calling ``respond("200 OK", data, &vec!())`` is the same as calling ``respond_ok(data)``.
	pub fn respond(
		&mut self,
		status_code: &str,
		data: &[u8],
		headers: &Vec<String>) -> io::Result<usize>
	{
		self.respond_chunked(status_code, data, data.len(), headers)
	}

	/// Send repsonse data to the client.
	/// 
	/// This is similar to ``respond_ok_chunked``, but you may control the details
	/// yourself.
	///
	/// # Parameters
	/// * ``status_code``: Select the status code of the response, e.ge ``200 OK``.
	/// * ``data``: Data to transmit. May be empty
	/// * ``content_size``: Size of the data to transmit in bytes
	/// * ``headers``: Additional headers to add to the response. May be empty.
	///
	/// Calling ``respond_chunked("200 OK", data, content_size, &vec!())`` is the same as calling
	/// ``repsond_ok_chunked(data, content_size)``.
	pub fn respond_chunked(
		&mut self,
		status_code: &str,
		mut data: impl Read,
		content_size: usize,
		headers: &Vec<String>) -> io::Result<usize> 
	{
		// Write status line
		let mut bytes_written =
			self.stream.write(format!("HTTP/1.0 {}\r\nContent-Length: {}\r\n", status_code, content_size).as_bytes())?;

		for h in headers {
			bytes_written += self.stream.write(format!("{}\r\n", h).as_ref())?;
		}
		bytes_written += self.stream.write("\r\n".as_bytes())?;

		let mut buffer = [0; Self::CHUNK_SIZE];
		loop {
			let bytes_read = data.read(&mut buffer)?;
			if bytes_read == 0 { break; }
			bytes_written += self.stream.write(&buffer[..bytes_read])?;
		}

		Ok(bytes_written)
	}

	const CHUNK_SIZE: usize = 4096;
}

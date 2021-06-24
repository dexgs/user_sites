# user sites

This is a simple server to serve webpages out of user home directories. It doesn't
do HTTPS, so you should probably put it behind a reverse proxy.

Currently, this project features no functionality beyond what is possible with any
popular web server (NGINX, Apache, etc.), but I intend to add more features in the
future.

## Planned Features
- Automatically create directory indices
- Simple server-side rendering
- Handle POST requests

## Usage
To run, pass in a single command line argument which is the port to which the server
will bind. The server will attempt to serve pages out of the ``www`` directory
in a user's home dir. For example, running the server on port 1234 would make
``/home/user/www/index.html`` accessible at ``http://localhost:1234/user/index.html``

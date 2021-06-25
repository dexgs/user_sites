# user sites

This is a simple server to serve webpages out of user home directories. It doesn't
do HTTPS, so you should probably put it behind a reverse proxy. The server requires
only read access to user home directories and works out-of-the-box with no configuration.
Options may be added in the future, but the goal is to make the intended functionality
available with minimal setup.

## Usage
To run, pass in a single command line argument which is the port to which the server
will bind. The server will attempt to serve pages out of the ``www`` directory
in a user's home dir. For example, running the server on port 1234 would make
``/home/user/www/index.html`` accessible at ``http://localhost:1234/user/index.html``

## Available Features

### Serve Static Files
The root of a user's website is ``~/www``. The contents of this directory and its
sub-directories will be exposed to the internet. Placing a file called ``index.html``
in a directory will serve that file when the directory is accessed. For example:
Accessing ``http://localhost:1234/user/my_page/`` will serve
``/home/user/www/my_page/index.html`` (if it exists).

### Server-Side Rendering
Placing an executable called ``index_executable`` into a directory will cause the
server to run that executable and relay its output over the web when that directory
is accessed. The command will be passed 2 arguments. The first will be the path
that was accessed and the second (currently unsupported, but coming soon) will
be the query string from the request.

### Auto-Indexed Directories
If a directory is accessed and it contains neither an ``index.html`` file nor an
``index_executable`` file, a directory index will automatically be generated and
served. The generated page can be customized by including the following files in
the directory:
- ``title``: The page's title will be set to the contents of this file
- ``header.html``: The contents of this file will be inserted at the top of the
    page's ``<body>`` tag.
- ``footer.html``: The contents of this file will be inserted at the bottom of
    the page's ``<body>`` tag.
- ``styles.css``: The page will try to apply this stylesheet.
By default, all of these files are hidden from the directory index via a CSS rule,
but this can be undone by overriding it in ``styles.css``.

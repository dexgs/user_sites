# user sites

This is a simple server to serve webpages out of user home directories. It doesn't
do HTTPS, so you should probably put it behind a reverse proxy. The server requires
only read access to user home directories and works out-of-the-box with no configuration.
Options may be added in the future, but the goal is to make the intended functionality
available with minimal setup.

## Usage
To run, pass in 1 command line argument: the port to which the
server will bind.

The server will attempt to serve pages out of the ``www`` directory in a user's
home dir. For example, running the server on port 1234 would make
``/home/user/www/index.html`` accessible at
``http://localhost:1234/user/index.html``.
**You should only run this software as an un-privileged user.**

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
is accessed. The command will be passed the path that was accessed as an argument.
The contents of the query string will be available to the program as environment
variables. There must be a file adjacent to ``index_executable`` called
``allowed_variables`` where each line contains the name of a variable. Any
keys in the query string which are not also present in ``allowed_variables``
will be discarded.

### Handle POST Requests
This is similar to the server-side rendering feature. Put an executable called
``form_executable`` at a location to handle POST requests. The program will be
passed the path to which the POST was made as an argument and will have access
to the form data. If the form was URL encoded, its values will be available as
environment variables. If the form was plaintext, it will be passed in as the
second argument. If the form was multipart, it will be sent to the program's
stdin. There must be a file adjacent to ``form_executable`` called
``allowed_variables`` as described in the previous paragraph.

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

All of these files are hidden from the generated directory index.

By default, the index will show the entire directory. However, if the query
parameters `p` and `n` are defined, the directory will be paginated and the
page with index `p` (starting at 1) will be shown where each page has `n` items
(excepting the last page, which may have less).

### Transclusion
If an HTML file contains the following pattern `{file-path}` where `file-path`
is a valid path (either absolute or relative to the HTML file), the contents
of that file will be inserted into the page where the pattern occurs.

(see [transclusion on Wikipedia](https://en.wikipedia.org/wiki/Help:Transclusion))

## Sample Nginx Configuration
This is how you can proxy this program running on port ``1234`` and with URL
prefix `"/users/"`:
```nginx
location /users/ {
    rewrite ^/users/(.*)$ /$1 break;
    proxy_pass http://127.0.0.1:1234;
}
```

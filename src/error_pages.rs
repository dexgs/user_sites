pub const ERROR_404: &'static str = format_html!("Nothing",
    "<h1>The page you are looking for does not exist.</h1>");

pub const ERROR_500: &'static str = format_html!("Error",
    "<h1>The file you requested exists, but could not be served to you due to some error.</h1>");

pub const ERROR_503: &'static str = format_html!("Server Busy",
    "<h1>Server too busy to serve response. Sorry.</h1>");

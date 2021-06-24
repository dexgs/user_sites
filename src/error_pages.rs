pub const ERROR_404: &'static str = "<!DOCTYPE html>
<html lang=\"en\">
    <head>
        <meta charset=\"UTF-8\">
        <title>Nobody</title>
    </head>
    <body>
        <h1>The person you are looking for does not exist.</h1>
    </body>
</html>";

pub const ERROR_500: &'static str = "<!DOCTYPE html>
<html lang=\"en\">
    <head>
        <meta charset=\"UTF-8\">
        <title>Error</title>
    </head>
    <body>
        <h1>The file you requested exists, but could not be served to you due to some error.</h1>
    </body>
</html>
";

macro_rules! head_begin {
    () => {
"<!DOCTYPE html>
<html lang=\"en\">
    <head>
        <meta charset=\"UTF-8\"/>
        <title>"};
}

macro_rules! head_end {
    () => {"</title>
    </head>
    <body>
"};
}

macro_rules! bottom {
    () => {"
    </body>
</html>"};
}

macro_rules! format_html {
    ($title:ident, $body:ident) => { format!("{}{}{}{}{}", head_begin!(), $title, head_end!(), $body, bottom!()) };
    ($title:tt, $body:tt) => { concat!(head_begin!(), $title, head_end!(), $body, bottom!()) };
}

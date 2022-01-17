macro_rules! head_begin {
    () => {
"<!DOCTYPE html>
<html lang=\"en\">
    <head>
        <meta charset=\"UTF-8\"/>
        "};
}

macro_rules! head_end {
    () => {"
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
    ($head:ident, $body:ident) => { format!("{}{}{}{}{}", head_begin!(), $head, head_end!(), $body, bottom!()) };
    ($head:tt, $body:tt) => { concat!(head_begin!(), $head, head_end!(), $body, bottom!()) };
}

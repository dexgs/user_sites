# syntax=docker/dockerfile:1
FROM alpine:3.14

RUN apk add cargo

WORKDIR /user_sites
COPY . .

RUN cargo build --release

USER nobody
CMD target/release/user_sites 1234

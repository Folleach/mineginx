FROM rust:1.93 as builder
WORKDIR /build
COPY . .
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo b -r --target=x86_64-unknown-linux-musl

FROM scratch
WORKDIR /app
COPY --from=builder ./build/target/x86_64-unknown-linux-musl/release/mineginx .
COPY --from=builder ./build/config/ ./config/
CMD ["./mineginx"]

FROM rust:1.94.0-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:trixie-slim
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /src/target/release/cpa-exporter /usr/local/bin/cpa-exporter
USER 65532:65532
EXPOSE 9321
ENTRYPOINT ["/usr/local/bin/cpa-exporter"]

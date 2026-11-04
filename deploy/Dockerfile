# Optional self-hosted server image. Ordinary local installs use native releases.
FROM rust:1.99.1-bookworm AS build
WORKDIR /build
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 right-wake \
    && useradd --uid 10001 --gid 10001 --create-home right-wake \
    && install -d -o 10001 -g 10001 -m 0700 /data
COPY --from=build /build/target/release/right-wake /usr/local/bin/right-wake
USER 10001:10001
ENV RIGHT_WAKE_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 7433
ENTRYPOINT ["/usr/local/bin/right-wake", "--data-dir", "/data"]
CMD ["serve", "--listen", "0.0.0.0:7433"]

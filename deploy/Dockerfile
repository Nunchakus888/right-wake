# Optional self-hosted server image. Ordinary local installs use native releases.
FROM rust:1.99.1-bookworm AS build
WORKDIR /build
COPY . .
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 waking \
    && useradd --uid 10001 --gid 10001 --create-home waking \
    && install -d -o 10001 -g 10001 -m 0700 /data
COPY --from=build /build/target/release/waking /usr/local/bin/waking
USER 10001:10001
ENV WAKING_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 7433
ENTRYPOINT ["/usr/local/bin/waking", "--data-dir", "/data"]
CMD ["serve", "--listen", "0.0.0.0:7433"]

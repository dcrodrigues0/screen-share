FROM rust:1.85-bookworm AS build

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/signaling-server /usr/local/bin/signaling-server

EXPOSE 8080
CMD ["signaling-server"]

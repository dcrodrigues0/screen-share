FROM rust:1.85

WORKDIR /usr/src/signaling
COPY . .

RUN cargo install --path .

EXPOSE 9001
CMD ["signaling-server"]
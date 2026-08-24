FROM rust:1.98-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p leave-relay

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 leave
COPY --from=build /src/target/release/leave-relay /usr/local/bin/leave-relay
USER leave
EXPOSE 8787
ENTRYPOINT ["leave-relay"]

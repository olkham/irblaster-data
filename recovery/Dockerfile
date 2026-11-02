# Builds the Linux version of IR Blaster Recovery without installing Rust (see docker-compose.yml).
# Build context: the repository root, because the app embeds firmware/boot/.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY firmware/boot firmware/boot
COPY recovery recovery
RUN cd recovery && cargo build --release --locked \
 && install -D target/release/irblaster-recovery /out/irblaster-recovery

FROM debian:bookworm-slim
COPY --from=build /out/irblaster-recovery /usr/local/bin/irblaster-recovery
ENTRYPOINT ["irblaster-recovery"]
CMD ["--help"]

# Ladderback's container image (technical design §12, decision 126): the
# `ladderback` binary of a Linux release archive, with the web UI built in,
# run as a machine's agent that backs up folders mounted into the container
# or, from M8, as the hub. It copies the archive's binary and never builds
# Rust, so the image and the archive are the same program.
#
# The build context is a folder holding each platform's unpacked release
# archive under Docker's name for its architecture: `amd64/ladderback` and
# `arm64/ladderback`, with the licences beside each. `just image` makes it
# from the archives and builds for their platforms, with the `VERSION` the
# binary says for the version label; `just smoke-image` tests the result.
# The install guide says how to run it (docs/guide/install.md).

# Debian 13 (trixie) slim, the build of 2026-11-04, by the digest of its
# multi-arch index, so every build starts from the same files: glibc 2.41,
# above the 2.35 the release binary needs (Alpine's musl can't run it). Its
# shell and apt let a hook be a shell script, and let an image built on this
# one add what its hooks or an rclone backend run; distroless has neither.
# To move it, take the digest from
# `docker buildx imagetools inspect debian:13-slim` and update the date.
FROM debian:13-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

ARG TARGETARCH
ARG VERSION

# The CA certificates are what the daemon needs beyond the base: its TLS is
# rustls, which trusts the system's roots, for an HTTPS backend and, from
# M8, a hub. Nothing else: jiff carries its own time zone database
# (`tzdb-bundle-always`), so `TZ` works whatever the base's tzdata holds, and
# the daemon runs no `rustic` CLI.
# The user has a fixed UID and GID, 10001, which no account on the host is
# likely to have, so files the container writes belong to nobody there. Its
# folders start as its own, mode 0700, and a new named volume mounted on one
# takes that owner and mode. Their parent, its home, is 0755 so that a
# container run as another `--user` reaches the host folders that user owns
# mounted there in place of the volumes.
RUN test -n "$VERSION" \
    && apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 ladderback \
    && useradd --system --uid 10001 --gid ladderback --home-dir /var/lib/ladderback \
        --no-create-home --shell /usr/sbin/nologin ladderback \
    && install -d -o ladderback -g ladderback -m 0755 /var/lib/ladderback \
    && install -d -o ladderback -g ladderback -m 0700 \
        /var/lib/ladderback/config /var/lib/ladderback/data /var/cache/ladderback

COPY ${TARGETARCH}/ladderback /usr/bin/ladderback
COPY ${TARGETARCH}/LICENSE-MIT ${TARGETARCH}/LICENSE-APACHE /usr/share/doc/ladderback/

# The configuration (config.toml) and the data folder (the database, the
# admin password's hash, the agent's ID) are what must outlive a container.
# The cache is only rustic's index cache, which a new one fetches again.
ENV LADDERBACK_CONFIG_DIR=/var/lib/ladderback/config \
    LADDERBACK_DATA_DIR=/var/lib/ladderback/data \
    LADDERBACK_CACHE_DIR=/var/cache/ladderback
VOLUME ["/var/lib/ladderback/config", "/var/lib/ladderback/data"]

USER 10001:10001
EXPOSE 9898

LABEL org.opencontainers.image.title="Ladderback" \
      org.opencontainers.image.description="A self-hosted backup manager for rustic: schedules, a web UI and notifications. Not yet a supported way to install and run Ladderback." \
      org.opencontainers.image.source="https://github.com/nledford/Ladderback" \
      org.opencontainers.image.documentation="https://github.com/nledford/Ladderback/blob/main/docs/guide/install.md#docker-image-not-supported-yet" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0" \
      org.opencontainers.image.version="${VERSION}"

# No HEALTHCHECK: the binary has no command that asks a running daemon how
# it is, and the image has no HTTP client to ask its routes.
ENTRYPOINT ["/usr/bin/ladderback"]
# Reached only through a published port, the daemon listens beyond the
# container's loopback, which it refuses until an `--allowed-host` names the
# host browsers reach it by (security §10.1). The image names none, so run
# with no arguments it stops and says so: the run command gives `serve` with
# that flag, as the install guide shows, and sign-in stays as the data
# folder says, on once `set-password` has kept a password there.
CMD ["serve", "--listen", "0.0.0.0:9898"]

FROM rust:1.93-alpine AS build

RUN apk add --no-cache \
      ca-certificates \
      tzdata \
      musl-dev

WORKDIR /usr/src/app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

ARG TARGETPLATFORM
ARG TARGETARCH
# название бинарника из cargo.toml
ARG BIN_NAME=b4n

# musl target под архитектуру для статической линковки
RUN case "${TARGETARCH}" in \
      amd64)  echo "x86_64-unknown-linux-musl" > /tmp/target ;; \
      arm64)  echo "aarch64-unknown-linux-musl" > /tmp/target ;; \
      *) echo "Unsupported TARGETARCH=${TARGETARCH}" && exit 1 ;; \
    esac

RUN rustup target add "$(cat /tmp/target)"

# билд и copy
RUN --mount=type=cache,id=cargo-registry-${TARGETPLATFORM},target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-target-${TARGETPLATFORM},target=/usr/src/app/target \
    set -eux; \
    cargo build --release --target "$(cat /tmp/target)" --bin "${BIN_NAME}"; \
    mkdir -p /out; \
    cp "/usr/src/app/target/$(cat /tmp/target)/release/${BIN_NAME}" /out/app; \
    cp /etc/ssl/certs/ca-certificates.crt /out/ca-certificates.crt; \
    mkdir -p /out/zoneinfo; \
    cp -a /usr/share/zoneinfo/. /out/zoneinfo/

###
FROM scratch as node
ENV TZ=Europe/Moscow

COPY --from=build /out/app /app
COPY --from=build /out/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /out/zoneinfo /usr/share/zoneinfo

EXPOSE 7001
ENTRYPOINT ["/app"]

###
FROM python:3.13-alpine AS web

ENV TZ=Europe/Moscow
ENV PYTHONUNBUFFERED=1

RUN apk add --no-cache \
      bash \
      ca-certificates \
      tzdata

WORKDIR /app

COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt

COPY --from=build /out/app /usr/local/bin/b4n
COPY --from=build /out/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=build /out/zoneinfo /usr/share/zoneinfo
COPY scripts/entrypoint.sh /entrypoint.sh
COPY webapp ./webapp

RUN chmod +x /entrypoint.sh

EXPOSE 7001 8080

ENTRYPOINT ["/entrypoint.sh"]

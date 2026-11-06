# VÖLUND container images (ADR 0039). Build context: repository root.
#
#   docker build -f deploy/docker/Dockerfile --target app -t volund:0.41.3 .
#   docker build -f deploy/docker/Dockerfile --target db  -t volund-db:17 .
#
# The app image keeps the native layout of deploy/debian/README.md: binaries in
# /usr/local/bin, static web build in /usr/share/volund/web, data below
# /srv/volund, service account `volund`.

ARG DEBIAN=trixie

# ---- Static browser application (Node is a build-only dependency)
FROM node:20-trixie AS web
WORKDIR /src/apps/volund-web
COPY apps/volund-web/package.json apps/volund-web/package-lock.json ./
RUN npm ci
# The web tests also check repository-wide contracts (license, versions).
COPY . /src/
RUN npm test && npm run build

# ---- Rust daemon and native OCCT converter
FROM debian:${DEBIAN} AS build
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates build-essential cmake ninja-build pkg-config git \
        rustc cargo \
        libocct-foundation-dev libocct-modeling-data-dev \
        libocct-modeling-algorithms-dev libocct-data-exchange-dev \
        libocct-ocaf-dev libocct-visualization-dev \
        libassimp-dev assimp-utils libtbb-dev libfontconfig1-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p volundd
RUN cmake -S native/cad-convert -B build/cad-convert -G Ninja -DCMAKE_BUILD_TYPE=Release \
    && cmake --build build/cad-convert --parallel "$(nproc)" \
    && ctest --test-dir build/cad-convert --output-on-failure
# Collect the Debian packages that provide every shared library both binaries
# link against, so the runtime image installs exactly those and nothing else.
RUN set -eu; mkdir /out; \
    install -m 0755 target/release/volundd build/cad-convert/volund-cad-convert /out/; \
    for lib in $(ldd /out/volundd /out/volund-cad-convert | awk '$2 == "=>" && $3 ~ /^\// { print $3 }' | sort -u); do \
        dpkg -S "$lib" 2>/dev/null || dpkg -S "/usr$lib" 2>/dev/null || dpkg -S "$(readlink -f "$lib")"; \
    done | cut -d: -f1 | sort -u > /out/runtime-packages; \
    cat /out/runtime-packages

# ---- Runtime image for the daemon and all workers
FROM debian:${DEBIAN}-slim AS app
ENV DEBIAN_FRONTEND=noninteractive
COPY --from=build /out/runtime-packages /tmp/runtime-packages
RUN apt-get update \
    && xargs -a /tmp/runtime-packages apt-get install -y --no-install-recommends \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* /tmp/runtime-packages
RUN groupadd --system --gid 10010 volund \
    && useradd --system --uid 10010 --gid volund --home-dir /srv/volund \
        --no-create-home --shell /usr/sbin/nologin volund
COPY --from=build /out/volundd /out/volund-cad-convert /usr/local/bin/
COPY --chmod=0755 deploy/docker/volund-loop /usr/local/bin/volund-loop
COPY --from=web /src/apps/volund-web/dist/ /usr/share/volund/web/
RUN chmod -R u=rwX,go=rX /usr/share/volund/web \
    && test -r /usr/share/volund/web/index.html \
    && test -r /usr/share/volund/web/LICENSE.txt \
    && /usr/local/bin/volund-cad-convert version
USER volund
WORKDIR /srv/volund
EXPOSE 8080
CMD ["volundd", "serve"]

# ---- PostgreSQL 17 with the `volund` OS account for peer authentication
FROM postgres:17-${DEBIAN} AS db
RUN groupadd --system --gid 10010 volund \
    && useradd --system --uid 10010 --gid volund --no-create-home \
        --shell /usr/sbin/nologin volund
COPY --chmod=0755 deploy/docker/postgres/10-volund.sh /docker-entrypoint-initdb.d/10-volund.sh
# The same image runs the validated dump job (deploy/backup/volund-backup), as
# it already carries the matching pg_dump/pg_restore/psql 17 client tools.
COPY --chmod=0755 deploy/backup/volund-backup /usr/local/sbin/volund-backup
COPY --chmod=0755 deploy/docker/volund-loop /usr/local/bin/volund-loop

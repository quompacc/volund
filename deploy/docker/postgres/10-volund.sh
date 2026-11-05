#!/bin/sh
# Runs once on an empty data directory (docker-entrypoint-initdb.d).
# Same role and database as deploy/debian/README.md: an unprivileged login role
# `volund` that owns a UTF-8 database `volund`. Authentication is local peer
# only; the cluster does not listen on TCP (see compose.yaml).
set -eu
psql -v ON_ERROR_STOP=1 --username postgres --dbname postgres <<'SQL'
CREATE ROLE volund LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE;
CREATE DATABASE volund OWNER volund ENCODING 'UTF8' TEMPLATE template0;
SQL

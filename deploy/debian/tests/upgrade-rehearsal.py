#!/usr/bin/env python3
"""Isolierte Upgrade-/Rückwegprobe von v0.38.3 auf den aktuellen Kandidaten."""

import getpass
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request


root = Path(sys.argv[1])
dump = Path(sys.argv[2])
assert root.is_absolute() and root.resolve() == root
assert root.parent == Path('/var/tmp') and re.fullmatch(r'volund-j2-[0-9]+', root.name)
assert dump.is_file() and dump.parent == root
assert os.geteuid() != 0
user = getpass.getuser()
assert re.fullmatch(r'[a-z][a-z0-9_]*', user)
pgbin = Path('/usr/lib/postgresql/17/bin')
work = root / 'upgrade-rehearsal'
work.mkdir(mode=0o700)
data = work / 'postgres'
sock = work / 'socket'
sock.mkdir(mode=0o700)
for name in ('derived', 'support'):
    (work / name).mkdir(mode=0o750)
for installed in [root / 'old/volundd', root / 'new/volundd',
                  *(root / 'old-web').rglob('*'), *(root / 'new-web').rglob('*')]:
    assert installed.stat().st_uid == 0, installed
    assert not os.access(installed, os.W_OK), installed


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def admin_sql(sql):
    run('psql', '-X', '-v', 'ON_ERROR_STOP=1', '-h', sock, '-p', '18495',
        '-U', 'j2admin', '-d', 'postgres', '-c', sql, stdout=subprocess.DEVNULL)


def app_sql(database, sql):
    return run('psql', '-X', '-v', 'ON_ERROR_STOP=1', '-h', sock, '-p', '18495',
               '-U', user, '-d', database, '-Atc', sql,
               capture_output=True, text=True).stdout.strip()


def restore(database):
    assert re.fullmatch(r'volund_j2_(baseline|failure|upgrade|rollback)', database)
    admin_sql(f'CREATE DATABASE {database} OWNER {user} TEMPLATE template0 ENCODING \'UTF8\'')
    run('pg_restore', '--exit-on-error', '--single-transaction', '--no-owner',
        '--no-privileges', '-h', sock, '-p', '18495', '-U', user, '-d', database, dump,
        stdout=subprocess.DEVNULL)


def environment(database, web, port):
    result = {key: value for key, value in os.environ.items() if not key.startswith('VOLUND_')}
    result.update({
        'VOLUND_DATABASE_URL': f'postgresql:///{database}?host={sock}&port=18495&user={user}',
        'VOLUND_LISTEN_ADDR': f'127.0.0.1:{port}',
        'VOLUND_WEB_ROOT': str(web),
        'VOLUND_DERIVED_ROOT': str(work / 'derived'),
        'VOLUND_SUPPORT_ROOT': str(work / 'support'),
    })
    return result


tables = (
    'authors', 'collection_models', 'collections', 'content_objects',
    'conversion_runs', 'derived_artifacts', 'library_roots', 'model_source_files',
    'model_tags', 'models', 'source_files', 'tags',
)


def fingerprint(database):
    values = {}
    for table in tables:
        projection = "to_jsonb(row_value) - 'revision'" if table == 'library_roots' else 'to_jsonb(row_value)'
        sql = (f"SELECT count(*) || ':' || md5(coalesce(string_agg(({projection})::text, "
               f"E'\\n' ORDER BY ({projection})::text), '')) FROM volund.{table} row_value")
        values[table] = app_sql(database, sql)
    return values


def assert_schema(database, migrations, tables_count):
    actual = app_sql(database, "SELECT "
                     "(SELECT count(*) FROM public._sqlx_migrations WHERE success) || ':' || "
                     "(SELECT count(*) FROM information_schema.tables "
                     "WHERE table_schema='volund' AND table_type='BASE TABLE')")
    assert actual == f'{migrations}:{tables_count}', actual


def http_probe(binary, database, web, port):
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', port))
    log = (work / f'daemon-{port}.log').open('w')
    daemon = subprocess.Popen([binary, 'serve'], env=environment(database, web, port),
                              cwd=work, stdout=log, stderr=subprocess.STDOUT)
    try:
        origin = f'http://127.0.0.1:{port}'
        for _attempt in range(100):
            assert daemon.poll() is None, f'Testdaemon {port} vorzeitig beendet'
            try:
                with urllib.request.urlopen(origin + '/api/v1/health', timeout=1) as response:
                    health = json.load(response)
                break
            except urllib.error.URLError:
                time.sleep(0.1)
        else:
            raise AssertionError(f'Testdaemon {port} startet nicht')
        assert health == {'status': 'ok', 'version': '0.39.0'}, health
        for route in ('/', '/models/upgrade-rehearsal'):
            with urllib.request.urlopen(origin + route, timeout=5) as response:
                assert response.read() == (web / 'index.html').read_bytes()
        for asset in (web / 'assets').iterdir():
            with urllib.request.urlopen(origin + '/assets/' + asset.name, timeout=5) as response:
                assert hashlib.sha256(response.read()).digest() == hashlib.sha256(asset.read_bytes()).digest()
    finally:
        if daemon.poll() is None:
            daemon.terminate()
        try:
            daemon.wait(timeout=15)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait(timeout=5)
        log.close()
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', port))


def assert_http_refused(binary, database, web, port):
    log_path = work / f'daemon-{port}-refused.log'
    with log_path.open('w') as log:
        daemon = subprocess.Popen([binary, 'serve'], env=environment(database, web, port),
                                  cwd=work, stdout=log, stderr=subprocess.STDOUT)
        try:
            result = daemon.wait(timeout=10)
        except subprocess.TimeoutExpired:
            daemon.terminate()
            daemon.wait(timeout=15)
            raise AssertionError('Alter Daemon akzeptiert unerwartet das neue Schema')
    assert result != 0
    assert '"code":"migration_required"' in log_path.read_text()
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', port))


run(pgbin / 'initdb', '-D', data, '-U', 'j2admin', '--encoding=UTF8', '--no-locale',
    '--data-checksums', '--auth-local=peer', '--auth-host=reject', stdout=subprocess.DEVNULL)
(data / 'pg_hba.conf').write_text('local all j2admin peer map=j2\nlocal all all peer\n')
(data / 'pg_ident.conf').write_text(f'j2 {user} j2admin\n')
pg_started = False
try:
    run(pgbin / 'pg_ctl', '-D', data, '-l', work / 'postgres.log',
        '-o', f"-k {sock} -p 18495 -c listen_addresses='' -c unix_socket_permissions=0700",
        '-w', 'start')
    pg_started = True
    admin_sql(f'CREATE ROLE {user} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE')

    restore('volund_j2_baseline')
    baseline = fingerprint('volund_j2_baseline')
    assert_schema('volund_j2_baseline', 30, 36)
    old = root / 'old/volundd'
    new = root / 'new/volundd'
    run(old, 'database-doctor', env=environment('volund_j2_baseline', root / 'old-web', 18095))
    print('PASS: tatsächlicher v0.38.3-Backupstand mit 30 Migrationen', flush=True)

    restore('volund_j2_failure')
    assert fingerprint('volund_j2_failure') == baseline
    app_sql('volund_j2_failure', 'CREATE TABLE volund.source_cleanup (forced_failure integer)')
    failed = subprocess.run([new, 'migrate'],
                            env=environment('volund_j2_failure', root / 'new-web', 18095),
                            capture_output=True, text=True)
    assert failed.returncode != 0, 'Erzwungener Migrationskonflikt muss fehlschlagen'
    assert_schema('volund_j2_failure', 30, 37)
    assert app_sql('volund_j2_failure', "SELECT count(*) FROM information_schema.columns "
                   "WHERE table_schema='volund' AND table_name='library_roots' "
                   "AND column_name='revision'") == '0'
    assert fingerprint('volund_j2_failure') == baseline
    app_sql('volund_j2_failure', 'DROP TABLE volund.source_cleanup')
    assert_schema('volund_j2_failure', 30, 36)
    run(old, 'database-doctor', env=environment('volund_j2_failure', root / 'old-web', 18095),
        stdout=subprocess.DEVNULL)
    print('PASS: Migrationsfehler ohne erfolgreiche Teilmigration oder Datenänderung', flush=True)

    restore('volund_j2_upgrade')
    assert fingerprint('volund_j2_upgrade') == baseline
    run(new, 'migrate', env=environment('volund_j2_upgrade', root / 'new-web', 18095))
    run(new, 'database-doctor', env=environment('volund_j2_upgrade', root / 'new-web', 18095))
    assert_schema('volund_j2_upgrade', 32, 37)
    assert app_sql('volund_j2_upgrade', 'SELECT count(*) FROM volund.source_cleanup') == '0'
    assert app_sql('volund_j2_upgrade', 'SELECT count(*) FROM volund.library_roots WHERE revision <> 1') == '0'
    assert fingerprint('volund_j2_upgrade') == baseline
    http_probe(new, 'volund_j2_upgrade', root / 'new-web', 18095)
    old_doctor = subprocess.run([old, 'database-doctor'],
                                env=environment('volund_j2_upgrade', root / 'old-web', 18096),
                                capture_output=True, text=True)
    assert old_doctor.returncode != 0, 'Alter Doctor muss zusätzlichen Schemaumfang erkennen'
    assert_http_refused(old, 'volund_j2_upgrade', root / 'old-web', 18096)
    assert fingerprint('volund_j2_upgrade') == baseline
    print('PASS: Upgrade; reiner Artefaktrückweg wird sicher verweigert', flush=True)

    restore('volund_j2_rollback')
    assert fingerprint('volund_j2_rollback') == baseline
    assert_schema('volund_j2_rollback', 30, 36)
    run(old, 'database-doctor', env=environment('volund_j2_rollback', root / 'old-web', 18097))
    admin_sql('ALTER DATABASE volund_j2_upgrade RENAME TO volund_j2_quarantine')
    admin_sql('ALTER DATABASE volund_j2_rollback RENAME TO volund')
    assert_schema('volund_j2_quarantine', 32, 37)
    assert fingerprint('volund_j2_quarantine') == baseline
    assert_schema('volund', 30, 36)
    run(old, 'database-doctor', env=environment('volund', root / 'old-web', 18097))
    http_probe(old, 'volund', root / 'old-web', 18097)
    assert fingerprint('volund') == baseline
    print('PASS: vollständiger Rückweg zum eingefrorenen Dump ohne Datenabweichung', flush=True)
finally:
    if pg_started:
        run(pgbin / 'pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop')
    assert not (data / 'postmaster.pid').exists()
    for port in (18095, 18096, 18097):
        with socket.socket() as probe:
            probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            probe.bind(('127.0.0.1', port))
    print('PASS: eigene Prozesse beendet und alle Testports freigegeben', flush=True)

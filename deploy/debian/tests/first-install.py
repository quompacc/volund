#!/usr/bin/env python3
"""Isolierte Erstinstallation: nur ein neu angelegtes volund-j1-Testverzeichnis.

Aufruf als unprivilegierter Testbenutzer: python3 first-install.py TESTVERZEICHNIS
Vorher root-eigene bin/volundd, bin/volund-cad-convert und web/ installieren.
Benötigt PostgreSQL 17 und dessen CLI-Werkzeuge; keinerlei systemd-Zugriff.
"""

import getpass
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import re
import secrets
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

root = Path(sys.argv[1])
assert root.is_absolute() and root.resolve() == root
assert root.parent == Path('/var/tmp') and re.fullmatch(r'volund-j1-[0-9]+', root.name)
assert os.geteuid() != 0
user = getpass.getuser()
assert re.fullmatch(r'[a-z][a-z0-9_]*', user)
port = 18094
with socket.socket() as probe:
    probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    probe.bind(('127.0.0.1', port))
pgbin = Path('/usr/lib/postgresql/17/bin')
work = root / 'first-install'
work.mkdir(mode=0o700)  # Existierende Testdaten niemals übernehmen oder löschen.
data = work / 'postgres'
sock = work / 'socket'
sock.mkdir(mode=0o700)
for name in ('library', 'incoming', 'derived', 'scratch', 'support'):
    (work / name).mkdir(mode=0o750)
for installed in [root / 'bin/volundd', root / 'bin/volund-cad-convert',
                  *(root / 'web').rglob('*')]:
    assert installed.stat().st_uid == 0, installed
    assert not os.access(installed, os.W_OK), installed
print('PASS: root-eigene, für den Dienstbenutzer unveränderbare Installation', flush=True)

subprocess.run([pgbin / 'initdb', '-D', data, '-U', 'j1admin', '--encoding=UTF8',
                '--no-locale', '--data-checksums', '--auth-local=peer',
                '--auth-host=reject'], check=True, stdout=subprocess.DEVNULL)
# Nur der Testbenutzer darf über den privaten Socket den Testcluster verwalten.
(data / 'pg_hba.conf').write_text('local all j1admin peer map=j1\nlocal all all peer\n')
(data / 'pg_ident.conf').write_text(f'j1 {user} j1admin\n')
pg_started = False
daemon = None
log = None
try:
    subprocess.run([pgbin / 'pg_ctl', '-D', data, '-l', work / 'postgres.log',
                    '-o', f"-k {sock} -p 18494 -c listen_addresses='' "
                    '-c unix_socket_permissions=0700', '-w', 'start'], check=True)
    pg_started = True
    pgargs = ['psql', '-X', '-v', 'ON_ERROR_STOP=1', '-h', str(sock), '-p', '18494']
    subprocess.run([*pgargs, '-U', 'j1admin', '-d', 'postgres', '-c',
                    f'CREATE ROLE {user} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE'], check=True)
    subprocess.run([*pgargs, '-U', 'j1admin', '-d', 'postgres', '-c',
                    f'CREATE DATABASE volund_j1_test OWNER {user} TEMPLATE template0 ENCODING \'UTF8\''],
                   check=True)
    query = subprocess.run([*pgargs, '-U', user, '-d', 'volund_j1_test', '-Atc',
                            "SELECT current_user, current_database(), "
                            "current_setting('data_checksums'), rolsuper "
                            "FROM pg_roles WHERE rolname=current_user"],
                           check=True, capture_output=True, text=True).stdout.strip()
    assert query == f'{user}|volund_j1_test|on|f', query
    print('PASS: leere UTF-8-Datenbank, Prüfsummen, Peer-Login ohne Superuser', flush=True)
    environment = dict(os.environ)
    # Keine möglicherweise geerbte produktive VÖLUND-Konfiguration verwenden.
    for key in list(environment):
        if key.startswith('VOLUND_'):
            del environment[key]
    token = secrets.token_hex(32)
    token_path = work / 'bootstrap.token'
    token_path.write_text(token)
    token_path.chmod(0o600)
    environment.update({
        'VOLUND_DATABASE_URL': f'postgresql:///volund_j1_test?host={sock}&port=18494&user={user}',
        'VOLUND_LISTEN_ADDR': f'127.0.0.1:{port}',
        'VOLUND_WEB_ROOT': str(root / 'web'),
        'VOLUND_DERIVED_ROOT': str(work / 'derived'),
        'VOLUND_SUPPORT_ROOT': str(work / 'support'),
        'VOLUND_BOOTSTRAP_TOKEN_FILE': str(token_path),
    })
    binary = root / 'bin/volundd'
    before = subprocess.run([binary, 'database-doctor'], env=environment, capture_output=True)
    assert before.returncode != 0, 'Leere Datenbank darf nicht bereit sein'
    for command in ('migrate', 'database-doctor', 'migrate', 'database-doctor'):
        subprocess.run([binary, command], env=environment, check=True)
    print('PASS: Erstmigration und wiederholte Migration mit database-doctor', flush=True)
    subprocess.run([root / 'bin/volund-cad-convert', 'version'], check=True)
    linked = subprocess.run(['ldd', root / 'bin/volund-cad-convert'],
                            capture_output=True, text=True, check=True).stdout
    assert 'not found' not in linked, linked
    log = (work / 'daemon.log').open('w')
    daemon = subprocess.Popen([binary, 'serve'], env=environment, cwd=work,
                              stdout=log, stderr=subprocess.STDOUT)
    origin = f'http://127.0.0.1:{port}'
    for attempt in range(100):
        assert daemon.poll() is None, 'Testdaemon vorzeitig beendet'
        try:
            with urllib.request.urlopen(origin + '/api/v1/health', timeout=1) as response:
                health = json.load(response)
            break
        except urllib.error.URLError:
            time.sleep(0.1)
    else:
        raise AssertionError('Testdaemon startet nicht innerhalb von zehn Sekunden')
    assert health == {'status': 'ok', 'version': '0.39.0'}, health
    for route in ('/', '/models/direct-install-check'):
        with urllib.request.urlopen(origin + route, timeout=5) as response:
            assert response.read() == (root / 'web/index.html').read_bytes()
    for asset in (root / 'web/assets').iterdir():
        with urllib.request.urlopen(origin + '/assets/' + asset.name, timeout=5) as response:
            assert hashlib.sha256(response.read()).digest() == hashlib.sha256(asset.read_bytes()).digest()
    with urllib.request.urlopen(origin + '/api/v1/setup', timeout=5) as response:
        assert json.load(response) == {'initialized': False, 'bootstrapAvailable': True}
    password = secrets.token_urlsafe(32)
    owner = {'email': 'j1@example.invalid', 'displayName': 'J1 Test', 'password': password}
    request = urllib.request.Request(origin + '/api/v1/setup/owner',
                                     data=json.dumps(owner).encode(),
                                     headers={'Content-Type': 'application/json', 'Origin': origin,
                                              'x-volund-setup-token': token})
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.status in (200, 201)
    token_path.unlink()
    with urllib.request.urlopen(origin + '/api/v1/setup', timeout=5) as response:
        assert json.load(response) == {'initialized': True, 'bootstrapAvailable': False}
    jar = http.cookiejar.CookieJar()
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    request = urllib.request.Request(origin + '/api/v1/sessions',
                                     data=json.dumps({'email': owner['email'], 'password': password}).encode(),
                                     headers={'Content-Type': 'application/json', 'Origin': origin})
    with opener.open(request, timeout=10) as response:
        assert response.status == 200
    assert list(jar), 'Login muss Sitzungscookie setzen'
    with opener.open(origin + '/api/v1/models', timeout=5) as response:
        catalog = json.load(response)
        assert catalog == [], catalog
    print('PASS: Health, SPA-Direktpfad, alle Assetbytes, Bootstrap, Login, Leerkatalog', flush=True)
finally:
    if daemon is not None:
        if daemon.poll() is None:
            daemon.terminate()
        try:
            daemon.wait(timeout=15)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait(timeout=5)
    if log is not None:
        log.close()
    if pg_started:
        subprocess.run([pgbin / 'pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop'], check=True)
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', port))
    assert not (data / 'postmaster.pid').exists()
    print('PASS: eigene Prozesse beendet und Testport freigegeben', flush=True)

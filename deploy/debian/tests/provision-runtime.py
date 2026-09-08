#!/usr/bin/env python3
"""Laufzeitprüfung der tatsächlich per Runbook provisionierten Erstinstallation."""
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import time
import urllib.error
import urllib.request

assert os.geteuid() != 0
assert Path('/etc/volund-j1-probe').is_file()
assert subprocess.check_output(['id', '-un'], text=True).strip() == 'volund'
query = subprocess.check_output(['psql', '-XAt', '-v', 'ON_ERROR_STOP=1', '-d', 'volund', '-c',
    "SELECT current_user,current_database(),current_setting('data_checksums'),"
    "rolsuper,rolcreatedb,rolcreaterole FROM pg_roles WHERE rolname=current_user"], text=True).strip()
assert query == 'volund|volund|on|f|f|f', query
web = Path('/usr/share/volund/web')
for file in [Path('/usr/local/bin/volundd'), Path('/usr/local/bin/volund-cad-convert'), *web.rglob('*')]:
    assert file.stat().st_uid == 0 and not os.access(file, os.W_OK), file
subprocess.run(['/usr/local/bin/volund-cad-convert', 'version'], check=True)
assert 'not found' not in subprocess.check_output(['ldd', '/usr/local/bin/volund-cad-convert'], text=True)
environment = {key: value for key, value in os.environ.items() if not key.startswith('VOLUND_')}
environment.update(VOLUND_LISTEN_ADDR='127.0.0.1:18099', VOLUND_WEB_ROOT=str(web),
    VOLUND_BOOTSTRAP_TOKEN_FILE='/etc/volund/bootstrap.token',
    VOLUND_DERIVED_ROOT='/srv/volund/derived', VOLUND_SUPPORT_ROOT='/srv/volund/support')
origin = 'http://127.0.0.1:18099'
with socket.socket() as probe:
    probe.bind(('127.0.0.1', 18099))
with Path('/srv/volund/scratch/daemon.log').open('w') as log:
    daemon = subprocess.Popen(['/usr/local/bin/volundd', 'serve'], env=environment,
                              stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(100):
            assert daemon.poll() is None, 'Testdaemon vorzeitig beendet'
            try:
                with urllib.request.urlopen(origin + '/api/v1/health', timeout=1) as response:
                    assert json.load(response) == {'status': 'ok', 'version': '0.39.0'}
                break
            except urllib.error.URLError:
                time.sleep(0.1)
        else:
            raise AssertionError('Startzeit überschritten')
        for route in ('/', '/models/direct-provision-check'):
            with urllib.request.urlopen(origin + route, timeout=5) as response:
                assert response.read() == (web / 'index.html').read_bytes()
        for asset in (web / 'assets').iterdir():
            with urllib.request.urlopen(origin + '/assets/' + asset.name, timeout=5) as response:
                assert hashlib.sha256(response.read()).digest() == hashlib.sha256(asset.read_bytes()).digest()
        with urllib.request.urlopen(origin + '/api/v1/setup', timeout=5) as response:
            assert json.load(response) == {'initialized': False, 'bootstrapAvailable': True}
        owner = {'email': 'provision@example.invalid', 'displayName': 'J1 Probe',
                 'password': secrets.token_urlsafe(32)}
        token = Path('/etc/volund/bootstrap.token').read_text().strip()
        request = urllib.request.Request(origin + '/api/v1/setup/owner',
            data=json.dumps(owner).encode(), headers={'Content-Type': 'application/json',
            'Origin': origin, 'x-volund-setup-token': token})
        with urllib.request.urlopen(request, timeout=10) as response:
            assert response.status in (200, 201)
        jar = http.cookiejar.CookieJar()
        opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
        request = urllib.request.Request(origin + '/api/v1/sessions',
            data=json.dumps({'email': owner['email'], 'password': owner['password']}).encode(),
            headers={'Content-Type': 'application/json', 'Origin': origin})
        with opener.open(request, timeout=10) as response:
            assert response.status == 200
        assert list(jar)
        with opener.open(origin + '/api/v1/models', timeout=5) as response:
            assert json.load(response) == []
        print('PASS: Peer-Rolle, Assets, SPA, Health, Bootstrap, Login und Leerkatalog', flush=True)
    finally:
        daemon.terminate()
        try:
            daemon.wait(timeout=15)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait(timeout=5)
with socket.socket() as probe:
    probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    probe.bind(('127.0.0.1', 18099))
print('PASS: eigener Testdaemon beendet, Port 18099 frei', flush=True)

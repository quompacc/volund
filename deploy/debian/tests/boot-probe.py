#!/usr/bin/env python3
"""J.2–J.4 boot, proxy, upgrade, and rollback probe in a booted Debian 13 container.

Usage, as root on a disposable probe host with debootstrap, systemd-nspawn, and
systemd-machined: python3 boot-probe.py /var/tmp/volund-j3-<digits>

The directory must contain old-source.tar (git archive of the released tag),
source.tar (the candidate), and old-web.tar/web.tar (each built dist/). The probe
creates rootfs/ there, boots it as machine volund-j3 with a private veth link,
and touches nothing else on the host. See deploy/debian/README.md.
"""
import http.client
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import tarfile
import time

MACHINE = 'volund-j3'
HOST_ADDRESS = '10.231.0.1'
GUEST_ADDRESS = '10.231.0.2'
HOSTNAME = 'volund.j3.test'
ORIGIN = f'https://{HOSTNAME}'
GUEST_SCRIPT = '/opt/new/source/deploy/debian/tests/boot-guest.py'
OWNER = {'email': 'boot-probe@example.invalid', 'displayName': 'J3 Probe',
         'password': secrets.token_urlsafe(32)}

base = Path(sys.argv[1])
assert os.geteuid() == 0
assert base.is_absolute() and base.resolve() == base
assert base.parent == Path('/var/tmp') and re.fullmatch(r'volund-j3-[0-9]+', base.name)
rootfs = base / 'rootfs'
assert not rootfs.exists(), 'a probe rootfs is never reused'
for name in ('old-source.tar', 'source.tar', 'old-web.tar', 'web.tar'):
    assert (base / name).is_file(), name
for tool in ('debootstrap', 'systemd-nspawn', 'machinectl', 'systemd-run'):
    assert shutil.which(tool), tool
assert subprocess.run(['machinectl', 'show', MACHINE], capture_output=True).returncode != 0


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def prepare():
    run('debootstrap', '--variant=minbase',
        '--include=systemd,systemd-sysv,dbus,sudo,python3,iproute2,procps,ca-certificates',
        'trixie', rootfs, 'https://deb.debian.org/debian', stdout=subprocess.DEVNULL)
    (rootfs / 'etc/volund-j3-probe').write_text('Isolierte J.2-J.4-Boot- und Proxyprobe\n')
    policy = rootfs / 'usr/sbin/policy-rc.d'
    policy.write_text('#!/bin/sh\nexit 101\n')
    policy.chmod(0o755)
    (rootfs / 'etc/hostname').write_text(MACHINE + '\n')
    for release, archive, web in (('old', 'old-source.tar', 'old-web.tar'),
                                  ('new', 'source.tar', 'web.tar')):
        for target, tar in (('source', archive), ('web', web)):
            destination = rootfs / 'opt' / release / target
            destination.mkdir(parents=True)
            with tarfile.open(base / tar) as bundle:
                bundle.extractall(destination, filter='data')
    (rootfs / 'opt/cargo').mkdir()


def offline(*arguments):
    """Run a guest step in the not-booted rootfs, sharing the host network."""
    run('systemd-nspawn', '--quiet', '--register=no', '--pipe', '-D', rootfs,
        '/usr/bin/python3', GUEST_SCRIPT, *arguments)


class Container:
    def __init__(self):
        self.process = subprocess.Popen(
            ['systemd-nspawn', '--quiet', '--boot', f'--machine={MACHINE}',
             '--network-veth', '-D', str(rootfs)],
            stdout=(base / 'boot.log').open('ab'), stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 120
        while subprocess.run(['systemd-run', f'--machine={MACHINE}', '--wait', '--pipe',
                              '--quiet', '/bin/true'], capture_output=True).returncode != 0:
            assert self.process.poll() is None, 'container exited during boot'
            assert time.monotonic() < deadline, 'container did not boot'
            time.sleep(1)
        link = f've-{MACHINE}'
        run('ip', 'addr', 'replace', f'{HOST_ADDRESS}/30', 'dev', link)
        run('ip', 'link', 'set', link, 'up')

    def guest(self, *arguments):
        run('systemd-run', f'--machine={MACHINE}', '--wait', '--pipe', '--quiet',
            '/usr/bin/python3', GUEST_SCRIPT, *arguments)

    def poweroff(self):
        run('machinectl', 'poweroff', MACHINE)
        self.process.wait(timeout=120)


class Client:
    """HTTPS through the guest's Caddy, verified against its internal CA."""

    def __init__(self):
        ca = rootfs / 'var/lib/caddy/.local/share/caddy/pki/authorities/local/root.crt'
        deadline = time.monotonic() + 60
        while not ca.is_file():
            assert time.monotonic() < deadline, 'Caddy did not create its CA'
            time.sleep(1)
        self.context = ssl.create_default_context(cafile=str(ca))
        self.cookies = {}

    def request(self, method, path, body=None, headers=None, expect=(200,)):
        deadline = time.monotonic() + 30
        while True:
            try:
                raw = socket.create_connection((GUEST_ADDRESS, 443), timeout=15)
                break
            except OSError:
                assert time.monotonic() < deadline, 'proxy unreachable'
                time.sleep(1)
        connection = http.client.HTTPSConnection(HOSTNAME, 443, context=self.context, timeout=30)
        connection.sock = self.context.wrap_socket(raw, server_hostname=HOSTNAME)
        all_headers = {'Accept': 'application/json', 'Origin': ORIGIN, **(headers or {})}
        if self.cookies:
            all_headers['Cookie'] = '; '.join(f'{k}={v}' for k, v in self.cookies.items())
        csrf = self.cookies.get('__Host-volund_csrf')
        if csrf and method != 'GET':
            all_headers['X-CSRF-Token'] = csrf
        payload = None
        if body is not None:
            payload = json.dumps(body).encode()
            all_headers['Content-Type'] = 'application/json'
        connection.request(method, path, body=payload, headers=all_headers)
        response = connection.getresponse()
        data = response.read()
        for cookie in response.headers.get_all('Set-Cookie') or []:
            name, _, value = cookie.split(';', 1)[0].partition('=')
            assert 'secure' in cookie.lower(), cookie
            self.cookies[name.strip()] = value.strip()
        assert response.status in expect, (method, path, response.status, data[:300])
        connection.close()
        kind = response.headers.get('Content-Type', '')
        return json.loads(data) if kind.startswith('application/json') and data else data

    def login(self):
        self.cookies = {}
        self.request('POST', '/api/v1/sessions',
                     {'email': OWNER['email'], 'password': OWNER['password']})
        assert '__Host-volund_session' in self.cookies, self.cookies


def items(document):
    if isinstance(document, list):
        return document
    for key in ('items', 'files', 'previews'):
        if key in document:
            return document[key]
    raise AssertionError(document)


def wait_for(condition, what, seconds=180):
    deadline = time.monotonic() + seconds
    while True:
        value = condition()
        if value:
            return value
        assert time.monotonic() < deadline, f'timed out waiting for {what}'
        time.sleep(2)


def direct_access_refused():
    try:
        socket.create_connection((GUEST_ADDRESS, 8080), timeout=5).close()
    except OSError:
        return
    raise AssertionError('the LAN reached volundd directly on port 8080')


def snapshot(client):
    files = items(client.request('GET', '/api/v1/roots/probe/files'))
    previews = {}
    for entry in files:
        listed = items(client.request('GET', f"/api/v1/files/{entry['id']}/previews?limit=50"))
        previews[entry['id']] = sorted((p['id'], p['status']) for p in listed)
    thumbnail = None
    ready = [pid for values in previews.values() for pid, status in values if status == 'ready']
    if ready:
        thumbnail = client.request('GET', f'/api/v1/previews/{sorted(ready)[0]}/artifacts/thumbnail-raster')
        assert thumbnail[:8] == b'\x89PNG\r\n\x1a\n'
    return sorted((f['id'], f.get('relativePath')) for f in files), previews, thumbnail


def first_boot(client):
    assert client.request('GET', '/api/v1/setup') == {'initialized': False, 'bootstrapAvailable': True}
    token = (rootfs / 'etc/volund/bootstrap.token').read_text().strip()
    client.request('POST', '/api/v1/setup/owner', OWNER, {'X-Volund-Setup-Token': token},
                   expect=(200, 201))
    client.login()
    client.request('POST', '/api/v1/libraries', {
        'key': 'probe', 'name': 'Probe', 'path': '/srv/volund/library/probe',
        'confirmation': 'ADD LIBRARY probe'}, expect=(200, 201))
    client.request('POST', '/api/v1/libraries/probe/scans', {'full': False}, expect=(200, 201, 202))
    files = wait_for(lambda: items(client.request('GET', '/api/v1/roots/probe/files')), 'scan')
    file_id = files[0]['id']
    preview = client.request('POST', f'/api/v1/files/{file_id}/previews', {'profile': 'web'},
                             expect=(200, 201, 202))
    wait_for(lambda: any(p['id'] == preview['id'] and p['status'] == 'ready' for p in
                         items(client.request('GET', f'/api/v1/files/{file_id}/previews?limit=50'))),
             'real OCCT preview')
    return file_id


def interrupted_conversion(container, client, file_id):
    container.guest('slow-worker')
    preview = client.request('POST', f'/api/v1/files/{file_id}/previews', {'profile': 'fine'},
                             expect=(200, 201, 202))
    container.guest('stop-worker', preview['id'])
    wait_for(lambda: client.request('GET', f"/api/v1/jobs/conversion/{preview['id']}")['status']
             == 'ready', 'requeued conversion')
    assert client.request('GET', f"/api/v1/jobs/conversion/{preview['id']}")['attempt'] == 2


prepare()
offline('packages')
offline('build', 'old')
offline('build', 'new')
offline('install', 'old')
print('PASS: Rootfs vorbereitet, beide Stände gebaut, v-alt installiert', flush=True)

container = Container()
container.guest('verify-boot', 'old')
client = Client()
assert client.request('GET', '/api/v1/health')['status'] == 'ok'
direct_access_refused()
file_id = first_boot(client)
before = snapshot(client)
container.poweroff()
print('PASS: Erstboot über TLS-Proxy: Owner, Login, Scan, echte Vorschau; Port 8080 gesperrt', flush=True)

container = Container()
container.guest('verify-boot', 'old')
client.login()
assert snapshot(client) == before, 'catalog changed across reboot'
direct_access_refused()
container.poweroff()
print('PASS: Neustart: Dienste, Firewall-Reihenfolge und Katalog unverändert', flush=True)

offline('switch', 'new')
container = Container()
container.guest('verify-boot', 'new')
client.login()
assert snapshot(client) == before, 'catalog changed across upgrade'
interrupted_conversion(container, client, file_id)
upgraded = snapshot(client)
container.poweroff()
print('PASS: Upgrade auf den Kandidaten; systemd-Stopp mitten in der Konvertierung erholt', flush=True)

offline('switch', 'old')
container = Container()
container.guest('verify-boot', 'old')
client.login()
# With a newer schema the rollback restores the frozen pre-upgrade dump, which
# deliberately discards the writes made by the candidate.
full = (rootfs / 'var/backups/volund-j3/full-rollback').is_file()
assert snapshot(client) == (before if full else upgraded), 'catalog changed across rollback'
direct_access_refused()
container.poweroff()
print('PASS: Rollback auf den Vorgängerstand ' + ('(vollständig per Dump)' if full else '(nur Binärdateien)')
      + '; Katalog wie erwartet', flush=True)
print(f'PASS: J.2-J.4-Probe vollständig; Probenbaum {base} kann entfernt werden', flush=True)

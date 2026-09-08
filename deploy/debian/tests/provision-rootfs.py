#!/usr/bin/env python3
"""Führt ausgewählte echte Runbookblöcke ausschließlich im markierten Rootfs aus."""
import os
from pathlib import Path
import re
import subprocess
import sys

assert os.geteuid() == 0
assert Path('/etc/volund-j1-probe').read_text() == 'Isolierte J.1-Provisionierungsprobe\n'
source = Path('/opt/source')
blocks = re.findall(r'```sh\n(.*?)```', (source / 'deploy/debian/README.md').read_text(), re.S)
assert len(blocks) >= 10
env = dict(os.environ, DEBIAN_FRONTEND='noninteractive', CARGO_HOME='/opt/cargo',
           CARGO_BUILD_JOBS='2', CARGO_NET_OFFLINE='true')
for key in list(env):
    if key.startswith('VOLUND_'):
        del env[key]
mode = sys.argv[1]
if mode == 'packages':
    assert not Path('/etc/postgresql').exists()
    assert subprocess.run(['getent', 'passwd', 'volund'], capture_output=True).returncode != 0
    for block in blocks[:3]:
        subprocess.run(['sh', '-eu', '-c', block], cwd=source, env=env, check=True,
                       input='Y\n' * 20, text=True)
    clusters = subprocess.check_output(['pg_lsclusters', '--no-header'], text=True)
    assert clusters.strip() == '', clusters
    print('PASS: originale Paketbefehle, kein automatisch angelegter PostgreSQL-Cluster', flush=True)
elif mode == 'build':
    assert blocks[4].startswith('cargo build --locked --release -p volundd')
    subprocess.run(['sh', '-eu', '-c', blocks[4]], cwd=source, env=env, check=True)
    # Web wurde auf dem getrennten Node-Buildhost gebaut; nur statische Assets übernehmen.
    web_install = blocks[5][blocks[5].index('sudo install -d'):]
    (source / 'apps/volund-web/dist').symlink_to('/opt/web', target_is_directory=True)
    # Der Benutzer entsteht im folgenden originalen Kontoblock, daher Lesetest erst danach.
    web_install = web_install.replace('sudo -u volund test -r /usr/share/volund/web/index.html', '')
    subprocess.run(['sh', '-eu', '-c', web_install], cwd=source, env=env, check=True)
    print('PASS: frische Rust-/C++-Builds, vier CTests und root-eigene Installation', flush=True)
elif mode == 'install':
    interfaces = {line.split(':')[0].strip() for line in Path('/proc/net/dev').read_text().splitlines() if ':' in line}
    assert interfaces == {'lo'}, 'Netzwerk muss isoliert sein'
    subprocess.run(['ip', 'link', 'set', 'dev', 'lo', 'up'], check=True)
    assert not Path('/srv/volund').exists()
    try:
        assert blocks[3].startswith('sudo adduser --system --group')
        subprocess.run(['sh', '-eu', '-c', blocks[3]], cwd=source, env=env, check=True)
        subprocess.run(['sudo', '-u', 'volund', 'test', '-r', '/usr/share/volund/web/index.html'], check=True)
        assert 'nologin' in subprocess.check_output(['getent', 'passwd', 'volund'], text=True)
        assert blocks[8].startswith('sudo install -d -o root -g volund')
        subprocess.run(['sh', '-eu', '-c', blocks[8]], cwd=source, env=env, check=True)
        assert blocks[9].startswith('sudo -u volund /usr/local/bin/volundd migrate')
        for _ in range(2):
            subprocess.run(['sh', '-eu', '-c', blocks[9]], cwd=source, env=env, check=True)
        subprocess.run(['sudo', '-u', 'volund', 'python3', '/opt/provision-runtime.py'],
                       env=env, check=True)
    finally:
        if Path('/etc/postgresql/17/volund/postgresql.conf').exists():
            subprocess.run(['pg_ctlcluster', '17', 'volund', 'stop'], check=True)
    assert not Path('/srv/volund/postgres/17/volund/postmaster.pid').exists()
    print('PASS: originale Konto-/Cluster-/Bootstrap-/Migrationsbefehle und Clusterbereinigung', flush=True)
else:
    raise AssertionError('Unbekannter Prüfmodus')

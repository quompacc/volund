#!/usr/bin/env python3
"""Guest side of the J.2–J.4 boot probe; runs only inside the marked rootfs.

Modes (called by boot-probe.py):
  packages              original package blocks plus Caddy and nftables
  build <old|new>       locked release build of one source tree
  install <old|new>     first installation from the runbook (not booted)
  switch <old|new>      replace binaries, web tree, and units (not booted)
  verify-boot <old|new> inside the booted system: units, order, health
  slow-worker           inside the booted system: pause the worker, slow converter
  stop-worker <run-id>  inside the booted system: systemd stop mid-conversion
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import urllib.request

MARKER = Path('/etc/volund-j3-probe')
assert os.geteuid() == 0
assert MARKER.read_text() == 'Isolierte J.2-J.4-Boot- und Proxyprobe\n'
PROXY_ADDRESS = '10.231.0.2'
HOSTNAME = 'volund.j3.test'
ENV = {key: value for key, value in os.environ.items() if not key.startswith('VOLUND_')}
ENV.update(DEBIAN_FRONTEND='noninteractive', CARGO_HOME='/opt/cargo', CARGO_BUILD_JOBS='8')
TIMERS = ['volund-preview-worker', 'volund-scan-worker', 'volund-import-cleanup',
          'volund-scheduler', 'volund-retention']
SLOW_CONVERTER = Path('/usr/local/libexec/volund-j3-slow-convert')
SLOW_DROPIN = Path('/etc/systemd/system/volund-preview-worker.service.d/j3-slow.conf')


def source(release):
    assert release in ('old', 'new')
    return Path('/opt') / release / 'source'


def blocks(release):
    text = (source(release) / 'deploy/debian/README.md').read_text()
    return re.findall(r'```sh\n(.*?)```', text, re.S)


def block(release, prefix):
    found = [item for item in blocks(release) if item.startswith(prefix)]
    assert len(found) == 1, (release, prefix, len(found))
    return found[0]


def offline(script):
    """Adapt a runbook block for a rootfs whose systemd is not running yet."""
    lines = []
    for line in script.replace('\\\n', ' ').splitlines():
        if 'daemon-reload' in line or line.startswith('wget '):
            continue
        lines.append(line.replace('systemctl enable --now', 'systemctl enable'))
    return '\n'.join(lines) + '\n'


def sh(script, cwd='/'):
    # Runbook apt blocks ask for confirmation, as an operator would see them.
    subprocess.run(['sh', '-eu', '-c', script], cwd=cwd, env=ENV, check=True,
                   input='Y\n' * 20, text=True)


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, env=ENV, **kwargs)


def packages():
    assert not Path('/etc/postgresql').exists()
    for prefix in ('sudo apt-get update', "printf '\\ncreate_main_cluster", 'sudo apt-get install'):
        sh(block('new', prefix))
    sh('apt-get install -y -q caddy nftables curl')
    clusters = subprocess.check_output(['pg_lsclusters', '--no-header'], text=True)
    assert clusters.strip() == '', clusters
    print('PASS: Runbook-Pakete, Caddy und nftables installiert, kein Auto-Cluster', flush=True)


def build(release):
    tree = source(release)
    sh('cargo build --locked --release -p volundd', cwd=tree)
    build_dir = tree / 'build/cad-convert'
    sh(f'cmake -S native/cad-convert -B {build_dir} -G Ninja -DCMAKE_BUILD_TYPE=Release'
       f' && cmake --build {build_dir} --parallel 8'
       f' && ctest --test-dir {build_dir} --output-on-failure', cwd=tree)
    out = Path('/opt') / release / 'release'
    out.mkdir()
    shutil.copy2(tree / 'target/release/volundd', out / 'volundd')
    shutil.copy2(build_dir / 'volund-cad-convert', out / 'volund-cad-convert')
    shutil.copy2(build_dir / 'volund-cad-fixture', out / 'volund-cad-fixture')
    print(f'PASS: {release}-Stand gebaut, CTests grün', flush=True)


def install_release(release):
    out = Path('/opt') / release / 'release'
    for name in ('volundd', 'volund-cad-convert'):
        run('install', '-o', 'root', '-g', 'root', '-m', '0755', out / name, f'/usr/local/bin/{name}')
    web = Path('/usr/share/volund/web')
    if web.exists():
        shutil.rmtree(web)
    sh(f'install -d -o root -g root -m 0755 {web} && cp -R /opt/{release}/web/. {web}/'
       f' && chown -R root:root {web}'
       f' && find {web} -type d -exec chmod 0755 {{}} + && find {web} -type f -exec chmod 0644 {{}} +')
    unit_block = block(release, 'sudo install -m 0644 deploy/systemd/volund-preview-worker')
    sh(offline(unit_block), cwd=source(release))
    sh(offline(block(release, 'sudo install -d -o volund -g volund -m 0750 /srv/volund/support')),
       cwd=source(release))


def proxy_configuration(release):
    tree = source(release)
    nft = (tree / 'deploy/nftables/volund-proxy-firewall.nft.example').read_text()
    assert '192.0.2.10' in nft
    Path('/etc/volund/volund-proxy-firewall.nft').write_text(nft.replace('192.0.2.10', PROXY_ADDRESS))
    run('install', '-m', '0644', tree / 'deploy/systemd/volund-proxy-firewall.service.example',
        '/etc/systemd/system/volund-proxy-firewall.service')
    dropin = Path('/etc/systemd/system/volundd.service.d')
    dropin.mkdir(exist_ok=True)
    run('install', '-m', '0644', tree / 'deploy/systemd/volundd-proxy.conf.example', dropin / 'proxy.conf')
    run('systemctl', 'enable', 'volund-proxy-firewall.service')


def install(release):
    assert not Path('/srv/volund').exists()
    sh(block(release, 'sudo adduser --system --group'))
    install_release(release)
    sh(block(release, 'sudo install -d -o root -g volund -m 0750 /etc/volund'))
    for _ in range(2):
        sh(block(release, 'sudo -u volund /usr/local/bin/volundd migrate'))
    proxy_configuration(release)
    Path('/etc/caddy/Caddyfile').write_text(
        f'https://{HOSTNAME} {{\n\tbind {PROXY_ADDRESS}\n\ttls internal\n'
        f'\treverse_proxy {PROXY_ADDRESS}:8080\n}}\n')
    Path('/etc/systemd/system/volund-j3-net.service').write_text(
        '[Unit]\nDescription=J.3 probe veth address\nBefore=network-online.target caddy.service\n'
        'Wants=network-online.target\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\n'
        f'ExecStart=/usr/sbin/ip addr replace {PROXY_ADDRESS}/30 dev host0\n'
        'ExecStart=/usr/sbin/ip link set host0 up\n\n[Install]\nWantedBy=multi-user.target\n')
    run('systemctl', 'enable', 'volund-j3-net.service', 'caddy.service', 'postgresql.service')
    library = Path('/srv/volund/library/probe')
    library.mkdir()
    run(Path('/opt') / release / 'release/volund-cad-fixture', library / 'fixture.step')
    sh(f'chown -R volund:volund {library} && chmod 0750 {library} && chmod 0640 {library}/fixture.step')
    SLOW_CONVERTER.parent.mkdir(parents=True, exist_ok=True)
    SLOW_CONVERTER.write_text('#!/bin/sh\nsleep 120\nexec /usr/local/bin/volund-cad-convert "$@"\n')
    SLOW_CONVERTER.chmod(0o755)
    run('pg_ctlcluster', '17', 'volund', 'stop')
    Path('/usr/sbin/policy-rc.d').unlink()
    print(f'PASS: Erstinstallation ({release}) aus dem Runbook, Units aktiviert', flush=True)


def switch(release):
    run('pg_ctlcluster', '17', 'volund', 'start')
    try:
        out = Path('/opt') / release / 'release'
        run('sudo', '-u', 'volund', out / 'volundd', 'migrate')
        run('sudo', '-u', 'volund', out / 'volundd', 'database-doctor')
        install_release(release)
        proxy_configuration(release)
        run('sudo', '-u', 'volund', '/usr/local/bin/volundd', 'database-doctor')
    finally:
        run('pg_ctlcluster', '17', 'volund', 'stop')
    print(f'PASS: auf {release} umgestellt, database-doctor grün', flush=True)


def active_since(unit):
    value = subprocess.check_output(['systemctl', 'show', '-P', 'ActiveEnterTimestampMonotonic', unit],
                                    text=True).strip()
    return int(value)


def verify_boot(release):
    state = subprocess.run(['systemctl', 'is-system-running', '--wait'], capture_output=True, text=True)
    failed = subprocess.check_output(['systemctl', '--failed', '--no-legend', '--plain'], text=True)
    assert state.stdout.strip() == 'running', (state.stdout, failed)
    for unit in ['postgresql@17-volund.service', 'volund-proxy-firewall.service', 'volundd.service',
                 'caddy.service', *[f'{timer}.timer' for timer in TIMERS]]:
        run('systemctl', 'is-active', '--quiet', unit)
    assert active_since('volund-proxy-firewall.service') < active_since('volundd.service'), \
        'firewall must be active before the non-loopback listener'
    ruleset = subprocess.check_output(['nft', 'list', 'table', 'inet', 'volund_proxy'], text=True)
    assert f'ip saddr {PROXY_ADDRESS} tcp dport 8080 accept' in ruleset, ruleset
    for name in ('volundd', 'volund-cad-convert'):
        digest = hashlib.sha256(Path(f'/usr/local/bin/{name}').read_bytes()).hexdigest()
        expected = hashlib.sha256((Path('/opt') / release / 'release' / name).read_bytes()).hexdigest()
        assert digest == expected, f'{name} is not the {release} build'
    with urllib.request.urlopen('http://127.0.0.1:8080/api/v1/health', timeout=5) as response:
        assert json.load(response)['status'] == 'ok'
    deadline = time.monotonic() + 60
    while subprocess.run(['systemctl', 'show', '-P', 'LastTriggerUSecMonotonic',
                          'volund-preview-worker.timer'], capture_output=True, text=True
                         ).stdout.strip() in ('', '0'):
        assert time.monotonic() < deadline, 'preview worker timer never fired'
        time.sleep(1)
    print(f'PASS: Boot ({release}) running, Firewall vor volundd, Timer aktiv und ausgelöst', flush=True)


def slow_worker():
    """Pause the timer and make the next conversion slow enough to interrupt."""
    run('systemctl', 'stop', 'volund-preview-worker.timer', 'volund-preview-worker.service')
    SLOW_DROPIN.parent.mkdir(exist_ok=True)
    SLOW_DROPIN.write_text(f'[Service]\nEnvironment=VOLUND_CONVERTER={SLOW_CONVERTER}\n')
    run('systemctl', 'daemon-reload')
    print('PASS: Vorschau-Worker pausiert, langsamer Konverter aktiv', flush=True)


def stop_worker(run_id):
    """Stop the worker unit mid-conversion; the job must be requeued (ADR 0030)."""
    assert re.fullmatch(r'[0-9a-f-]{36}', run_id)
    query = ("SELECT status||'|'||attempt FROM volund.conversion_runs WHERE public_id::text='"
             + run_id + "'")

    def job():
        return subprocess.check_output(['sudo', '-u', 'volund', 'psql', '-XAt', '-d', 'volund',
                                        '-c', query], text=True).strip()

    assert SLOW_DROPIN.is_file()
    run('systemctl', 'start', '--no-block', 'volund-preview-worker.service')
    deadline = time.monotonic() + 60
    while job() != 'running|1' or not subprocess.run(['pgrep', '-f', str(SLOW_CONVERTER)],
                                                      capture_output=True).returncode == 0:
        assert time.monotonic() < deadline, job()
        time.sleep(0.5)
    started = time.monotonic()
    run('systemctl', 'stop', 'volund-preview-worker.service')
    assert time.monotonic() - started < 30, 'stop must finish within TimeoutStopSec'
    result = subprocess.check_output(['systemctl', 'show', '-P', 'Result',
                                      'volund-preview-worker.service'], text=True).strip()
    assert result == 'success', result
    assert job() == 'queued|2', job()
    assert subprocess.run(['pgrep', '-f', str(SLOW_CONVERTER)], capture_output=True).returncode != 0
    SLOW_DROPIN.unlink()
    run('systemctl', 'daemon-reload')
    run('systemctl', 'start', 'volund-preview-worker.timer')
    print('PASS: systemctl stop mitten in der Konvertierung: Job neu eingereiht, Prozesse beendet',
          flush=True)


mode = sys.argv[1]
if mode == 'packages':
    packages()
elif mode == 'build':
    build(sys.argv[2])
elif mode == 'install':
    install(sys.argv[2])
elif mode == 'switch':
    switch(sys.argv[2])
elif mode == 'verify-boot':
    verify_boot(sys.argv[2])
elif mode == 'slow-worker':
    slow_worker()
elif mode == 'stop-worker':
    stop_worker(sys.argv[2])
else:
    raise AssertionError('unknown probe mode')

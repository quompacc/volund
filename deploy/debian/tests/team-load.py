#!/usr/bin/env python3
"""Read-dominant eight-user resource measurement in an isolated Debian test root."""

import concurrent.futures
import getpass
import hashlib
import json
import math
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

USERS = 8
TARGET_RPS = 20.0
WARMUP_SECONDS = 15
MEASURE_SECONDS = 180
PORT = 18098
LIMITS = {
    'minimum_rps': 18.0,
    'maximum_error_rate': 0.0,
    'p95_ms': 250.0,
    'p99_ms': 750.0,
    'daemon_peak_rss_mib': 256.0,
    'postgres_peak_rss_mib': 512.0,
    'combined_average_cpu_cores': 2.0,
    'database_growth_mib': 64.0,
    'maximum_app_connections': 5,
    'maximum_lock_waiters': 0,
}

root = Path(sys.argv[1])
dump = Path(sys.argv[2])
assert root.is_absolute() and root.resolve() == root
assert root.parent == Path('/var/tmp') and re.fullmatch(r'volund-k2-[0-9]+', root.name)
assert dump.is_file() and dump.parent == root
assert os.geteuid() != 0
user = getpass.getuser()
work = root / 'team-load'
work.mkdir(mode=0o700)
data = work / 'postgres'
sock = work / 'socket'
sock.mkdir(mode=0o700)
for name in ('web', 'derived', 'support'):
    (work / name).mkdir(mode=0o750)
(work / 'web/index.html').write_text('<!doctype html><title>K2</title>')
pgbin = Path('/usr/lib/postgresql/17/bin')
binary = root / 'bin/volundd'
assert binary.stat().st_uid == 0 and not os.access(binary, os.W_OK)


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def sql(database, statement, admin=False):
    role = 'k2admin' if admin else user
    target = 'postgres' if admin else database
    return run('psql', '-X', '-v', 'ON_ERROR_STOP=1', '-h', sock, '-p', '18498',
               '-U', role, '-d', target, '-Atc', statement,
               capture_output=True, text=True).stdout.strip()


def environment():
    result = {key: value for key, value in os.environ.items() if not key.startswith('VOLUND_')}
    result.update({
        'VOLUND_DATABASE_URL': f'postgresql:///volund_k2_test?host={sock}&port=18498&user={user}',
        'VOLUND_LISTEN_ADDR': f'127.0.0.1:{PORT}',
        'VOLUND_WEB_ROOT': str(work / 'web'),
        'VOLUND_DERIVED_ROOT': str(work / 'derived'),
        'VOLUND_SUPPORT_ROOT': str(work / 'support'),
    })
    return result


def descendants(parent):
    relationships = {}
    for entry in Path('/proc').iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = (entry / 'stat').read_text().split()
            relationships.setdefault(int(fields[3]), []).append(int(entry.name))
        except (FileNotFoundError, ProcessLookupError, PermissionError, IndexError):
            continue
    found = {parent}
    pending = [parent]
    while pending:
        children = relationships.get(pending.pop(), [])
        found.update(children)
        pending.extend(children)
    return found


def process_usage(pids):
    ticks = 0
    rss_kib = 0
    for pid in pids:
        try:
            fields = Path(f'/proc/{pid}/stat').read_text().split()
            ticks += int(fields[13]) + int(fields[14])
            for line in Path(f'/proc/{pid}/status').read_text().splitlines():
                if line.startswith('VmRSS:'):
                    rss_kib += int(line.split()[1])
                    break
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
    return ticks, rss_kib / 1024


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


run(pgbin / 'initdb', '-D', data, '-U', 'k2admin', '--encoding=UTF8', '--no-locale',
    '--data-checksums', '--auth-local=peer', '--auth-host=reject', stdout=subprocess.DEVNULL)
(data / 'pg_hba.conf').write_text('local all k2admin peer map=k2\nlocal all all peer\n')
(data / 'pg_ident.conf').write_text(f'k2 {user} k2admin\n')
pg_started = False
daemon = None
daemon_log = None
try:
    run(pgbin / 'pg_ctl', '-D', data, '-l', work / 'postgres.log',
        '-o', f"-k {sock} -p 18498 -c listen_addresses='' -c unix_socket_permissions=0700",
        '-w', 'start')
    pg_started = True
    sql('', f'CREATE ROLE {user} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE', admin=True)
    sql('', f'CREATE DATABASE volund_k2_test OWNER {user} TEMPLATE template0 ENCODING \'UTF8\'', admin=True)
    run('pg_restore', '--exit-on-error', '--single-transaction', '--no-owner', '--no-privileges',
        '-h', sock, '-p', '18498', '-U', user, '-d', 'volund_k2_test', dump,
        stdout=subprocess.DEVNULL)
    run(binary, 'migrate', env=environment(), stdout=subprocess.DEVNULL)
    run(binary, 'database-doctor', env=environment(), stdout=subprocess.DEVNULL)
    tokens = []
    owner = sql('volund_k2_test', "SELECT account.id FROM volund.users account "
                "JOIN volund.password_credentials credential ON credential.user_id=account.id "
                "WHERE account.role='owner' AND account.status='active' ORDER BY account.id LIMIT 1")
    assert owner.isdigit()
    for number in range(USERS):
        token = hashlib.sha256(f'volund-k2-session-{number}'.encode()).hexdigest()
        digest = hashlib.sha256(token.encode()).hexdigest()
        csrf = hashlib.sha256(f'volund-k2-csrf-{number}'.encode()).hexdigest()
        sql('volund_k2_test', "INSERT INTO volund.sessions "
            "(user_id,token_digest,csrf_digest,idle_expires_at,absolute_expires_at,user_agent) "
            f"VALUES ({owner},'{digest}','{csrf}',now()+interval '30 minutes',"
            "now()+interval '8 hours','isolated-k2-load')")
        tokens.append(token)
    model_id = sql('volund_k2_test',
                   "SELECT public_id::text FROM volund.models ORDER BY id LIMIT 1")
    assert model_id
    endpoints = ('/api/v1/models', '/api/v1/roots',
                 f'/api/v1/models/{model_id}/history?limit=50&offset=0',
                 '/api/v1/jobs?limit=50&offset=0', '/api/v1/settings',
                 '/api/v1/preferences', '/api/v1/session')
    queue_before = sql('volund_k2_test', "SELECT 'scan:'||status||':'||count(*) FROM volund.scan_runs "
                       "GROUP BY status UNION ALL SELECT 'preview:'||status||':'||count(*) "
                       "FROM volund.conversion_runs GROUP BY status ORDER BY 1")
    database_before = int(sql('volund_k2_test', "SELECT pg_database_size('volund_k2_test')"))
    daemon_log = (work / 'daemon.log').open('w')
    daemon = subprocess.Popen([binary, 'serve'], env=environment(), cwd=work,
                              stdout=daemon_log, stderr=subprocess.STDOUT)
    origin = f'http://127.0.0.1:{PORT}'
    for _attempt in range(100):
        assert daemon.poll() is None
        try:
            urllib.request.urlopen(origin + '/api/v1/health', timeout=1).close()
            break
        except urllib.error.URLError:
            time.sleep(0.1)
    else:
        raise AssertionError('K2-Testdaemon startet nicht')

    latencies = []
    endpoint_latencies = {endpoint: [] for endpoint in endpoints}
    errors = []
    lock = threading.Lock()
    measurement_start = time.monotonic() + WARMUP_SECONDS
    measurement_end = measurement_start + MEASURE_SECONDS

    def virtual_user(number):
        next_request = time.monotonic()
        index = number
        while time.monotonic() < measurement_end:
            started = time.monotonic()
            endpoint = endpoints[index % len(endpoints)]
            request = urllib.request.Request(origin + endpoint,
                                             headers={'Cookie': f'volund_session={tokens[number]}'})
            status = None
            try:
                with urllib.request.urlopen(request, timeout=5) as response:
                    status = response.status
                    response.read()
            except (urllib.error.URLError, TimeoutError) as error:
                status = type(error).__name__
            elapsed = (time.monotonic() - started) * 1000
            if started >= measurement_start:
                with lock:
                    latencies.append(elapsed)
                    endpoint_latencies[endpoint].append(elapsed)
                    if status != 200:
                        errors.append({'endpoint': endpoint, 'status': str(status)})
            index += 1
            next_request += USERS / TARGET_RPS
            time.sleep(max(0, next_request - time.monotonic()))

    with concurrent.futures.ThreadPoolExecutor(max_workers=USERS) as executor:
        futures = [executor.submit(virtual_user, number) for number in range(USERS)]
        time.sleep(WARMUP_SECONDS)
        postgres_pid = int((data / 'postmaster.pid').read_text().splitlines()[0])
        ticks_start = process_usage({daemon.pid} | descendants(postgres_pid))[0]
        peak_daemon_rss = 0.0
        peak_postgres_rss = 0.0
        maximum_connections = 0
        maximum_active = 0
        maximum_lock_waiters = 0
        while time.monotonic() < measurement_end:
            _ticks, daemon_rss = process_usage({daemon.pid})
            _ticks, postgres_rss = process_usage(descendants(postgres_pid))
            peak_daemon_rss = max(peak_daemon_rss, daemon_rss)
            peak_postgres_rss = max(peak_postgres_rss, postgres_rss)
            connections = sql('volund_k2_test', "SELECT count(*)||':'||"
                              "count(*) FILTER(WHERE state='active')||':'||"
                              "count(*) FILTER(WHERE wait_event_type='Lock') "
                              "FROM pg_stat_activity WHERE application_name='volundd'")
            total, active, lock_waiters = map(int, connections.split(':'))
            maximum_connections = max(maximum_connections, total)
            maximum_active = max(maximum_active, active)
            maximum_lock_waiters = max(maximum_lock_waiters, lock_waiters)
            time.sleep(1)
        for future in futures:
            future.result()
    ticks_end = process_usage({daemon.pid} | descendants(postgres_pid))[0]
    queue_after = sql('volund_k2_test', "SELECT 'scan:'||status||':'||count(*) FROM volund.scan_runs "
                      "GROUP BY status UNION ALL SELECT 'preview:'||status||':'||count(*) "
                      "FROM volund.conversion_runs GROUP BY status ORDER BY 1")
    database_after = int(sql('volund_k2_test', "SELECT pg_database_size('volund_k2_test')"))
    assert queue_after == queue_before
    result = {
        'profile': {'users': USERS, 'targetRps': TARGET_RPS, 'warmupSeconds': WARMUP_SECONDS,
                    'measureSeconds': MEASURE_SECONDS, 'requests': len(latencies)},
        'throughputRps': len(latencies) / MEASURE_SECONDS,
        'errors': len(errors),
        'errorRate': len(errors) / len(latencies),
        'latencyMs': {'p50': percentile(latencies, .50), 'p95': percentile(latencies, .95),
                      'p99': percentile(latencies, .99), 'max': max(latencies)},
        'endpointP95Ms': {endpoint: percentile(values, .95)
                          for endpoint, values in endpoint_latencies.items()},
        'daemonPeakRssMiB': peak_daemon_rss,
        'postgresPeakRssMiB': peak_postgres_rss,
        'combinedAverageCpuCores': (ticks_end - ticks_start) / os.sysconf('SC_CLK_TCK') / MEASURE_SECONDS,
        'databaseGrowthMiB': (database_after - database_before) / 1048576,
        'databasePool': {'maximumConnections': maximum_connections,
                         'maximumActive': maximum_active,
                         'maximumLockWaiters': maximum_lock_waiters},
        'queueStable': True,
    }
    assert result['throughputRps'] >= LIMITS['minimum_rps'], result
    assert result['errorRate'] <= LIMITS['maximum_error_rate'], result
    assert result['latencyMs']['p95'] <= LIMITS['p95_ms'], result
    assert result['latencyMs']['p99'] <= LIMITS['p99_ms'], result
    assert result['daemonPeakRssMiB'] <= LIMITS['daemon_peak_rss_mib'], result
    assert result['postgresPeakRssMiB'] <= LIMITS['postgres_peak_rss_mib'], result
    assert result['combinedAverageCpuCores'] <= LIMITS['combined_average_cpu_cores'], result
    assert result['databaseGrowthMiB'] <= LIMITS['database_growth_mib'], result
    assert result['databasePool']['maximumConnections'] <= LIMITS['maximum_app_connections'], result
    assert result['databasePool']['maximumLockWaiters'] <= LIMITS['maximum_lock_waiters'], result
    print(json.dumps({'limits': LIMITS, 'result': result}, sort_keys=True), flush=True)
finally:
    if daemon is not None and daemon.poll() is None:
        daemon.terminate()
        try:
            daemon.wait(timeout=15)
        except subprocess.TimeoutExpired:
            daemon.kill()
            daemon.wait(timeout=5)
    if daemon_log is not None:
        daemon_log.close()
    if pg_started:
        run(pgbin / 'pg_ctl', '-D', data, '-m', 'fast', '-w', 'stop')
    assert not (data / 'postmaster.pid').exists()
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', PORT))
    print('PASS: eigene Prozesse beendet und Testport freigegeben', flush=True)

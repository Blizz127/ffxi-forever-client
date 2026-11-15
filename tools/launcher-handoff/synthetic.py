#!/usr/bin/env python3
"""Exact-binary launcher handoff checks. Run only in the network-none wrapper.

Uses invented credentials, a temporary loopback TLS server and a private Xvfb.
Never calls accounts, a game server, the publisher, or a real launcher session.
"""
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import threading
import time

ROOT = Path('/evidence')
LOADER = ['/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2',
          '--library-path', '/runtime-libs:/usr/lib/x86_64-linux-gnu']
USER = 'synthetic_user'
PASSWORD = 'synthetic-password-25'
TOKEN = 'a1' * 32
SOCIAL = 'synthetic-social-token-never-auth'


def check(condition, label):
    # Do not dump requests or fixture values on failure.
    if not condition:
        raise AssertionError(label)


class XClientMessage(C.Structure):
    _fields_ = [('type', C.c_int), ('serial', C.c_ulong), ('send_event', C.c_int),
                ('display', C.c_void_p), ('window', C.c_ulong),
                ('message_type', C.c_ulong), ('format', C.c_int), ('data', C.c_long * 5)]


class XEvent(C.Union):
    _fields_ = [('message', XClientMessage), ('padding', C.c_long * 24)]


class Display:
    def __init__(self):
        self.x = C.CDLL('libX11.so.6')
        definitions = {
            'XOpenDisplay': ([C.c_char_p], C.c_void_p),
            'XDefaultRootWindow': ([C.c_void_p], C.c_ulong),
            'XInternAtom': ([C.c_void_p, C.c_char_p, C.c_int], C.c_ulong),
            'XQueryTree': ([C.c_void_p, C.c_ulong, C.POINTER(C.c_ulong), C.POINTER(C.c_ulong), C.POINTER(C.POINTER(C.c_ulong)), C.POINTER(C.c_uint)], C.c_int),
            'XGetWindowProperty': ([C.c_void_p, C.c_ulong, C.c_ulong, C.c_long, C.c_long, C.c_int, C.c_ulong, C.POINTER(C.c_ulong), C.POINTER(C.c_int), C.POINTER(C.c_ulong), C.POINTER(C.c_ulong), C.POINTER(C.POINTER(C.c_ubyte))], C.c_int),
            'XSendEvent': ([C.c_void_p, C.c_ulong, C.c_int, C.c_long, C.POINTER(XEvent)], C.c_int),
            'XFlush': ([C.c_void_p], C.c_int),
            'XCloseDisplay': ([C.c_void_p], C.c_int),
            'XFree': ([C.c_void_p], C.c_int),
        }
        for name, (args, result) in definitions.items():
            getattr(self.x, name).argtypes = args
            getattr(self.x, name).restype = result
        self.d = self.x.XOpenDisplay(None)
        check(self.d, 'private X display opened')

    def window(self, pid):
        root, parent, children, count = C.c_ulong(), C.c_ulong(), C.POINTER(C.c_ulong)(), C.c_uint()
        self.x.XQueryTree(self.d, self.x.XDefaultRootWindow(self.d), C.byref(root), C.byref(parent), C.byref(children), C.byref(count))
        try:
            for window in children[:count.value]:
                actual, fmt, n, left, data = C.c_ulong(), C.c_int(), C.c_ulong(), C.c_ulong(), C.POINTER(C.c_ubyte)()
                self.x.XGetWindowProperty(self.d, window, self.x.XInternAtom(self.d, b'_NET_WM_PID', 0), 0, 1, 0, 0, C.byref(actual), C.byref(fmt), C.byref(n), C.byref(left), C.byref(data))
                try:
                    if fmt.value == 32 and n.value == 1 and C.cast(data, C.POINTER(C.c_ulong))[0] == pid:
                        return window
                finally:
                    if data:
                        self.x.XFree(data)
        finally:
            if children:
                self.x.XFree(children)

    def close(self, pid):
        window = self.window(pid)
        check(window, 'client window found for graceful close')
        event = XEvent()
        event.message = XClientMessage(33, 0, 1, self.d, window,
            self.x.XInternAtom(self.d, b'WM_PROTOCOLS', 0), 32,
            (C.c_long * 5)(self.x.XInternAtom(self.d, b'WM_DELETE_WINDOW', 0), 0, 0, 0, 0))
        check(self.x.XSendEvent(self.d, window, 0, 0, C.byref(event)), 'window close sent')
        self.x.XFlush(self.d)


def run_case(display, name, handoff, response, forever=True, expected_auth=True):
    directory = ROOT / name
    directory.mkdir()
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen(4)
    listener.settimeout(.2)
    port = listener.getsockname()[1]
    config = {
        'game_dir': '/game', 'installed': False, 'server': 'FFXI Forever' if forever else 'regular',
        'named_servers': [{'name': 'FFXI Forever' if forever else 'regular', 'host': '127.0.0.1',
                           'auth_port': port, 'loader_version': '2.1.2'}],
        'username': '', 'pol_id': '', 'logins': [], 'remember_password': False, 'saved_password': '',
        'frame_limit': 30, 'graphics_detected': True, 'software_gpu_seen': True,
        'modern_migrated': True, 'modern_ui': forever, 'ui_scale': 1,
        'tool': {'0003': 640, '0004': 400, '0034': 1, '0022': 0},
        'graphics': {'fxaa': False, 'bloom': False, 'tonemap': False, 'ao': False, 'fog': False},
    }
    (directory / 'config.json').write_text(json.dumps(config))
    tokenfile = directory / 'friends-token'
    tokenfile.write_text(json.dumps({'token': SOCIAL, 'scope': 'social.chat'}))
    tokenfile.chmod(0o600)
    feed = {'url': 'https://accounts.invalid/v1/social/friends', 'token_file': str(tokenfile),
            'scope': 'social.chat', 'inbox_tells': False}
    if isinstance(handoff, dict):
        handoff = {**handoff, 'friends_feed': feed}
        content = json.dumps(handoff)
    else:
        content = handoff
    login = directory / 'login-fixture.json'
    login.write_text(content)
    login.chmod(0o600)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_3
    context.load_cert_chain('/tmp/fixture.pem', '/tmp/fixture.key')
    requests, errors = [], []
    stop = threading.Event()

    def serve():
        while not stop.is_set():
            try:
                connection, _ = listener.accept()
            except socket.timeout:
                continue
            try:
                with context.wrap_socket(connection, server_side=True) as connection:
                    connection.settimeout(5)
                    body = b''
                    while len(body) <= 65536:
                        chunk = connection.recv(4096)
                        check(chunk, 'auth request not truncated')
                        body += chunk
                        try:
                            request = json.loads(body)
                            break
                        except ValueError:
                            continue
                    else:
                        raise AssertionError('auth request bounded')
                    requests.append(request)
                    connection.sendall(json.dumps(response).encode())
            except Exception as error:
                errors.append(type(error).__name__)

    server = threading.Thread(target=serve)
    server.start()
    env = {**os.environ, 'HOME': str(directory), 'FFXI_CONFIG': str(directory / 'config.json'),
           'VANA_LOGIN_FILE': str(login), 'VANA_MANAGED': '1', 'RUST_LOG': 'info'}
    started = time.monotonic()
    process = None
    try:
        with (directory / 'process.log').open('w') as output:
            process = subprocess.Popen(LOADER + ['/app/vana-client'], cwd=directory, env=env, stdout=output, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 45
            while time.monotonic() < deadline:
                check(process.poll() is None, 'client remains running')
                if display.window(process.pid) and not login.exists() and (requests or not expected_auth):
                    break
                time.sleep(.1)
            check(not login.exists(), 'one-use login file removed')
            check(display.window(process.pid), 'native window initialized')
            # Includes the 10-second automatic-reconnect interval, if a refusal incorrectly schedules one.
            time.sleep(12 if expected_auth else 3)
            check(not errors, 'TLS stub exchange clean')
            check(len(requests) == int(expected_auth), f'auth request count: expected {int(expected_auth)}, observed {len(requests)}')
            if expected_auth:
                request = requests[0]
                check(request.get('command') == 0x10, 'login command only')
                check(request.get('username') == USER and request.get('password') == PASSWORD, 'game credentials forwarded')
                check(request.get('version') == [2, 1, 2], 'named-server loader override retained')
                check(request.get('otp') == '', 'launch token is not an OTP')
                check(SOCIAL not in json.dumps(request), 'social token never sent to game auth')
                if forever:
                    check(request.get('client') == 'banshee-forever' and request.get('client_build') == 25, 'Forever marker and build')
                    check(request.get('launch_token') == handoff.get('launch_token'), 'launch token forwarded or omitted')
                else:
                    check(not {'client', 'client_build', 'launch_token'} & request.keys(), 'no Forever auth fields on regular server')
            display.close(process.pid)
            check(process.wait(timeout=15) == 0, 'normal window-close exit')
        logs = '\n'.join(p.read_text(errors='replace') for p in directory.glob('*.log'))
        check(not any(secret in logs for secret in [PASSWORD, TOKEN, SOCIAL]), 'no fixture secrets in logs')
        if expected_auth and response['result'] == 0x14:
            check('login refused: the server requires the Forever client (0x14)' in logs, 'client-lock refusal handled')
        if expected_auth and response['result'] == 0x0b:
            check('keeping the refusal for the player' in logs, 'version refusal kept without fallback')
        return {'name': name, 'result': 'PASS', 'auth_requests': len(requests),
                'one_use_file_deleted': True, 'normal_exit': 0, 'secrets_logged': False,
                'elapsed_seconds': round(time.monotonic() - started, 2)}
    finally:
        stop.set()
        server.join(timeout=7)
        listener.close()
        # Failure cleanup is restricted to this synthetic child. It is not a passing graceful-close check.
        if process and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        login.unlink(missing_ok=True)
        tokenfile.unlink(missing_ok=True)


def main():
    check(os.environ.get('SYNTHETIC_NETWORK_NONE') == '1', 'use network-none wrapper')
    check(sorted(p.name for p in Path('/sys/class/net').iterdir()) == ['lo'], 'only loopback network present')
    binary_sha = hashlib.sha256(Path('/app/vana-client').read_bytes()).hexdigest()
    check(binary_sha == os.environ['EXPECTED_BINARY_SHA256'], 'exact packaged binary')
    ROOT.mkdir(exist_ok=True)
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
                    '-nodes', '-keyout', '/tmp/fixture.key', '-out', '/tmp/fixture.pem', '-days', '1',
                    '-subj', '/CN=127.0.0.1'], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    with (ROOT / 'xvfb.log').open('w') as output:
        xvfb = subprocess.Popen(LOADER + ['/usr/bin/Xvfb', ':88', '-screen', '0', '800x600x24', '-nolisten', 'tcp'], stdout=output, stderr=subprocess.STDOUT)
        display = None
        results = []
        try:
            for _ in range(100):
                if Path('/tmp/.X11-unix/X88').exists():
                    break
                check(xvfb.poll() is None, 'private X server started')
                time.sleep(.1)
            display = Display()
            saved = {'username': USER, 'password': PASSWORD, 'launch_token': TOKEN}
            cases = [
                ('saved-forever-lock-refusal', saved, {'result': 0x14}, True, True),
                ('missing-launch-token', {'username': USER, 'password': PASSWORD}, {'result': 0x14}, True, True),
                ('version-refusal', saved, {'result': 0x0b, 'error_message': 'Use loader 2.1.0'}, True, True),
                ('regular-server-isolation', saved, {'result': 2}, False, True),
                ('feed-only-no-autologin', {}, {'result': 2}, True, False),
                ('malformed-handoff', '{', {'result': 2}, True, False),
                ('oversized-handoff', ' ' * (65536 + 1), {'result': 2}, True, False),
            ]
            for case in cases:
                result = run_case(display, *case)
                results.append(result)
                print(json.dumps(result), flush=True)
        finally:
            if display:
                display.x.XCloseDisplay(display.d)
            xvfb.terminate()
            xvfb.wait(timeout=5)
            (ROOT / 'result.json').write_text(json.dumps({
                'binary_sha256': binary_sha, 'tests': results,
                'all_passed': len(results) == 7,
                'scope': 'Exact Linux .25 default launcher entry, synthetic handoff and loopback TLS auth only',
                'not_run': ['real launcher account mint/session', 'real auth/lobby/zone', 'Windows GUI', 'owner device', 'publication'],
            }, indent=2) + '\n')


if __name__ == '__main__':
    main()

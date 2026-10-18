#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["paho-mqtt>=2.1"]
# ///
"""Control the Bambu Lab A1 over its LAN interfaces (no cloud).

MQTT/TLS :8883 for status and commands, implicit FTPS :990 for files on the
printer's microSD card, TLS :6000 for the camera's JPEG stream. All three log
in as user "bblp" with the access code from the printer screen
(Settings > WLAN). See README.md beside this file.
"""
import argparse
import ftplib
import json
import os
import socket
import ssl
import struct
import sys
import threading
import time
from pathlib import Path

import paho.mqtt.client as mqtt

HERE = Path(__file__).resolve().parent


def load_config():
    env_file = HERE / '.env'
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith('#') and '=' in line:
                key, value = line.split('=', 1)
                os.environ.setdefault(key.strip(), value.strip())
    missing = [k for k in ('BAMBU_HOST', 'BAMBU_SERIAL', 'BAMBU_ACCESS_CODE') if not os.environ.get(k)]
    if missing:
        sys.exit(f'missing {", ".join(missing)}: set them in {env_file} or the environment')
    return os.environ['BAMBU_HOST'], os.environ['BAMBU_SERIAL'], os.environ['BAMBU_ACCESS_CODE']


def tls_context():
    # The printer presents a certificate from Bambu's private CA with the
    # serial as CN; there is no public chain to verify against.
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    return ctx


class Printer:
    def __init__(self, host, serial, code):
        self.host, self.serial, self.code = host, serial, code
        self.state = {}
        self.updated = threading.Event()
        self.client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, protocol=mqtt.MQTTv311)
        self.client.username_pw_set('bblp', code)
        self.client.tls_set_context(tls_context())
        self.client.on_connect = self._on_connect
        self.client.on_message = self._on_message

    def _on_connect(self, client, userdata, flags, reason, properties):
        if reason.is_failure:
            sys.exit(f'MQTT login refused: {reason} (wrong access code, or LAN mode off?)')
        client.subscribe(f'device/{self.serial}/report')
        self.send({'pushing': {'command': 'pushall'}})

    def _on_message(self, client, userdata, msg):
        data = json.loads(msg.payload)
        # A1 reports deltas; merge them into one picture of the printer.
        for section, values in data.items():
            if isinstance(values, dict):
                self.state.setdefault(section, {}).update(values)
            else:
                self.state[section] = values
        self.updated.set()

    def connect(self):
        self.client.connect(self.host, 8883, keepalive=30)
        self.client.loop_start()
        if not self.updated.wait(10):
            sys.exit('connected but no report arrived within 10 s')
        time.sleep(1.0)  # let the pushall finish arriving
        return self

    def send(self, payload):
        for body in payload.values():
            body.setdefault('sequence_id', str(int(time.time())))
        self.client.publish(f'device/{self.serial}/request', json.dumps(payload))

    def close(self):
        time.sleep(0.5)
        self.client.loop_stop()
        self.client.disconnect()


def loaded(p):
    """tray_now 255 means no AMS filament is in the toolhead; 254 is the external spool."""
    return str((p.get('ams') or {}).get('tray_now', '255')) != '255'


def air_printing(p):
    return p.get('gcode_state') in ('RUNNING', 'PAUSE') and int(p.get('layer_num') or 0) >= 1 and not loaded(p)


def summary(state):
    p = state.get('print', {})
    lines = [
        f"state      {p.get('gcode_state', '?')}  ({p.get('mc_percent', '?')}%  layer {p.get('layer_num', '?')}/{p.get('total_layer_num', '?')}  {p.get('mc_remaining_time', '?')} min left)",
        f"job        {p.get('subtask_name') or '-'}",
        f"nozzle     {p.get('nozzle_temper', '?')} / {p.get('nozzle_target_temper', '?')} C",
        f"bed        {p.get('bed_temper', '?')} / {p.get('bed_target_temper', '?')} C",
        f"fans       part {p.get('cooling_fan_speed', '?')}  heatbreak {p.get('heatbreak_fan_speed', '?')}",
        f"wifi       {p.get('wifi_signal', '?')}",
        f"sd card    {p.get('sdcard', '?')}",
        f"errors     {p.get('print_error', 0)}  hms {p.get('hms', [])}",
        f"toolhead   {'nothing loaded' if not loaded(p) else 'loaded'}",
    ]
    lights = p.get('lights_report') or []
    if lights:
        lines.append('lights     ' + ', '.join(f"{l['node']}={l['mode']}" for l in lights))
    tray = (p.get('vt_tray') or {})
    if tray:
        lines.append(f"external   {tray.get('tray_type') or 'none'} {tray.get('tray_color', '')}")
    for unit in (p.get('ams') or {}).get('ams', []):
        for t in unit.get('tray', []):
            if t.get('tray_type'):
                lines.append(f"ams slot {int(t['id'])+1} {t['tray_type']} #{t['tray_color'][:6]}")
    return '\n'.join(lines)


class ImplicitFTPS(ftplib.FTP_TLS):
    """Port 990 is TLS from the first byte, and the data channel must reuse
    the control channel's TLS session or the printer drops it."""

    def __init__(self):
        super().__init__(context=tls_context())
        self._sock = None

    @property
    def sock(self):
        return self._sock

    @sock.setter
    def sock(self, value):
        if value is not None and not isinstance(value, ssl.SSLSocket):
            value = self.context.wrap_socket(value)
        self._sock = value

    def ntransfercmd(self, cmd, rest=None):
        conn, size = ftplib.FTP.ntransfercmd(self, cmd, rest)
        if self._prot_p:
            conn = self.context.wrap_socket(conn, server_hostname=self.host, session=self.sock.session)
            # The printer never answers a TLS close_notify; waiting for it times out.
            conn.unwrap = lambda: None
        return conn, size


def ftp(host, code):
    f = ImplicitFTPS()
    f.connect(host, 990, timeout=15)
    f.login('bblp', code)
    f.prot_p()
    return f


def snapshot(host, code, out, timeout=15):
    auth = struct.pack('<IIII', 0x40, 0x3000, 0, 0) + b'bblp'.ljust(32, b'\0') + code.encode().ljust(32, b'\0')
    with socket.create_connection((host, 6000), timeout=timeout) as raw:
        with tls_context().wrap_socket(raw, server_hostname=host) as s:
            s.sendall(auth)

            def read(n):
                buf = b''
                while len(buf) < n:
                    chunk = s.recv(n - len(buf))
                    if not chunk:
                        raise ConnectionError('camera closed the stream (access code wrong, or LAN liveview off?)')
                    buf += chunk
                return buf

            header = read(16)
            size = struct.unpack('<I', header[:4])[0]
            jpeg = read(size)
    if not jpeg.startswith(b'\xff\xd8'):
        sys.exit('camera returned something that is not a JPEG')
    Path(out).write_bytes(jpeg)
    print(f'saved {out} ({len(jpeg)} bytes)')


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest='cmd', required=True)
    sub.add_parser('status', help='one-shot status summary')
    sub.add_parser('raw', help='full merged state as JSON')
    w = sub.add_parser('watch', help='print status every N seconds')
    w.add_argument('--every', type=float, default=10)
    s = sub.add_parser('snapshot', help='save one camera frame')
    s.add_argument('out', nargs='?', default='a1-snapshot.jpg')
    ck = sub.add_parser('check', help='timestamped camera frame + status, for watching a print')
    ck.add_argument('--dir', default='a1-watch')
    sub.add_parser('ls', help='list files on the microSD card')
    u = sub.add_parser('upload', help='upload a sliced .gcode.3mf to the card')
    u.add_argument('file')
    pr = sub.add_parser('print', help='start a .gcode.3mf already on the card')
    pr.add_argument('name', help='file name on the card, e.g. Housing.gcode.3mf')
    pr.add_argument('--plate', type=int, default=1)
    pr.add_argument('--no-level', action='store_true', help='skip auto bed levelling')
    pr.add_argument('--timelapse', action='store_true')
    pr.add_argument('--slot', type=int, help='AMS Lite slot 1-4 to feed from; omit for the external spool')
    for name in ('pause', 'resume', 'stop'):
        sub.add_parser(name)
    li = sub.add_parser('light', help='work light on/off')
    li.add_argument('mode', choices=['on', 'off'])
    g = sub.add_parser('gcode', help='send raw G-code lines, e.g. "G28"')
    g.add_argument('lines', nargs='+')
    sp = sub.add_parser('speed', help='print speed profile')
    sp.add_argument('level', choices=['silent', 'standard', 'sport', 'ludicrous'])
    args = ap.parse_args()
    host, serial, code = load_config()

    if args.cmd == 'snapshot':
        return snapshot(host, code, args.out)
    if args.cmd == 'check':
        out = Path(args.dir); out.mkdir(parents=True, exist_ok=True)
        frame = out / time.strftime('%Y%m%d-%H%M%S.jpg')
        snapshot(host, code, frame)
        p = Printer(host, serial, code).connect()
        print(summary(p.state))
        st = p.state.get('print', {})
        p.close()
        if air_printing(st):
            print('ALERT      printing layers with no filament in the toolhead')
            sys.exit(2)
        # Non-zero exit flags something the printer itself already knows is wrong.
        sys.exit(1 if st.get('print_error') or st.get('hms') or st.get('gcode_state') == 'FAILED' else 0)
    if args.cmd == 'ls':
        f = ftp(host, code)
        for name in f.nlst():
            print(name)
        return f.quit()
    if args.cmd == 'upload':
        path = Path(args.file)
        f = ftp(host, code)
        with path.open('rb') as fh:
            f.storbinary(f'STOR {path.name}', fh)
        f.quit()
        return print(f'uploaded {path.name}')

    p = Printer(host, serial, code).connect()
    try:
        if args.cmd == 'status':
            print(summary(p.state))
        elif args.cmd == 'raw':
            print(json.dumps(p.state, indent=2))
        elif args.cmd == 'watch':
            while True:
                print(time.strftime('%H:%M:%S'), '-' * 50)
                print(summary(p.state))
                time.sleep(args.every)
        elif args.cmd in ('pause', 'resume', 'stop'):
            p.send({'print': {'command': args.cmd}})
        elif args.cmd == 'light':
            p.send({'system': {'command': 'ledctrl', 'led_node': 'chamber_light', 'led_mode': args.mode,
                               'led_on_time': 500, 'led_off_time': 500, 'loop_times': 0, 'interval_time': 0}})
        elif args.cmd == 'gcode':
            p.send({'print': {'command': 'gcode_line', 'param': '\n'.join(args.lines) + '\n'}})
        elif args.cmd == 'speed':
            level = {'silent': 1, 'standard': 2, 'sport': 3, 'ludicrous': 4}[args.level]
            p.send({'print': {'command': 'print_speed', 'param': str(level)}})
        elif args.cmd == 'print':
            if args.slot and not loaded(p.state.get('print', {})):
                # Twice the start sequence skipped the AMS load and printed in the air.
                sys.exit('nothing is loaded in the toolhead; load the slot from the printer screen first')
            p.send({'print': {
                'command': 'project_file', 'param': f'Metadata/plate_{args.plate}.gcode',
                'url': f'file:///sdcard/{args.name}', 'subtask_name': args.name,
                'project_id': '0', 'profile_id': '0', 'task_id': '0', 'subtask_id': '0',
                'md5': '', 'timelapse': args.timelapse, 'bed_type': 'auto',
                'bed_levelling': not args.no_level, 'flow_cali': False, 'vibration_cali': True,
                'layer_inspect': False, 'use_ams': args.slot is not None,
                'ams_mapping': [args.slot - 1] if args.slot else '',
                # Firmware 01.08 also reads the per-unit form Bambu Studio sends.
                'ams_mapping2': [{'ams_id': 0, 'slot_id': args.slot - 1}] if args.slot else []}})
            print(f'start requested for {args.name}; check with `status`')
    except KeyboardInterrupt:
        pass
    finally:
        p.close()


if __name__ == '__main__':
    main()

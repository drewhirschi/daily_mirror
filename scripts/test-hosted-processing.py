#!/usr/bin/env python3
"""Run the real packaged Vercel adapters, codecs and inference on disposable data."""
import io
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
from PIL import Image
import requests

ROOT = Path(__file__).resolve().parents[1]
FUNCTIONS = ROOT / 'server/.vercel/output/functions/__nextrs_functions'
TOKEN = 'synthetic-hosted-verification-token'

def port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]

def main():
    processes = []
    checks = []
    with tempfile.TemporaryDirectory(prefix='daily-mirror-hosted-') as folder:
        temp = Path(folder)
        db = temp / 'data.db'
        photo_dir = temp / 'photos'
        origins = {name: f'http://127.0.0.1:{port()}' for name in ['default', 'images', 'vision', 'broken-dispatch']}
        env = {k: v for k, v in os.environ.items() if not k.startswith(('DAILY_MIRROR_', 'VERCEL', 'CRON_SECRET', 'MEDIAPIPE'))}
        env.update(DAILY_MIRROR_STORAGE_BACKEND='local', DAILY_MIRROR_STORAGE_DIR=str(photo_dir),
                   DAILY_MIRROR_DATABASE_URL=str(db), DAILY_MIRROR_UPLOAD_TOKEN=TOKEN,
                   DAILY_MIRROR_PROCESSOR_TOKEN=TOKEN, CRON_SECRET=TOKEN,
                   DAILY_MIRROR_PROCESSING_PIPELINE='face-v5', DAILY_MIRROR_HOSTED_PROCESSING='1',
                   # The claim gate lives in the vision bundle; images/default dispatch to it.
                   DAILY_MIRROR_HOSTED_CONCURRENCY='3',
                   DAILY_MIRROR_WORKER_URL=origins['vision'] + '/api/processing/run',
                   MEDIAPIPE_LIB='resources/vision/lib/libmediapipe.so', NEXTRS_PUBLIC_DIR=str(ROOT/'server/public'))
        logs = {}
        def call(name, method, path, expected, label, **kwargs):
            r = requests.request(method, origins[name]+path, timeout=120, allow_redirects=False, **kwargs)
            assert r.status_code == expected, (label, r.status_code, r.text[:200])
            checks.append(label)
            return r
        def status(photo):
            if not db.exists(): return None
            try:
                with sqlite3.connect(db) as c:
                    row = c.execute('SELECT status FROM photo_processing WHERE photo_id=?', (photo,)).fetchone()
                    return row[0] if row else None
            except sqlite3.OperationalError: return None
        def completed(photo, tries=200):
            for _ in range(tries):
                if status(photo) == 'complete': return
                time.sleep(0.1)
            raise AssertionError(f'{photo} did not complete: {status(photo)}')
        def live_hosted_leases():
            """Rows holding an unexpired vercel lease right now, as the claim gate counts them."""
            if not db.exists(): return 0
            try:
                with sqlite3.connect(db) as c:
                    return c.execute("SELECT count(*) FROM photo_processing WHERE status='leased'"
                                     " AND leased_by LIKE 'vercel-%'"
                                     " AND lease_expires_at > CURRENT_TIMESTAMP").fetchone()[0]
            except sqlite3.OperationalError: return 0
        try:
            for name, origin in origins.items():
                bundle = 'images' if name == 'broken-dispatch' else name
                folder = FUNCTIONS / (bundle + '.func')
                log = (temp/(name+'.log')).open('w'); logs[name] = log
                settings = {**env, 'VERCEL_DEV_PORT':origin.rsplit(':',1)[1]}
                if name == 'broken-dispatch': settings['DAILY_MIRROR_WORKER_URL'] = f'http://127.0.0.1:{port()}/missing'
                p = subprocess.Popen([str(folder/'executable')], cwd=folder, env=settings, stdout=log, stderr=log)
                processes.append(p)
                for _ in range(100):
                    assert p.poll() is None, (temp/(name+'.log')).read_text()
                    try:
                        requests.get(origin+'/healthz', timeout=0.2)
                        break
                    except requests.ConnectionError: time.sleep(0.05)
                else: raise AssertionError(name+' did not start')
            for name in ['default', 'images']:
                assert not (FUNCTIONS/(name+'.func')/'resources/vision').exists()
            checks.append('native models and libraries are exclusive to vision')
            auth = {'Authorization':'Bearer '+TOKEN}
            call('vision','POST','/api/processing/run',401,'worker rejects invalid credentials',json={},headers={'Authorization':'Bearer invalid'})
            call('default','GET','/api/maintenance/process',401,'recovery cron rejects invalid credentials')
            call('images','POST','/api/processing/run',404,'images excludes worker route',json={},headers=auth)
            call('default','POST','/api/processing/run',404,'default excludes worker route',json={},headers=auth)
            fixture = io.BytesIO(); Image.new('RGB',(800,600),(100,140,180)).save(fixture,format='JPEG')
            jpeg = fixture.getvalue()
            first = '20260907T120000Z-hosted01'
            start = time.monotonic()
            call('images','POST','/api/photos',201,'upload starts actual hosted inference',data=jpeg,
                 headers={**auth,'x-capture-id':first,'Content-Type':'image/jpeg'})
            response_seconds = time.monotonic()-start
            completed(first)
            checks.append('real YuNet MediaPipe SFace pipeline completes without manual dispatch')
            with sqlite3.connect(db) as c:
                assert c.execute('SELECT oriented_width,oriented_height FROM photo_analyses WHERE photo_id=?',(first,)).fetchone() == (800,600)
                assert c.execute('SELECT count(*) FROM faces WHERE photo_id=?',(first,)).fetchone()[0] == 0
            checks.append('blank JPEG produces correct dimensions and zero faces')
            call('vision','POST','/api/processing/run',200,'duplicate notification is harmless',json={'photo_id':first,'continue_queue':False},headers=auth)
            with sqlite3.connect(db) as c:
                assert c.execute('SELECT attempt_count FROM photo_processing WHERE photo_id=?',(first,)).fetchone()[0] == 1
            time.sleep(1)
            second = '20260907T120001Z-hosted02'
            call('broken-dispatch','POST','/api/photos',201,'upload survives failed notification',data=jpeg,
                 headers={**auth,'x-capture-id':second,'Content-Type':'image/jpeg'})
            assert status(second) == 'pending'
            call('default','GET','/api/maintenance/process',200,'NextRS recovery route waits for actual processing',headers=auth)
            completed(second)
            checks.append('recovery cron processes a job whose upload notification failed')
            # Three back-to-back uploads each notify with their own photo id, so three
            # independent worker invocations claim in parallel while the limit is 3.
            batch = ['20260907T1202%02dZ-parallel%d' % (index, index) for index in range(3)]
            for capture in batch:
                call('images','POST','/api/photos',201,f'parallel upload {capture} accepted',data=jpeg,
                     headers={**auth,'x-capture-id':capture,'Content-Type':'image/jpeg'})
            peak_leases = 0
            for _ in range(300):
                peak_leases = max(peak_leases, live_hosted_leases())
                if peak_leases >= 2: break
                time.sleep(0.05)
            assert peak_leases >= 2, f'never observed overlapping hosted leases (peak {peak_leases})'
            checks.append(f'concurrency 3 holds {peak_leases} hosted leases at the same instant')
            for capture in batch:
                completed(capture, tries=600)
            with sqlite3.connect(db) as c:
                attempts = [c.execute('SELECT attempt_count FROM photo_processing WHERE photo_id=?',
                                      (capture,)).fetchone()[0] for capture in batch]
            assert attempts == [1,1,1], attempts
            checks.append('all three parallel photos complete on their first attempt')
            print(json.dumps({'kind':'local-packaged-vercel-adapters-with-native-inference','passed':len(checks),
                              'upload_response_seconds':response_seconds,'peak_hosted_leases':peak_leases,
                              'checks':checks},indent=2))
        except BaseException:
            for name, log in logs.items():
                log.flush()
                print(name, (temp/(name+'.log')).read_text()[-4000:])
            raise
        finally:
            for p in processes: p.terminate()
            for p in processes:
                try:p.wait(timeout=5)
                except subprocess.TimeoutExpired:p.kill();p.wait()
            for log in logs.values():log.close()

if __name__ == '__main__': main()

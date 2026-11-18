#!/usr/bin/env bash
# Install the already-built Release app from jsn onto drew-25.
# Needs: the iPhone on this Linux box's LAN (for its _remotepairing advert) and
# Tailscale connected on the phone (jsn relays to 100.116.255.48).
set -euo pipefail
PHONE_TS=${PHONE_TS:-100.116.255.48}
BIND=192.168.0.106
APP='~/work/daily-mirror-mobile/build-jsn/Build/Products/Release-iphoneos/DailyMirror.app'

# The phone's tunnel ports move around between sessions; relay whatever it has open now plus a window around them.
open_ports=$(python3 - <<'PY'
import asyncio
async def probe(p, sem):
    async with sem:
        try:
            r, w = await asyncio.wait_for(asyncio.open_connection('192.168.4.36', p), 0.8); w.close(); return p
        except Exception: return None
async def main():
    sem = asyncio.Semaphore(800)
    found = sorted(p for p in await asyncio.gather(*[probe(p, sem) for p in range(49153, 65536)]) if p and p != 62078)
    lo = max(49153, (found[0] if found else 56150) - 120); hi = min(65535, lo + 300)
    print(",".join([f"{lo}-{hi}"] + [str(p) for p in found if not lo <= p <= hi][:40]))
asyncio.run(main())
PY
)
echo "relaying ports $open_ports"
line=$(timeout 10 avahi-browse -rtp _remotepairing._tcp 2>/dev/null | grep '^=' | grep IPv4 | head -1 || true)
if [ -z "$line" ]; then echo "NO_ADVERT: iPhone not advertising _remotepairing on this LAN"; exit 3; fi
IFS=';' read -r _ _ _ instance _ _ host _ port txt <<<"$line"
# avahi -p escapes with \DDD decimal; decode those.
instance=$(python3 -c 'import re,sys; print(re.sub(r"\\(\d{3})", lambda m: chr(int(m.group(1))), sys.argv[1]))' "$(echo "$line" | cut -d';' -f4)")
host=${host%.local}
txtargs=$(python3 -c 'import shlex,sys,re; t=sys.argv[1]; items=re.findall(r"\"([^\"]*)\"", t); print(" ".join("--txt "+shlex.quote(i) for i in items))' "$txt")
echo "instance=$instance host=$host port=$port"

ssh -F /dev/null -o BatchMode=yes drew@jsn "export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
set -e
ulimit -n 8192
pkill -f coredevice-tailnet-bridge.py 2>/dev/null || true
nohup python3 ~/coredevice-tailnet-bridge.py --bind $BIND --phone $PHONE_TS \
  --instance $(printf %q "$instance") --hostname $(printf %q "$host.local") --service-port $port \
  --ports $open_ports $txtargs > ~/bridge.log 2>&1 &
for i in \$(seq 1 30); do
  if xcrun devicectl list devices 2>/dev/null | grep drew-25 | grep -qv unavailable; then break; fi
  sleep 2
done
xcrun devicectl list devices | grep drew-25
xcrun devicectl device install app --device drew-25 $APP
xcrun devicectl device process launch --device drew-25 app.dailymirror.ios || true
pkill -f coredevice-tailnet-bridge.py || true"

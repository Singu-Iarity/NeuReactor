#!/usr/bin/env python3
"""Minimal NeuReactor plugin: registers, waits for operator approval, then
prints a few readings every second. Standard library only.

    python3 examples/plugins/telemetry_logger.py [http://127.0.0.1:8642]
"""
import json
import sys
import time
import urllib.request

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8642") + "/api/v1"


def call(path, token=None, body=None):
    req = urllib.request.Request(BASE + path, method="POST" if body is not None else "GET")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    data = json.dumps(body).encode() if body is not None else None
    if data:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, data) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        return json.load(e)


reg = call("/plugins/register", body={"name": "Telemetry logger", "scopes": ["telemetry_read"]})
token = reg["token"]
print("Registered; approve 'Telemetry logger' in the control room Plugins panel...")
while not call("/plugins/me", token)["plugin"]["approved"]:
    time.sleep(1)

while True:
    t = call("/telemetry", token)
    r = t["readings"]
    print(f"t={t['sim_time']:8.0f}s  P={r['nuclear_power_pct']:6.2f}%  "
          f"Tavg={r['tavg_c']:6.2f}C  p={r['rcs_pressure_mpa']:5.2f}MPa  {r['gross_mwe']:6.0f} MWe"
          + ("  TRIPPED" if t["reactor_tripped"] else ""))
    time.sleep(1)

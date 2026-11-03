"""POST 探活：激活端点是否仍匿名可达（不跟随重定向）"""
import json
import ssl
import urllib.request
import urllib.error

CTX = ssl._create_unverified_context()

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None

opener = urllib.request.build_opener(NoRedirect, urllib.request.HTTPSHandler(context=CTX))

body = json.dumps({"code": "PROBE-0000", "alias": "probe", "device_fp": "probe", "os": "windows", "app_version": "probe"}).encode()
for host in ["https://bridge.daoson.top:8443", "https://orch.daoson.top:8443"]:
    url = host + "/api/terminals/activate"
    req = urllib.request.Request(url, data=body, headers={"content-type": "application/json"}, method="POST")
    try:
        with opener.open(req, timeout=15) as r:
            print(host, "→", r.status, r.read()[:120])
    except urllib.error.HTTPError as e:
        loc = e.headers.get("Location", "-")[:70]
        print(host, "→", e.code, "|", loc, "|", e.read()[:100])
    except Exception as e:
        print(host, "→", type(e).__name__, str(e)[:80])

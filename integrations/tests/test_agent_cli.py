"""Exercise the actual PowerShell client against an isolated loopback fixture.
No ReleaseHub user data, real credentials, GitHub calls or third-party packages.
Run: python integrations/tests/test_agent_cli.py [--shell powershell.exe|pwsh]
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SCRIPT = Path(__file__).resolve().parents[1] / "releasehub-agent.ps1"
TOKEN = "a" * 64
TICKET = "b" * 64
COMMIT = "c" * 40
SHELL = "pwsh"


class Fixture(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        self.handle_request()

    def do_POST(self):
        self.handle_request()

    def handle_request(self):
        state = self.server.state
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode("utf-8")
        state["requests"].append((self.command, self.path, dict(self.headers), json.loads(body) if body else None))
        code = state.get("http_status", 200)
        value = {"error": "fixture failure " + TOKEN + " " + TICKET} if code != 200 else None
        if self.headers.get("Authorization") != "Bearer " + TOKEN:
            code, value = 401, {"error": "Missing token"}
        if code == 200:
            if self.path == "/v1/health":
                value = {"name": "ReleaseHub", "installApi": False}
            elif self.path == "/v1/projects":
                value = state["projects"]
            elif self.path == "/v1/check-updates":
                value = {"checked": len((json.loads(body) or {}).get("projectIds", state["projects"])), "updates": 1, "errors": 0}
            elif self.path == "/v1/build-tasks":
                state["polls"] += 1
                if state.get("finish_after") and state["polls"] >= state["finish_after"]:
                    state["tasks"][0]["status"] = "succeeded"
                value = state["tasks"]
            elif self.path.endswith("/claim"):
                state["tasks"][0]["status"] = "claimed"
                value = {"task": state["tasks"][0], "claimToken": TICKET, "expiresInSeconds": 7200}
            elif self.path.endswith(("/progress", "/result")):
                if self.headers.get("X-ReleaseHub-Claim") != TICKET:
                    code, value = 403, {"error": "Wrong claim ticket"}
                else:
                    payload = json.loads(body)
                    state["tasks"][0].update(payload)
                    state["tasks"][0]["status"] = payload.get("status", "succeeded")
                    value = state["tasks"][0]
            else:
                code, value = 404, {"error": "Unknown route"}
        if state.get("delay"):
            time.sleep(state["delay"])
        raw = (state.get("raw") or json.dumps(value, ensure_ascii=False)).encode("utf-8")
        self.send_response(code)
        if code in (301, 302, 307):
            self.send_header("Location", "/redirect-target")
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        try:
            self.wfile.write(raw)
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass


class ClientTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.url = "http://127.0.0.1:" + str(cls.server.server_port)

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join()

    def setUp(self):
        self.state = {
            "requests": [], "polls": 0,
            "projects": [{"id": "project-1", "name": "追新", "repository": "https://github.com/example/app"}, {"id": "project-2", "name": "Other", "repository": "https://github.com/example/other"}],
            "tasks": [{"id": "task-1", "projectName": "追新", "repository": "example/app", "sourceCommit": COMMIT, "status": "queued"}],
        }
        self.server.state = self.state

    def call(self, *args, expected=0, env=None, offline=False):
        variables = os.environ.copy()
        # Deliberately replace, never use, credentials inherited from the machine.
        variables.update(RELEASEHUB_AGENT_TOKEN=TOKEN, RELEASEHUB_CLAIM_TOKEN=TICKET, RELEASEHUB_AGENT_URL=self.url)
        if env:
            variables.update(env)
        command = [SHELL, "-NoLogo", "-NoProfile", "-NonInteractive", "-File", str(SCRIPT), *args, "-Json"]
        result = subprocess.run(command, env=variables, capture_output=True, timeout=18)
        stdout = result.stdout.decode("utf-8-sig", errors="strict").strip()
        stderr = result.stderr.decode("utf-8-sig", errors="replace").strip()
        self.assertEqual(result.returncode, expected, f"{args}: {stdout}\n{stderr}")
        self.assertFalse(stderr, stderr)
        parsed = json.loads(stdout)
        self.assertEqual(parsed["ok"], expected == 0)
        self.assertEqual(parsed["schemaVersion"], 1)
        if offline:
            self.assertEqual(self.state["requests"], [])
        return parsed

    def test_offline_help_schema_and_usage(self):
        self.assertIn("commands", self.call("help", env={"RELEASEHUB_AGENT_TOKEN": ""}, offline=True)["data"])
        self.assertIn("wait", self.call("schema", offline=True)["data"]["commands"])
        self.assertEqual(self.call("unknown", expected=2, offline=True)["error"]["code"], "invalid_arguments")
        self.call("health", "-Failed", expected=2, offline=True)

    def test_validation_never_sends_request(self):
        self.call("health", "-BaseUrl", "https://example.com", expected=2, offline=True)
        self.call("health", env={"RELEASEHUB_AGENT_TOKEN": ""}, expected=2, offline=True)
        self.call("claim", "-TaskId", "../other", expected=2, offline=True)
        self.call("result", "-TaskId", "task-1", "-ArtifactUrl", "https://github.com/owner/repo/releases/tag/v1?token=bad", "-SourceCommit", COMMIT, expected=2, offline=True)
        self.call("progress", "-TaskId", "task-1", expected=2, offline=True)

    def test_health_and_filters_keep_array_shape(self):
        self.assertEqual(self.call("health")["data"]["name"], "ReleaseHub")
        projects = self.call("projects", "-Query", "追新")["data"]
        self.assertEqual([p["id"] for p in projects], ["project-1"])
        self.assertEqual(self.call("projects", "-Query", "missing")["data"], [])
        self.assertEqual(len(self.call("tasks", "-Status", "queued")["data"]), 1)
        self.assertEqual(self.call("tasks", "-Status", "failed")["data"], [])
        self.state["tasks"] = []
        self.assertEqual(self.call("tasks")["data"], [])

    def test_targeted_check_never_sends_download_option(self):
        self.call("check", "-ProjectIds", "project-1,project-2")
        method, route, _, body = self.state["requests"][-1]
        self.assertEqual((method, route), ("POST", "/v1/check-updates"))
        self.assertEqual(body, {"projectIds": ["project-1", "project-2"]})
        self.call("check")
        self.assertEqual(self.state["requests"][-1][3], {})

    def test_task_lookup_and_missing(self):
        self.assertEqual(self.call("task", "-TaskId", "task-1")["data"]["status"], "queued")
        self.assertEqual(self.call("task", "-TaskId", "missing", expected=8)["error"]["code"], "task_not_found")

    def test_claim_progress_result_and_unicode(self):
        claim = self.call("claim", "-TaskId", "task-1", "-AgentName", "local-test")["data"]
        self.assertEqual(claim["claimToken"], TICKET)
        result = self.call("progress", "-TaskId", "task-1", "-Message", "正在编译，保留原软件")["data"]
        self.assertEqual(result["progress"], "正在编译，保留原软件")
        result = self.call("result", "-TaskId", "task-1", "-SourceCommit", COMMIT, "-ArtifactUrl", "https://github.com/example/app/actions/runs/123", "-Sha256", "d" * 64)["data"]
        self.assertEqual(result["status"], "succeeded")
        self.assertEqual(result["artifactSha256"], "d" * 64)
        self.assertEqual(len(self.state["requests"]), 3)

    def test_failed_progress_and_expired_ticket(self):
        data = self.call("progress", "-TaskId", "task-1", "-Message", "Build failed", "-Failed")["data"]
        self.assertEqual(data["status"], "failed")
        self.assertEqual(data["error"], "Build failed")
        self.call("progress", "-TaskId", "task-1", "-Message", "Again", env={"RELEASEHUB_CLAIM_TOKEN": "e" * 64}, expected=4)

    def test_http_errors_redact_tokens_and_do_not_retry(self):
        for status, expected in [(401, 4), (409, 5), (429, 5), (500, 5)]:
            self.state["http_status"] = status
            result = self.call("claim", "-TaskId", "task-1", expected=expected)
            self.assertEqual(result["error"]["httpStatus"], status)
            self.assertNotIn(TOKEN, json.dumps(result))
            self.assertNotIn(TICKET, json.dumps(result))
            self.assertEqual(result["error"]["outcomeUnknown"], status == 500)
        self.assertEqual(len(self.state["requests"]), 4)

    def test_redirect_is_not_followed(self):
        self.state["http_status"] = 307
        result = self.call("health", expected=5)
        self.assertEqual(result["error"]["code"], "redirect_refused")
        self.assertEqual(len(self.state["requests"]), 1)

    def test_invalid_json(self):
        self.state["raw"] = "not-json"
        self.assertEqual(self.call("health", expected=5)["error"]["code"], "invalid_response")

    def test_bounded_wait_and_terminal_states(self):
        self.state["finish_after"] = 2
        self.assertEqual(self.call("wait", "-TaskId", "task-1", "-PollIntervalSec", "2", "-WaitTimeoutSec", "6")["data"]["status"], "succeeded")
        self.state.pop("finish_after")
        self.state["tasks"][0]["status"] = "failed"
        self.assertEqual(self.call("wait", "-TaskId", "task-1", expected=6)["data"]["status"], "failed")
        self.state["tasks"][0]["status"] = "cancelled"
        self.call("wait", "-TaskId", "task-1", expected=6)
        self.state["tasks"][0]["status"] = "running"
        self.assertEqual(self.call("wait", "-TaskId", "task-1", "-WaitTimeoutSec", "1", expected=7)["error"]["code"], "wait_timeout")
        self.assertTrue(all(req[0] == "GET" for req in self.state["requests"]))

    def test_request_timeout_and_proxy_bypass(self):
        self.call("health", env={"HTTP_PROXY": "http://127.0.0.1:1", "ALL_PROXY": "http://127.0.0.1:1", "NO_PROXY": ""})
        self.state["delay"] = 2
        response = self.call("check", "-TimeoutSec", "1", expected=3)
        self.assertEqual(response["error"]["code"], "request_timeout")
        self.assertTrue(response["error"]["outcomeUnknown"])
        self.assertEqual(len(self.state["requests"]), 2)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--shell", default="pwsh")
    args, remaining = parser.parse_known_args()
    SHELL = shutil.which(args.shell) or args.shell
    unittest.main(argv=[__file__, *remaining], verbosity=2)

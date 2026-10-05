from __future__ import annotations

import importlib.util
import json
import os
import re
import subprocess
import sys
import textwrap
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "validation" / "release" / "workflow-contracts.py"
SPEC = importlib.util.spec_from_file_location("workflow_contracts", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
contracts = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = contracts
SPEC.loader.exec_module(contracts)


class WorkflowCompilerEnvironmentTests(unittest.TestCase):
    def test_rejects_workflow_global_rustflags(self) -> None:
        workflow = """name: ci
env:
  RUSTFLAGS: \"-D warnings --cfg aes_armv8\"
jobs:
  embedded:
    runs-on: ubuntu-latest
    steps:
      - run: cargo check --target thumbv7em-none-eabihf
"""

        self.assertEqual(
            contracts.validate_ci_compiler_environment(workflow),
            [
                "ci.yml must not define workflow-global RUSTFLAGS; cross-toolchain jobs "
                "inherit them"
            ],
        )

    def test_rejects_host_rustflags_on_a_cross_toolchain_job(self) -> None:
        workflow = """name: ci
env:
  CARGO_TERM_COLOR: always
jobs:
  embedded:
    runs-on: ubuntu-latest
    env:
      RUSTFLAGS: \"-D warnings --cfg aes_armv8\"
    steps:
      - run: cargo check --target riscv32imac-unknown-none-elf
"""

        self.assertEqual(
            contracts.validate_ci_compiler_environment(workflow),
            [
                "ci.yml cross-toolchain job embedded must not define host RUSTFLAGS"
            ],
        )

    def test_allows_job_scoped_rustflags_for_host_only_work(self) -> None:
        workflow = """name: ci
env:
  CARGO_TERM_COLOR: always
jobs:
  host:
    runs-on: ubuntu-latest
    env:
      RUSTFLAGS: \"-D warnings --cfg aes_armv8\"
    steps:
      - run: cargo test --workspace --locked
"""

        self.assertEqual(contracts.validate_ci_compiler_environment(workflow), [])


class WorkflowSchedulingTests(unittest.TestCase):
    def workflow_jobs(self, name: str) -> dict[str, str]:
        return dict(contracts.workflow_jobs(
            (ROOT / ".github" / "workflows" / name).read_text(encoding="utf-8")
        ))

    def needs(self, block: str) -> set[str]:
        match = re.search(r"(?m)^    needs: (.+)$", block)
        self.assertIsNotNone(match, "job must declare its prerequisites")
        return {job.strip() for job in match.group(1).strip("[]").split(",")}

    def test_only_emulated_suites_wait_for_emulator_preparation(self) -> None:
        for workflow, lane, output, anchor, selector in (
            ("release-readiness.yml", "qualify", "emulated", "qualification-steps",
             "--tier release"),
            ("deep-validation.yml", "hardening", "hardening_emulated", "hardening-steps",
             '--domain hardening --tier "$VALIDATION_TIER"'),
        ):
            with self.subTest(workflow=workflow):
                jobs = self.workflow_jobs(workflow)
                emulated = f"{lane}-emulated"
                self.assertEqual(self.needs(jobs[lane]), {"inventory"})
                self.assertEqual(self.needs(jobs[emulated]), {"inventory", "embedded-emulators"})
                self.assertIn(f"steps: &{anchor}\n", jobs[lane])
                self.assertIn(f"steps: *{anchor}\n", jobs[emulated])
                self.assertIn(f"fromJSON(needs.inventory.outputs.{output})", jobs[emulated])
                self.assertIn(f"{selector} --emulators none)", jobs["inventory"])
                self.assertIn(f"{selector} --emulators required)", jobs["inventory"])
                self.assertTrue({lane, emulated} <= self.needs(jobs["embedded-assurance"]))
                gate = "aggregate" if lane == "qualify" else "deep-validation"
                self.assertTrue({lane, emulated, "embedded-assurance"} <= self.needs(jobs[gate]))
                self.assertIn("    if: always()\n", jobs[gate])
                if lane == "qualify":
                    self.assertIn(
                        'validation/run.py aggregate --tier release --expected-sha "${{ github.sha }}"',
                        jobs[gate],
                    )

    def test_release_setup_keeps_direct_and_nested_tool_consumers(self) -> None:
        qualify = self.workflow_jobs("release-readiness.yml")["qualify"]
        steps = re.split(r"(?m)(?=^      - )", qualify)
        selectors = {
            "node": "uses: actions/setup-node@",
            "uv": "uses: astral-sh/setup-uv@",
            "llvm": "name: Prepare embedded object inspection tools",
            "native": "name: Prepare Linux native development packages",
        }
        conditions = {}
        for tool, selector in selectors.items():
            step = next(step for step in steps if selector in step)
            conditions[tool] = re.search(r"(?m)^        if: (.+)$", step).group(1)

        selected = {tool: set() for tool in selectors}
        matrix = subprocess.run(
            [sys.executable, str(ROOT / "validation/run.py"), "matrix", "--tier", "release"],
            check=True, capture_output=True, text=True,
        )
        suites = json.loads(matrix.stdout)["include"]
        for suite in suites:
            for tool, condition in conditions.items():
                expression = re.sub(
                    r"matrix\.(\w+)", lambda match: repr(suite.get(match.group(1), "")),
                    condition,
                )
                os_name = "Linux" if suite["runner"].startswith("ubuntu-") else "Other"
                expression = expression.replace("runner.os", repr(os_name))
                expression = expression.replace("&&", "and").replace("||", "or")
                if eval(expression, {"__builtins__": {}}):
                    selected[tool].add(suite["id"])

        # Include indirect npm callers, not only suites whose command starts with npm.
        self.assertEqual(selected["node"], {
            "hopspot-javascript-package", "javascript-browser-package", "javascript-contract",
            "wasm-auto-wifi", "wasm-casework", "wasm-events", "wasm-websocket",
            "flasher-web", "esp32-firmware-check", "shipping-firmware",
            "dependency-audit", "release-contracts",
        })
        self.assertEqual(selected["uv"], {
            suite["id"] for suite in suites if suite["domain"] in {"oracles", "interop"}
        } | {"release-contracts"})
        self.assertEqual(selected["llvm"], {
            "embedded-builds", "esp32-firmware-check", "shipping-firmware",
            "embedded-isa-riscv32imac", "embedded-isa-thumbv7em", "embedded-isa-xtensa-esp32s3",
            "embedded-platform-esp32s3", "embedded-platform-nrf52840",
        })
        self.assertTrue({
            "host-workspaces", "integration-capstones", "sanitizer-address",
            "sanitizer-leak", "sanitizer-thread", "embedded-platform-nrf52840",
        } <= selected["native"])
        for suite in suites:
            if suite["domain"] in {"kani", "fuzz", "oracles"}:
                self.assertNotIn(suite["id"], selected["native"])

    def test_deep_validation_rejects_every_unsuccessful_lane(self) -> None:
        aggregate = self.workflow_jobs("deep-validation.yml")["deep-validation"]
        bindings = dict(re.findall(
            r"(?m)^      (\w+): \$\{\{ needs\.([\w-]+)\.result \}\}$", aggregate
        ))
        self.assertEqual(set(bindings.values()), self.needs(aggregate))
        script = textwrap.dedent(aggregate.split("      - run: |\n", 1)[1])
        successful = dict.fromkeys(bindings, "success")
        scenarios = [("all successful", successful, 0)]
        for variable in bindings:
            for status in ("failure", "cancelled", "skipped"):
                scenarios.append((f"{variable}={status}", {**successful, variable: status}, 1))
        for name, results, expected in scenarios:
            with self.subTest(name=name):
                result = subprocess.run(
                    ["bash", "-e", "-o", "pipefail", "-c", script],
                    env={**os.environ, **results}, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, expected, result.stderr)

    def test_feature_aggregate_rejects_every_unsuccessful_lane(self) -> None:
        jobs = self.workflow_jobs("ci.yml")
        aggregate = jobs["feature-configs"]
        lanes = {"feature-core", "feature-tokio", "feature-embassy", "feature-applications"}
        self.assertEqual(self.needs(aggregate), lanes)
        self.assertIn("    if: always()\n", aggregate)
        self.assertTrue(lanes <= jobs.keys())
        for lane in lanes:
            self.assertNotRegex(jobs[lane], r"(?m)^    (?:needs|if):")
        bindings = dict(re.findall(
            r"(?m)^      (\w+): \$\{\{ needs\.([\w-]+)\.result \}\}$", aggregate
        ))
        self.assertEqual(set(bindings.values()), lanes)
        script = textwrap.dedent(aggregate.split("        run: |\n", 1)[1])
        successful = dict.fromkeys(bindings, "success")
        scenarios = [("all successful", successful, 0)]
        for variable in bindings:
            for status in ("failure", "cancelled", "skipped"):
                scenarios.append((f"{variable}={status}", {**successful, variable: status}, 1))
        for name, results, expected in scenarios:
            with self.subTest(name=name):
                result = subprocess.run(
                    ["bash", "-e", "-o", "pipefail", "-c", script],
                    env={**os.environ, **results}, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, expected, result.stderr)
        self.assertIn("- feature-configs\n", jobs["release-critical"])

    def test_sdk_builds_overlap_contracts_but_publication_still_requires_them(self) -> None:
        jobs = self.workflow_jobs("host-sdks.yml")
        self.assertIn("preflight", jobs)
        for job in ("contract", "native", "android", "rust-packages"):
            with self.subTest(job=job):
                self.assertEqual(self.needs(jobs[job]), {"preflight"})
        publishers = {name for name in jobs if name.startswith("publish-")}
        self.assertEqual(publishers, {
            "publish-python", "publish-dotnet", "publish-maven", "publish-crates",
        })
        for name in publishers:
            with self.subTest(job=name):
                self.assertIn("contract", self.needs(jobs[name]))
                condition = jobs[name].split("    needs:", 1)[0]
                # Without status overrides, GitHub requires successful prerequisites.
                self.assertNotRegex(condition, r"\b(?:always|failure|cancelled)\s*\(")

    def test_javascript_browser_runs_independently_but_both_lanes_gate_release(self) -> None:
        jobs = self.workflow_jobs("napi.yml")
        self.assertNotRegex(jobs["javascript-browser"], r"(?m)^    (?:needs|if):")
        self.assertNotIn("actions/download-artifact@", jobs["javascript-browser"])
        self.assertEqual(self.needs(jobs["javascript-native"]), {"napi-build"})
        self.assertIn("bindings-x86_64-unknown-linux-gnu", jobs["javascript-native"])
        aggregate = jobs["javascript-hosts"]
        self.assertEqual(self.needs(aggregate), {"javascript-browser", "javascript-native"})
        self.assertIn("    if: always()\n", aggregate)
        self.assertIn("BROWSER_RESULT: ${{ needs.javascript-browser.result }}", aggregate)
        self.assertIn("NATIVE_RESULT: ${{ needs.javascript-native.result }}", aggregate)
        script = textwrap.dedent(aggregate.split("        run: |\n", 1)[1])
        for browser in ("success", "failure", "cancelled", "skipped"):
            for native in ("success", "failure", "cancelled", "skipped"):
                with self.subTest(browser=browser, native=native):
                    result = subprocess.run(
                        ["bash", "-e", "-o", "pipefail", "-c", script],
                        env={**os.environ, "BROWSER_RESULT": browser, "NATIVE_RESULT": native},
                        capture_output=True, text=True,
                    )
                    expected = 0 if browser == native == "success" else 1
                    self.assertEqual(result.returncode, expected, result.stderr)
        self.assertIn("- javascript-hosts\n", jobs["napi-release-critical"])
        self.assertIn("napi-release-critical", self.needs(jobs["npm-stage"]))
        self.assertEqual(self.needs(jobs["napi-publish"]), {"npm-stage"})


if __name__ == "__main__":
    unittest.main()

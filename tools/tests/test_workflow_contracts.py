from __future__ import annotations

import importlib.util
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


if __name__ == "__main__":
    unittest.main()

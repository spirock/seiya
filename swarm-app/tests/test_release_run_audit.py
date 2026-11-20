import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "release_run_audit", Path(__file__).resolve().parents[1] / "packaging/release_run_audit.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
SHA = "a" * 40


def fixture():
    repo = {"id": 42, "full_name": "spirock/seiya"}
    run = {"id": 7, "status": "completed", "conclusion": "success", "event": "push",
           "head_branch": "main", "head_sha": SHA, "repository": repo, "head_repository": repo}
    artifacts = {"artifacts": [
        {"name": name, "id": i + 1, "expired": False, "size_in_bytes": 1,
         "workflow_run": {"id": 7, "head_sha": SHA, "head_branch": "main",
                          "repository_id": 42, "head_repository_id": 42}}
        for i, name in enumerate(sorted(module.NAMES))
    ]}
    return run, artifacts


class ReleaseRunAuditTests(unittest.TestCase):
    def test_valid_run_and_artifacts(self):
        run, artifacts = fixture()
        self.assertEqual(set(module.audit(run, artifacts, SHA)), module.NAMES)

    def test_failed_wrong_sha_branch_and_fork_fail(self):
        for field, value in [("conclusion", "failure"), ("status", "in_progress"),
                             ("head_sha", "b" * 40), ("head_branch", "test"),
                             ("head_repository", {"id": 99, "full_name": "fork/seiya"})]:
            run, artifacts = fixture()
            run[field] = value
            with self.assertRaises(ValueError):
                module.audit(run, artifacts, SHA)

    def test_expired_missing_duplicate_and_cross_run_fail(self):
        for mutation in ("expired", "missing", "duplicate", "wrongrun", "wrongsha"):
            run, artifacts = fixture()
            items = artifacts["artifacts"]
            if mutation == "expired":
                items[0]["expired"] = True
            elif mutation == "missing":
                items.pop()
            elif mutation == "duplicate":
                items.append(copy.deepcopy(items[0]))
            elif mutation == "wrongrun":
                items[0]["workflow_run"]["id"] = 8
            else:
                items[0]["workflow_run"]["head_sha"] = "b" * 40
            with self.assertRaises(ValueError):
                module.audit(run, artifacts, SHA)


if __name__ == "__main__":
    unittest.main()

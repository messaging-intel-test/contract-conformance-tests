import unittest
from deep_tests.contract_model import Command, IdempotencyConflict, ReferenceStore, generate_valid_trace, replay
class ContractStressMatrixTests(unittest.TestCase):
    def test_replay_matrix(self):
        for seed in (7,101,2026):
            cmds=generate_valid_trace(seed,steps=900); expected=replay(cmds).snapshot()
            for n in (2,3,5,7,11): self.assertEqual(replay(cmds,duplicate_every=n).snapshot(),expected)
    def test_duplicate_storm(self):
        s=ReferenceStore(); c=Command("create","alpha","one","storm"); first=s.apply(c)
        for _ in range(128): self.assertEqual(s.apply(c),first)
        self.assertEqual(s.revision,1)
    def test_idempotency_binding_after_write_storm(self):
        s=ReferenceStore(); s.apply(Command("create","alpha","one","stable"))
        for i in range(100): s.apply(Command("create",f"key-{i}",str(i),f"u-{i}"))
        with self.assertRaises(IdempotencyConflict): s.apply(Command("update","alpha","changed","stable"))
if __name__ == "__main__": unittest.main()

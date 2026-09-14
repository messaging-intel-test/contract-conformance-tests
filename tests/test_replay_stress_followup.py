import unittest

from deep_tests.contract_model import Command, ReferenceStore, generate_valid_trace, replay


class ReplayStressFollowupTests(unittest.TestCase):
    def test_message_create_duplicate_storm_is_exactly_once(self):
        store = ReferenceStore()
        command = Command("create", "message-alpha", "queued", "message-create-key")
        first = store.apply(command)
        for _ in range(64):
            self.assertEqual(store.apply(command), first)
        self.assertEqual(store.revision, 1)

    def test_message_trace_converges_across_duplicate_schedules(self):
        commands = generate_valid_trace(202609144, steps=900)
        snapshots = {replay(commands, duplicate_every=n).snapshot() for n in (2, 3, 5, 7, 13, 17)}
        self.assertEqual(len(snapshots), 1)


if __name__ == "__main__":
    unittest.main()

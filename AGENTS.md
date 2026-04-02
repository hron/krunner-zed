Integration Test Guidelines

- Each integration test file under tests/ should contain at most one test that
  mutates process-global state (for example, using std::env::set_var or other
  global mutations).
- Prefer one integration test per file. Cargo builds each tests/*.rs into a
  separate test binary (process), so process-global state is isolated between
  files. This removes the need for in-process synchronization (ENV_LOCK) or a
  serialisation dev-dependency like serial_test for integration tests.
- If you need multiple logical assertions, either combine them into a single
  test function or split them into separate files under tests/.
- Rationale: tests in the same integration test file run in the same process
  and can race when mutating global state. Keeping tests in separate files
  provides isolation via separate processes.

CI lint (recommended)
- We add a CI lint that fails the build if a single tests/*.rs file contains
  more than one top-level test attribute. This is a heuristic intended to
  catch accidental multi-test files; it is not a perfect parser but helps
  enforce the guideline.

If you need a stronger check we can add a small parser script, but the
rg-based approach used in CI is simple and sufficient for the common cases.

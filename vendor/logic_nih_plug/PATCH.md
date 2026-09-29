Source: CuteDSP/logic_nih_plug adda1f92cd1cdaa0951293ca659c9aeafc9e9ae2 (ISC).
Only the framework library is vendored, not its example workspace.
Derive dependency remains pinned to that revision.

Local safety patch: CLAP state length is checked (1..=16MiB) before allocation.
Upstream aborts on random/malformed state lengths in clap-validator
state-invalid-random. Valid embedded Scala/profile states remain portable.

Additional local integration corrections:
- VST3 stream length is bounded to the same 16 MiB.
- Successful CLAP state load requests host parameter-value rescan.
- CLAP/VST3 plugin ownership uses AtomicRefCell instead of a callback mutex.
  Plugin implementations consequently require Sync; valid host lifecycle calls
  must not concurrently mutate the instance.
- LEGACY_RT_STATE_RESTORE=false bypasses legacy GUI state rendezvous polling
  and rejects host state loads while processing. adaptive-ji prepares GUI changes
  off-thread instead. Other framework users retain the default state policy.
- Transport::new is public for frontend callback tests.
- An unused macro reexport warning is suppressed.

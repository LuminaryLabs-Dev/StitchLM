# Four constrained connectors

Latest user instruction: at most four connectors, each capped at1GB, with very
little CPU usage. This replaces the20connector/10MBtest. Do not run that old test.

The native integration harness uses Linux prlimit with a1,000,000,000byte virtual
address-space ceiling, low scheduling priority and a10%CPU duty target. Virtual
address space includes mappings and is not the same metric as resident RAM. The
CPU duty target sleeps after compute and allows bursts; report measurements and
do not call it a cgroup-enforced percentage cap. No writable cgroup controller is
available in this sandbox. Browser memory must be measured independently.

Use real model outputs, compare local/distributed tokens, and test cancellation,
disconnection, recovery, stale results and unsupported formats. Connection counts
or synthetic tensor results alone cannot pass inference acceptance.

# Decision 4: Windows

**Out of scope.** macOS and Linux only, with code kept portable where it costs nothing (§1).

**WSL2** (recorded 2026-11-14, #529). Ladderback runs inside WSL2 as an ordinary Linux install, for files inside WSL; that is a Linux setup, not Windows support. It isn't a way to back up Windows, and isn't offered as one:
- WSL stops an idle distribution and its VM (Microsoft's `instanceIdleTimeout` and `vmIdleTimeout`), and a systemd service inside doesn't keep it running, so scheduled backups don't run unless something keeps WSL running.
- Files under `/mnt/c` are read through WSL's mount of the Windows drive: no shadow copies, so open files may fail, and no Windows metadata (ACLs, attributes, alternate data streams), so a restore to Windows loses it.
- Sleep, power and network (decision 80's link reads) are the VM's view, not Windows's.

These points are inferred from WSL's documentation until #528 reports a real run; the wording follows what it finds.

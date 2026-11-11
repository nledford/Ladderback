# Decision 3: Secrets on the hub

**The hub has its own secret entry** for each hub-managed repo. Agents never send secrets. Secrets can come from a command, keyring, file (incl. Docker/Podman secrets and systemd credentials), environment variable or inline. `${VAR}` / `${file:…}` substitution is available in repository fields (§10.2).

# Install Ladderback

Ladderback runs on each computer it backs up. Each system has one recommended way to install it, and its page says how to install, update and remove Ladderback that way:

- **[Debian, Ubuntu, Raspberry Pi OS and Fedora](install-linux-package.md):** Debian 12 or newer, Ubuntu 22.04 or newer, 64-bit Raspberry Pi OS and Fedora install the release's `.deb` or `.rpm` package.
- **[Other Linux](install-linux.md):** any other 64-bit Linux uses the install script, which downloads the release and checks it.
- **[macOS](install-macos.md):** the install script builds the release on your Mac, signed with your own certificate so that macOS keeps its permissions from one update to the next.

Linux means 64-bit (x86_64 or aarch64) with glibc 2.35 or newer: 32-bit ARM and musl distributions such as Alpine aren't supported, and neither is [Windows](#windows-and-wsl2). A hub for several computers isn't in a release yet. Once Ladderback is installed and started, [Getting started](getting-started.md) takes you to a first backup in the web UI. [Other ways to install](other-ways-to-install.md), such as the release's archive by hand or a build from source, and the [Docker image](docker.md), which isn't a supported way to run Ladderback yet, are in the Advanced part.

## Start Ladderback

Run this as yourself, not under `sudo`:

```sh
ladderback install-service
```

It runs Ladderback in the background from each login on, and starts it now. Open `http://127.0.0.1:9898` in your browser for the web UI, and go on with [Getting started](getting-started.md).

On Linux, a user's services stop when the user logs out: unless yours may already keep running, the command says to run `loginctl enable-linger`, which lets them. [Running as a service](running-as-a-service.md) has the details, how to stop it, and how to give it [a proxy](running-as-a-service.md#behind-a-proxy).

## Where Ladderback keeps your files

Apart from the program itself, Ladderback keeps everything in your home folder:

- **Settings** (repositories, plans and notifications): `~/.config/ladderback` on Linux, `~/Library/Application Support/ladderback` on macOS.
- **Data** (the operation history, the admin password and this machine's ID): `~/.local/share/ladderback` on Linux, `~/Library/Application Support/ladderback` on macOS.
- **Cache**, which holds nothing Ladderback needs: `~/.cache/ladderback` on Linux, `~/Library/Caches/ladderback` on macOS.

On Linux, the settings folder appears once you first save a setting. On macOS the service logs to `~/Library/Logs/ladderback.log`.

## What removing leaves behind

Each system's page says how to remove Ladderback: stop its service with `ladderback remove-service`, then remove the program. That leaves your files in place, so that installing Ladderback again picks up where you left off, and so that removing a program never costs you a backup:

- **Your backups.** Each repository, with every snapshot in it, stays where it is. Ladderback never deletes a repository, and the `rustic` program still opens it with its password.
- **Ladderback's folders,** in the table above.
- **Passwords in the keyring.** A repository's password, or a notification's token, that you kept in the system keyring (the keychain on macOS) stays there, under the name `ladderback`.
- **Linger, on Linux.** If you ran `loginctl enable-linger` for Ladderback alone, `loginctl disable-linger` undoes it.
- **A proxy you gave the service.** On Linux, delete `~/.config/systemd/user/ladderback.service.d`, which holds it. On macOS, `launchctl unsetenv` each variable, and remove the launch agent of your own that sets them at each login ([Running as a service](running-as-a-service.md#behind-a-proxy)).

Before you delete anything more, keep each repository's password somewhere safe if you want its backups: without the password nobody can read a repository, and the settings folder or the keyring may be the only place it is. Then, to remove Ladderback's own files too:

```sh
# Linux:
rm -rf ~/.config/ladderback ~/.local/share/ladderback ~/.cache/ladderback
# macOS:
rm -rf ~/Library/Application\ Support/ladderback ~/Library/Caches/ladderback ~/Library/Logs/ladderback.log
```

## Windows and WSL2

Windows isn't supported. Ladderback runs inside WSL2 as a Linux install, for files inside WSL, installed as the page for that Linux says. It isn't a way to back up Windows: WSL stops when idle, so scheduled backups don't run unless something keeps it running, and files under `/mnt/c` are read without shadow copies or Windows metadata (ACLs, attributes, alternate data streams), so open files may fail and a restore to Windows loses that metadata. `ladderback install-service` needs systemd, which WSL starts when `/etc/wsl.conf` has `systemd=true` under `[boot]`, as Ubuntu from `wsl --install` already does. Ladderback hasn't been tried in WSL2 on a real machine yet ([#528](https://github.com/nledford/Ladderback/issues/528)).

## Sections that moved

Links from older releases lead here.

### macOS: Full Disk Access

Granting Full Disk Access is on [Install on macOS](install-macos.md#full-disk-access-and-photos).

### By hand

Building Ladderback from source by hand, and the tools the build needs, is in the Advanced part: [Other ways to install, By hand](other-ways-to-install.md#by-hand).

### Docker image (not supported yet)

The Docker image, which isn't yet a supported way to install and run Ladderback, has a page of its own in the Advanced part: [Docker image](docker.md).

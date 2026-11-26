# Docker image (not supported yet)

Each release after 0.5.0 publishes a Docker image for x86_64 and aarch64 Linux, as `ghcr.io/nledford/ladderback` and `docker.io/nledford/ladderback`: the same image, tagged with the release's version. Only GitHub's latest release also gets `latest`, so a pre-release never moves it; to try a pre-release, pull it by its version. For 0.5.0, or a build of your own, `just image <version> <archive>... --tag ghcr.io/nledford/ladderback --load`, in a checkout, builds it from a release archive, as a release does. Docker Hub limits how often you may pull without an account; GitHub's registry doesn't.

The image is there to try: it isn't yet a supported way to install and run Ladderback, and [the package or the install script](install.md) is. Its container is a machine of its own, which backs up the folders mounted into it. From the MVP's release it runs the hub too, and becomes the recommended way to run one on a machine whose only job is the hub; an installed machine becomes the hub in place instead, since moving its data into a container isn't supported yet ([decision 136](https://github.com/nledford/Ladderback/blob/main/docs/design/decisions/136-an-installed-machine-becomes-the-hub.md)). An image from an earlier release has no hub, and refuses `serve --hub`.

The image runs `ladderback` as user and group 10001 and listens on port 9898. It keeps its configuration in the volume at `/var/lib/ladderback/config`, its data (the operation history and the admin password among them) in the volume at `/var/lib/ladderback/data`, and its cache in `/var/cache/ladderback`. Turn sign-in on first: `set-password` asks twice for an admin password of at least 8 characters, without showing it, and keeps it in the data volume. With `-i` alone in place of `-it`, it reads one line from a pipe instead.

```sh
docker run --rm -it -v ladderback-data:/var/lib/ladderback/data ghcr.io/nledford/ladderback set-password
```

Then start the container, here on a computer that browsers reach as `nas.example.com`, backing up `/srv/photos`:

```sh
docker run --detach --name ladderback \
  --hostname nas --init --restart unless-stopped --stop-timeout 75 \
  --env TZ=Europe/Berlin --publish 9898:9898 \
  -v ladderback-config:/var/lib/ladderback/config \
  -v ladderback-data:/var/lib/ladderback/data \
  -v ladderback-cache:/var/cache/ladderback \
  -v /srv/photos:/srv/photos:ro \
  ghcr.io/nledford/ladderback \
  serve --listen 0.0.0.0:9898 --allowed-host nas.example.com --service-managed
```

The same container as a Compose file, with its port on loopback for a reverse proxy on the same computer, is [`examples/compose.yaml`](examples/compose.yaml): turn sign-in on with `docker compose run --rm ladderback set-password`, then start it with `docker compose up --detach`.

What each part is for:

- **`--allowed-host`** names the host name browsers reach Ladderback by. Listening beyond the container's loopback, Ladderback refuses to start without one, so the image's own command, `serve --listen 0.0.0.0:9898`, doesn't start until you give it ([Remote access](remote-access.md#listening-beyond-loopback)). Over plain HTTP the password crosses the network unencrypted, so put a reverse proxy that adds TLS in front. With the proxy on the same machine, publish the port on loopback only, `--publish 127.0.0.1:9898:9898`: Docker's own firewall rules pass by ufw and firewalld.
- **`--hostname`** stays the same from one container to the next. Ladderback names the machine, and its snapshots' host, by the container's hostname, which Docker makes up anew for each container unless you give one.
- **Each folder to back up is mounted read-only, at the path it has on the host.** The snapshots record the paths inside the container, so the same path keeps them meaningful to a restore and to `rustic`. A restore into a mounted folder needs it mounted without `:ro`, and writes files as the container's user, or with their original owners when it runs as root.
- **`--init`** runs a small init as process 1, which hands on `docker stop`'s signal and reaps the processes hooks leave behind. **`--stop-timeout 75`** gives Ladderback the 65 seconds it may take to stop cleanly, where Docker kills it after 10; in Compose, `stop_grace_period: 75s`.
- **`--restart unless-stopped`** stands in for `install-service`, which a container can't use, and `--service-managed` lets **Restart Ladderback** in Settings restart it: Ladderback exits, and Docker starts it again.
- **`TZ`** sets the time zone the schedules run in; without it, a container's is UTC.
- **The cache volume** is optional: it keeps the repositories' cache from one container to the next.

The container's user reads only what user 10001 or everyone may read. To back up a whole machine, run as root with `--user 0`: root reads every file mounted into the container, and the volumes need no change. To run as one of the host's users instead, with `--user <uid>:<gid>`, keep the configuration, data and cache in folders on the host that this user owns, made as that user:

```sh
install -d -m 0700 ~/ladderback/config ~/ladderback/data ~/ladderback/cache
```

Mount them in place of the volumes, as in `-v ~/ladderback/data:/var/lib/ladderback/data`, and give the same `--user`, in every `docker run`, `set-password`'s included. A named volume won't do: while it is empty, each mount gives it back to user 10001, so a `chown` of one doesn't last. Keep to one user, since the files root writes stay root's.

A container has no keyring, so keep a repository's password in a file, such as a Docker secret under `/run/secrets`, in an environment variable or behind a command ([Secrets](secrets.md)). A repository on a local path needs a volume or a folder on the host mounted at that path, as `/srv/photos` is above: one written anywhere else lives in the container and goes when the container is removed.

On macOS, Docker runs Linux in a virtual machine, so Ladderback there sees only the folders shared with it and gets no Full Disk Access, Photos library or keychain: use the [install script](install-macos.md). From the MVP's release, the same image runs the hub with `serve --hub --listen 0.0.0.0:9898 --allowed-host <name>` in place of the command above; [The hub](hub.md) says how to pair machines with a hub.

## Updating and removing

To update, pull the image, remove the container and run the same `docker run` again, with the same mounts: the volumes keep the configuration, the history and the password, and the repositories mounted into it stay where they are.

To remove it, remove the container: `docker rm --force ladderback`. Its volumes stay, with the configuration, the history and the admin password. Once you have each repository's password somewhere else, `docker volume rm ladderback-config ladderback-data ladderback-cache` deletes them too. The repositories, and the folders mounted into the container, stay where they are.
